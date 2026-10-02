use crate::{
    grants::{Grants, Options},
    io, mapping,
    protocol::{self, Id, MAX_INPUT, MAX_OUTPUT},
};
use rmcp::model::{
    DiscoverResult, ErrorCode, Implementation, InitializeResult, ListToolsResult, ProtocolVersion,
    ServerCapabilities,
};
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    fs::File,
    io::{Read, Write},
    os::fd::AsRawFd,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};
use workbench_core::RunControl;

fn capabilities() -> ServerCapabilities {
    let mut capabilities = ServerCapabilities::default();
    capabilities.tools = Some(Default::default());
    capabilities
}

const MAX_SLOTS: usize = 8;
const MAX_RESERVED: usize = 6 * 1024 * 1024;
const OUTPUT_TIMEOUT: Duration = Duration::from_secs(5);
const IO_SLICE: usize = 16 * 1024;

#[derive(PartialEq)]
enum Legacy {
    New,
    ReplyPending,
    Waiting,
    Ready,
}
struct Frame {
    id: Option<Id>,
    bytes: Vec<u8>,
    written: usize,
    deadline: Instant,
    initialize: bool,
}
struct Active {
    id: Id,
    control: RunControl,
    thread: JoinHandle<Result<WorkerOutcome, ()>>,
    cancelled: bool,
}
struct WorkerOutcome {
    bytes: Vec<u8>,
    cleanup_confirmed: bool,
}
struct Server {
    grants: Arc<Grants>,
    queue: VecDeque<Frame>,
    active: Option<Active>,
    legacy: Legacy,
    cleanup_unconfirmed: bool,
    drain_deadline: Option<Instant>,
}

fn cancellation_id(params: &Value) -> Option<Id> {
    let fields = params.as_object()?;
    if fields
        .keys()
        .any(|key| !matches!(key.as_str(), "requestId" | "reason" | "_meta"))
        || params
            .get("reason")
            .is_some_and(|reason| !reason.is_string())
        || params.get("_meta").is_some_and(|meta| !meta.is_object())
    {
        return None;
    }
    Id::parse(&params["requestId"])
}

fn finish_work(
    id: &Id,
    modern: bool,
    result: &workbench_core::result::OperationResult,
) -> Result<WorkerOutcome, ()> {
    // Receipt delivery and worker termination do not prove owned process cleanup. Preserve the
    // core's explicit uncertainty even when cancellation suppresses this receipt on the wire.
    let cleanup_confirmed = !result
        .errors
        .iter()
        .any(|error| error.code == "cleanup_unconfirmed")
        && result
            .data
            .get("cleanup")
            .is_none_or(|cleanup| cleanup["confirmed"] == true)
        && result
            .data
            .get("supervisor")
            .filter(|value| !value.is_null())
            .is_none_or(|supervisor| supervisor["cleanup"]["confirmed"] == true);
    Ok(WorkerOutcome {
        bytes: protocol::receipt(id, modern, result)?,
        cleanup_confirmed,
    })
}

pub(crate) fn serve(options: Options) -> u8 {
    let (input, output) = match (io::pipe(0, true), io::pipe(1, false)) {
        (Ok(i), Ok(o)) => (i, o),
        _ => {
            crate::invalid_startup();
            return 2;
        }
    };
    let signal = Arc::new(AtomicUsize::new(0));
    let mut registrations = Vec::new();
    for number in [signal_hook::consts::SIGINT, signal_hook::consts::SIGTERM] {
        match signal_hook::flag::register_usize(number, Arc::clone(&signal), number as usize) {
            Ok(id) => registrations.push(id),
            Err(_) => {
                for id in registrations {
                    signal_hook::low_level::unregister(id);
                }
                crate::invalid_startup();
                return 2;
            }
        }
    }
    let old_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {
        io::diagnostic(b"{\"event\":\"mcp_internal_failure\",\"outcome\":\"unconfirmed\"}\n")
    }));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let grants = match Grants::new(&options) {
            Ok(grants) => grants,
            Err(_) => {
                crate::invalid_startup();
                return 2;
            }
        };
        let mut server = Server {
            grants: Arc::new(grants),
            queue: VecDeque::new(),
            active: None,
            legacy: Legacy::New,
            cleanup_unconfirmed: false,
            drain_deadline: None,
        };
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            server.run(input, output, &signal)
        }));
        let clean = server.cleanup(signal.load(Ordering::Relaxed));
        if !clean {
            io::diagnostic(b"{\"event\":\"mcp_cleanup_unconfirmed\"}\n");
            // An unjoinable worker must not survive this adapter in a detached thread.
            std::process::exit(1);
        }
        match signal.load(Ordering::Relaxed) {
            2 => 130,
            15 => 143,
            _ => result.unwrap_or(1),
        }
    }))
    .unwrap_or(1);
    std::panic::set_hook(old_hook);
    for id in registrations {
        signal_hook::low_level::unregister(id);
    }
    result
}

impl Server {
    fn mark_cleanup_unconfirmed(&mut self) {
        self.cleanup_unconfirmed = true;
        self.drain_deadline
            .get_or_insert_with(|| Instant::now() + OUTPUT_TIMEOUT);
    }
    fn capacity(&self, extra: usize) -> bool {
        self.queue.len() + usize::from(self.active.is_some()) < MAX_SLOTS
            && self.queue.iter().map(|f| f.bytes.len()).sum::<usize>()
                + if self.active.is_some() { MAX_OUTPUT } else { 0 }
                + extra
                <= MAX_RESERVED
    }
    fn enqueue(&mut self, id: Option<Id>, bytes: Vec<u8>, initialize: bool) -> Result<(), ()> {
        if !self.capacity(bytes.len()) {
            return Err(());
        }
        self.queue.push_back(Frame {
            id,
            bytes,
            written: 0,
            deadline: Instant::now() + OUTPUT_TIMEOUT,
            initialize,
        });
        Ok(())
    }
    fn error(&mut self, id: Option<Id>, code: ErrorCode, message: &'static str) -> Result<(), ()> {
        let bytes = protocol::error(id.as_ref(), code, message)?;
        self.enqueue(id, bytes, false)
    }
    fn collision(&self, id: &Id) -> bool {
        self.active.as_ref().is_some_and(|a| &a.id == id)
            || self.queue.iter().any(|f| f.id.as_ref() == Some(id))
    }
    fn cancel(&mut self, id: &Id) -> Result<(), ()> {
        if let Some(position) = self.queue.iter().position(|f| f.id.as_ref() == Some(id)) {
            if self.queue[position].initialize {
                return Ok(());
            }
            if self.queue[position].written != 0 {
                return Err(());
            }
            self.queue.remove(position);
        }
        if let Some(active) = self.active.as_mut().filter(|a| &a.id == id) {
            active.cancelled = true;
            active.control.signal.store(15, Ordering::Relaxed);
        }
        Ok(())
    }
    fn complete(&mut self) -> Result<(), ()> {
        if self.active.as_ref().is_none_or(|a| !a.thread.is_finished()) {
            return Ok(());
        }
        let active = self.active.take().expect("finished active worker");
        let outcome = active.thread.join();
        if !matches!(outcome, Ok(Ok(_))) {
            self.mark_cleanup_unconfirmed();
            active.control.signal.store(15, Ordering::Relaxed);
            io::diagnostic(b"{\"event\":\"mcp_worker_failure\",\"cleanup\":\"unconfirmed\"}\n");
            return Err(());
        }
        let outcome = outcome.map_err(|_| ())??;
        if !outcome.cleanup_confirmed {
            self.mark_cleanup_unconfirmed();
        }
        if !active.cancelled {
            self.enqueue(Some(active.id), outcome.bytes, false)?;
        }
        Ok(())
    }
    fn cleanup(&mut self, signal: usize) -> bool {
        self.queue.clear();
        let Some(active) = self.active.take() else {
            return !self.cleanup_unconfirmed;
        };
        active
            .control
            .signal
            .store(if signal == 0 { 15 } else { signal }, Ordering::Relaxed);
        let deadline = Instant::now() + Duration::from_secs(5);
        while !active.thread.is_finished() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        if !active.thread.is_finished() {
            return false;
        }
        active
            .thread
            .join()
            .is_ok_and(|result| result.is_ok_and(|outcome| outcome.cleanup_confirmed))
            && !self.cleanup_unconfirmed
    }
    fn frame(&mut self, bytes: &[u8], output_fd: i32) -> Result<(), ()> {
        // The terminal drain has no request/error path. Only a valid cancellation can retract
        // queued output; malformed input cannot append replies, advance initialization or renew
        // its fixed deadline. Keep this branch before normal parsing and envelope errors.
        if self.cleanup_unconfirmed {
            if let Ok(value) = workbench_core::request::parse_bounded_json(bytes, 24)
                && value["jsonrpc"] == "2.0"
                && value["method"] == "notifications/cancelled"
                && value.get("id").is_none()
                && value.as_object().is_some_and(|object| {
                    object
                        .keys()
                        .all(|key| matches!(key.as_str(), "jsonrpc" | "method" | "params"))
                })
                && let Some(id) = cancellation_id(&value["params"])
            {
                self.cancel(&id)?;
            }
            return Ok(());
        }
        let value = match workbench_core::request::parse_bounded_json(bytes, 24) {
            Ok(value) => value,
            Err(_) => {
                return self.error(
                    None,
                    ErrorCode::PARSE_ERROR,
                    "Malformed, duplicate-key or over-depth JSON.",
                );
            }
        };
        let Some(object) = value.as_object() else {
            return self.error(
                None,
                ErrorCode::INVALID_REQUEST,
                "A JSON-RPC object is required.",
            );
        };
        let id = object.get("id").and_then(Id::parse);
        // Collisions close the stream even when the duplicate request has malformed params.
        if id.as_ref().is_some_and(|id| self.collision(id)) {
            return Err(());
        }
        if object.get("id").is_some() && id.is_none() {
            return self.error(None, ErrorCode::INVALID_REQUEST, "Request ID is invalid.");
        }
        let method = object.get("method").and_then(Value::as_str);
        if value["jsonrpc"] != "2.0"
            || method.is_none()
            || object
                .keys()
                .any(|k| !matches!(k.as_str(), "jsonrpc" | "id" | "method" | "params"))
        {
            return self.error(id, ErrorCode::INVALID_REQUEST, "Invalid JSON-RPC envelope.");
        }
        let method = method.expect("checked method");
        let params = object.get("params").cloned().unwrap_or_else(|| json!({}));
        if id.is_none() {
            if !params.is_object() || params.get("_meta").is_some_and(|meta| !meta.is_object()) {
                return Ok(());
            }
            if method == "notifications/cancelled" {
                if let Some(id) = cancellation_id(&params) {
                    self.cancel(&id)?;
                }
            } else if method == "notifications/initialized"
                && self.legacy == Legacy::Waiting
                && params
                    .as_object()
                    .is_some_and(|p| p.keys().all(|k| k == "_meta"))
            {
                self.legacy = Legacy::Ready;
            }
            return Ok(());
        }
        let id = id.expect("request id");
        if !self.capacity(0) {
            return Err(());
        }
        if !params.is_object() {
            return self.error(
                Some(id),
                ErrorCode::INVALID_PARAMS,
                "Parameters must be an object.",
            );
        }
        let modern = match protocol::modern(&params) {
            Ok(modern) => modern,
            Err(Some(version)) => {
                let bytes = protocol::unsupported(&id, version)?;
                return self.enqueue(Some(id), bytes, false);
            }
            Err(None) => {
                return self.error(
                    Some(id),
                    ErrorCode::INVALID_PARAMS,
                    "Modern request metadata is missing or invalid.",
                );
            }
        };
        if method == "initialize" {
            if modern || self.legacy != Legacy::New {
                return self.error(
                    Some(id),
                    ErrorCode::INVALID_REQUEST,
                    "Legacy initialization is unavailable in this state.",
                );
            }
            if !params["protocolVersion"].is_string()
                || !params["capabilities"].is_object()
                || !params["clientInfo"].is_object()
                || params.as_object().is_none_or(|p| {
                    p.keys().any(|k| {
                        !matches!(
                            k.as_str(),
                            "protocolVersion" | "capabilities" | "clientInfo" | "_meta"
                        )
                    })
                })
                || serde_json::from_value::<rmcp::model::InitializeRequestParams>(params).is_err()
            {
                return self.error(
                    Some(id),
                    ErrorCode::INVALID_PARAMS,
                    "Invalid initialization parameters.",
                );
            }
            let result = InitializeResult::new(capabilities())
                .with_protocol_version(ProtocolVersion::V_2025_11_25)
                .with_server_info(Implementation::new(
                    "agenticsre-workbench",
                    env!("CARGO_PKG_VERSION"),
                ));
            let bytes = protocol::reply(&id, &result)?;
            self.enqueue(Some(id), bytes, true)?;
            self.legacy = Legacy::ReplyPending;
            return Ok(());
        }
        if !modern && self.legacy != Legacy::Ready {
            return self.error(
                Some(id),
                if self.legacy == Legacy::New {
                    ErrorCode::INVALID_PARAMS
                } else {
                    ErrorCode::INVALID_REQUEST
                },
                "Per-request modern metadata or completed legacy initialization is required.",
            );
        }
        let keys: &[&str] = match method {
            "tools/call" => &["name", "arguments", "_meta"],
            "tools/list" => &["cursor", "_meta"],
            "ping" => &["_meta"],
            "server/discover" if modern => &["_meta"],
            _ => {
                return self.error(
                    Some(id),
                    ErrorCode::METHOD_NOT_FOUND,
                    "Method is not supported.",
                );
            }
        };
        if params
            .as_object()
            .expect("object params")
            .keys()
            .any(|k| !keys.contains(&k.as_str()))
        {
            return self.error(
                Some(id),
                ErrorCode::INVALID_PARAMS,
                "Unknown parameter fields.",
            );
        }
        match method {
            "ping" => {
                let result = if modern {
                    json!({"resultType":"complete"})
                } else {
                    json!({})
                };
                self.enqueue(Some(id.clone()), protocol::reply(&id, &result)?, false)
            }
            "server/discover" if modern => {
                let mut result = DiscoverResult::new(
                    vec![ProtocolVersion::V_2026_07_28, ProtocolVersion::V_2025_11_25],
                    capabilities(),
                );
                result.set_server_info(Implementation::new(
                    "agenticsre-workbench",
                    env!("CARGO_PKG_VERSION"),
                ));
                self.enqueue(Some(id.clone()), protocol::reply(&id, &result)?, false)
            }
            "tools/list" => {
                if params.get("cursor").is_some() {
                    return self.error(
                        Some(id),
                        ErrorCode::INVALID_PARAMS,
                        "This bounded tool list has no continuation cursor.",
                    );
                }
                let mut result = ListToolsResult::with_all_items(mapping::tools(&self.grants));
                if modern {
                    result.ttl_ms = Some(0);
                    result.cache_scope = Some(rmcp::model::CacheScope::Private);
                } else {
                    result.result_type = None;
                }
                self.enqueue(Some(id.clone()), protocol::reply(&id, &result)?, false)
            }
            "tools/call" => {
                let Some(name) = params["name"].as_str() else {
                    return self.error(
                        Some(id),
                        ErrorCode::INVALID_PARAMS,
                        "A granted tool name is required.",
                    );
                };
                let arguments = params
                    .get("arguments")
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                let request = match mapping::normalize(name, arguments, &self.grants) {
                    Ok(request) => request,
                    Err(message) => {
                        return self.error(Some(id), ErrorCode::INVALID_PARAMS, message);
                    }
                };
                if self.active.is_some() {
                    return self.error(Some(id), ErrorCode(1001), "workbench_busy");
                }
                if !self.capacity(MAX_OUTPUT) {
                    return Err(());
                }
                let control = RunControl {
                    signal: Arc::new(AtomicUsize::new(0)),
                    result_fd: Some(output_fd),
                };
                let worker_control = RunControl {
                    signal: Arc::clone(&control.signal),
                    result_fd: control.result_fd,
                };
                let grants = Arc::clone(&self.grants);
                let worker_id = id.clone();
                let thread = std::thread::Builder::new()
                    .name("workbench-mcp-core".into())
                    .spawn(move || {
                        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            let result = workbench_core::execute_with_authority_identities(
                                request,
                                &worker_control,
                                &grants.context,
                                grants.policy_digest.as_deref(),
                                grants.grafana_identity.as_ref(),
                            );
                            finish_work(&worker_id, modern, &result)
                        }))
                        .map_err(|_| ())?
                    })
                    .map_err(|_| ())?;
                self.active = Some(Active {
                    id,
                    control,
                    thread,
                    cancelled: false,
                });
                Ok(())
            }
            _ => self.error(
                Some(id),
                ErrorCode::METHOD_NOT_FOUND,
                "Method is not supported.",
            ),
        }
    }
    fn run(&mut self, mut input: File, mut output: File, signal: &AtomicUsize) -> u8 {
        let mut buffer = Vec::with_capacity(MAX_INPUT);
        let mut chunk = [0u8; IO_SLICE];
        let mut saturated_since = None;
        loop {
            if self.cleanup_unconfirmed
                && (self.queue.is_empty()
                    || self
                        .drain_deadline
                        .is_some_and(|deadline| Instant::now() >= deadline))
            {
                return 1;
            }
            if signal.load(Ordering::Relaxed) != 0 {
                return 1;
            }
            if self
                .queue
                .front()
                .is_some_and(|f| Instant::now() >= f.deadline)
            {
                return 1;
            }
            let (read_events, write_events) =
                match io::poll(&input, &output, true, !self.queue.is_empty()) {
                    Ok(events) => events,
                    Err(_) => return 1,
                };
            if write_events & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
                return 1;
            }
            // Read bounded input before completing/writing responses so queued cancellation wins.
            if read_events & (libc::POLLIN | libc::POLLHUP) != 0 {
                match input.read(&mut chunk) {
                    Ok(0) => return if buffer.is_empty() { 0 } else { 1 },
                    Ok(size) => {
                        for byte in &chunk[..size] {
                            if *byte == b'\n' {
                                if self.frame(&buffer, output.as_raw_fd()).is_err() {
                                    return 1;
                                }
                                buffer.clear();
                            } else {
                                if buffer.len() >= MAX_INPUT {
                                    return 1;
                                }
                                buffer.push(*byte);
                            }
                        }
                    }
                    Err(error)
                        if matches!(
                            error.kind(),
                            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                        ) => {}
                    Err(_) => return 1,
                }
            }
            // With no queued response, POLLOUT must still be sampled while work is active.
            // A separate zero-time probe retains the main poll's25ms wait instead of spinning
            // on a normally writable pipe. No inherited descriptor flags are changed.
            if self.active.is_some() {
                match io::output_ready(&output) {
                    Ok(true) => saturated_since = None,
                    Ok(false) => {
                        let since = saturated_since.get_or_insert_with(Instant::now);
                        if since.elapsed() >= OUTPUT_TIMEOUT {
                            return 1;
                        }
                    }
                    Err(_) => return 1,
                }
            } else {
                saturated_since = None;
            }
            if read_events & (libc::POLLERR | libc::POLLNVAL) != 0 || self.complete().is_err() {
                return 1;
            }
            if let Some(frame) = self.queue.front_mut() {
                let end = (frame.written + IO_SLICE).min(frame.bytes.len());
                match output.write(&frame.bytes[frame.written..end]) {
                    Ok(0) => return 1,
                    Ok(size) => {
                        frame.written += size;
                        if frame.written == frame.bytes.len() {
                            let frame = self.queue.pop_front().expect("completed frame");
                            if frame.initialize {
                                self.legacy = Legacy::Waiting;
                            }
                        }
                    }
                    Err(error)
                        if matches!(
                            error.kind(),
                            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                        ) => {}
                    Err(_) => return 1,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Barrier, mpsc};

    fn server() -> Server {
        Server {
            grants: Arc::new(
                Grants::new(&Options {
                    allow: vec!["task.run".into()],
                    ..Options::default()
                })
                .unwrap(),
            ),
            queue: VecDeque::new(),
            active: None,
            legacy: Legacy::Ready,
            cleanup_unconfirmed: false,
            drain_deadline: None,
        }
    }
    fn task(id: i64) -> Vec<u8> {
        serde_json::to_vec(&json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":"workbench_task_run","arguments":{"id":"error-budget","version":1,"input":{"slo":99.9}}}})).unwrap()
    }
    #[test]
    fn cancellation_keeps_worker_busy_until_owned_cleanup_and_suppresses_result() {
        let mut server = server();
        let signal = Arc::new(AtomicUsize::new(0));
        let worker_signal = signal.clone();
        let release = Arc::new(Barrier::new(2));
        let worker_release = release.clone();
        let (observed, receiver) = mpsc::sync_channel(1);
        let thread = std::thread::spawn(move || {
            while worker_signal.load(Ordering::Relaxed) == 0 {
                std::thread::yield_now();
            }
            observed.send(()).unwrap();
            worker_release.wait();
            Ok(WorkerOutcome {
                bytes: b"cancelled worker bytes must never be sent\n".to_vec(),
                cleanup_confirmed: true,
            })
        });
        server.active = Some(Active {
            id: Id::Number(1),
            control: RunControl {
                signal: signal.clone(),
                result_fd: None,
            },
            thread,
            cancelled: false,
        });
        server
            .frame(
                br#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":1}}"#,
                1,
            )
            .unwrap();
        receiver.recv_timeout(Duration::from_secs(1)).unwrap();
        server.frame(&task(2), 1).unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&server.queue[0].bytes).unwrap()["error"]["code"],
            1001
        );
        assert!(
            server.collision(&Id::Number(1)),
            "cancelled ID retained until join"
        );
        release.wait();
        let deadline = Instant::now() + Duration::from_secs(1);
        while server
            .active
            .as_ref()
            .is_some_and(|a| !a.thread.is_finished())
        {
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
        server.complete().unwrap();
        assert!(!server.collision(&Id::Number(1)));
        assert_eq!(server.queue.len(), 1, "no cancelled reply queued");
        assert_eq!(signal.load(Ordering::Relaxed), 15);
    }
    #[test]
    fn serialized_capacity_and_request_slots_are_live_not_lifetime_bounded() {
        let mut server = server();
        server
            .enqueue(Some(Id::Number(1)), vec![b' '; MAX_OUTPUT], false)
            .unwrap();
        server
            .enqueue(Some(Id::Number(2)), vec![b' '; MAX_OUTPUT], false)
            .unwrap();
        assert!(server.enqueue(Some(Id::Number(3)), vec![0], false).is_err());
        server.queue.clear();
        for id in 0..MAX_SLOTS {
            server
                .enqueue(Some(Id::Number(id as i64)), b"{}\n".to_vec(), false)
                .unwrap();
        }
        assert!(
            server
                .enqueue(Some(Id::Number(9)), b"{}\n".to_vec(), false)
                .is_err()
        );
        server.queue.pop_front();
        server
            .enqueue(Some(Id::Number(0)), b"{}\n".to_vec(), false)
            .unwrap();
    }
    #[test]
    fn partial_frame_cancellation_closes_transport_and_worker_failure_has_no_receipt() {
        let mut server = server();
        server
            .enqueue(
                Some(Id::Number(1)),
                b"{\"jsonrpc\":\"2.0\"}\n".to_vec(),
                false,
            )
            .unwrap();
        server.queue.front_mut().unwrap().written = 1;
        assert!(server.cancel(&Id::Number(1)).is_err());
        server.queue.clear();
        let signal = Arc::new(AtomicUsize::new(0));
        server.active = Some(Active {
            id: Id::Number(2),
            control: RunControl {
                signal: signal.clone(),
                result_fd: None,
            },
            thread: std::thread::spawn(|| Err(())),
            cancelled: false,
        });
        let deadline = Instant::now() + Duration::from_secs(1);
        while !server.active.as_ref().unwrap().thread.is_finished() {
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
        assert!(server.complete().is_err());
        assert!(server.queue.is_empty());
        assert_eq!(signal.load(Ordering::Relaxed), 15);
    }
    fn unconfirmed_worker(cancelled: bool) -> Active {
        unconfirmed_worker_with_output(cancelled, 0)
    }
    fn unconfirmed_worker_with_output(cancelled: bool, output_bytes: usize) -> Active {
        let mut receipt = workbench_core::result::OperationResult::new(
            &workbench_core::request::Request::new("task.run", json!({})),
        );
        receipt.execution.status = workbench_core::result::Status::Unknown;
        receipt.error(
            "cleanup_unconfirmed",
            "Synthetic core receipt with unconfirmed owned cleanup.",
        );
        receipt.data = json!({"supervisor":{"cleanup":{"confirmed":false}}});
        receipt.output.stdout = "x".repeat(output_bytes);
        let thread = std::thread::spawn(move || finish_work(&Id::Number(42), true, &receipt));
        let deadline = Instant::now() + Duration::from_secs(1);
        while !thread.is_finished() {
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
        Active {
            id: Id::Number(42),
            control: RunControl::default(),
            thread,
            cancelled,
        }
    }
    #[test]
    fn core_cleanup_uncertainty_closes_admission_after_normal_completion() {
        let mut server = server();
        server.active = Some(unconfirmed_worker(false));
        server.complete().unwrap();
        let response: Value = serde_json::from_slice(&server.queue[0].bytes).unwrap();
        assert_eq!(
            response["result"]["structuredContent"]["errors"][0]["code"],
            "cleanup_unconfirmed"
        );
        server
            .frame(br#"{"jsonrpc":"2.0","id":2,"method":"ping"}"#, 1)
            .unwrap();
        assert_eq!(
            server.queue.len(),
            1,
            "unconfirmed cleanup must close admission"
        );
        assert!(server.active.is_none());
        assert!(
            !server.cleanup(0),
            "normal completion is not cleanup confirmation"
        );
    }
    #[test]
    fn core_cleanup_uncertainty_survives_cancelled_result_suppression() {
        let mut server = server();
        server.active = Some(unconfirmed_worker(true));
        server.complete().unwrap();
        assert!(server.queue.is_empty());
        assert!(
            !server.cleanup(0),
            "cancelled receipt suppression must not erase cleanup uncertainty"
        );
    }
    #[test]
    fn eof_cleanup_does_not_equate_worker_join_with_core_cleanup() {
        let mut server = server();
        server.active = Some(unconfirmed_worker(false));
        assert!(
            !server.cleanup(0),
            "EOF must not report successful cleanup for cleanup_unconfirmed"
        );
    }
    #[test]
    fn malformed_notification_metadata_cannot_cancel_or_initialize() {
        for meta in [json!(false), json!(null), json!([]), json!("malformed")] {
            let mut server = server();
            server
                .enqueue(Some(Id::Number(42)), b"{}\n".to_vec(), false)
                .unwrap();
            server.frame(&serde_json::to_vec(&json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":42,"_meta":meta}})).unwrap(),1).unwrap();
            assert_eq!(
                server.queue.len(),
                1,
                "malformed notification metadata must not suppress output"
            );
            server.legacy = Legacy::Waiting;
            server.frame(&serde_json::to_vec(&json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{"_meta":meta}})).unwrap(),1).unwrap();
            assert!(
                server.legacy == Legacy::Waiting,
                "malformed notification metadata must not initialize"
            );
        }
    }
    #[test]
    fn nested_supervisor_uncertainty_is_not_hidden_by_missing_top_level_error() {
        for operation in ["task.run", "process.exec"] {
            let mut receipt = workbench_core::result::OperationResult::new(
                &workbench_core::request::Request::new(operation, json!({})),
            );
            receipt.data = json!({"supervisor":{"process_id":123,"cleanup":{"confirmed":false}}});
            assert!(
                !finish_work(&Id::Number(1), true, &receipt)
                    .unwrap()
                    .cleanup_confirmed
            );
            receipt.data["supervisor"]["cleanup"]["confirmed"] = json!(true);
            assert!(
                finish_work(&Id::Number(1), true, &receipt)
                    .unwrap()
                    .cleanup_confirmed
            );
        }
    }
    #[test]
    fn normal_and_cancelled_uncertain_results_end_real_loop_unsuccessfully() {
        use std::os::fd::FromRawFd;
        fn pipe() -> (File, File) {
            let mut fds = [0; 2];
            // SAFETY: two initialized slots receive new pipe descriptors owned by this test.
            let created =
                unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC | libc::O_NONBLOCK) };
            assert_eq!(created, 0);
            // SAFETY: successful pipe2 returned two new descriptors, each transferred exactly once.
            unsafe { (File::from_raw_fd(fds[0]), File::from_raw_fd(fds[1])) }
        }
        for cancelled in [false, true] {
            let (input, _input_writer) = pipe();
            let (mut output_reader, output) = pipe();
            let mut server = server();
            server.active = Some(unconfirmed_worker(cancelled));
            assert_eq!(server.run(input, output, &AtomicUsize::new(0)), 1);
            assert!(!server.cleanup(0));
            let mut bytes = Vec::new();
            output_reader.read_to_end(&mut bytes).unwrap();
            if cancelled {
                assert!(bytes.is_empty());
            } else {
                let value: Value = serde_json::from_slice(&bytes).unwrap();
                assert_eq!(
                    value["result"]["structuredContent"]["errors"][0]["code"],
                    "cleanup_unconfirmed"
                );
                assert_eq!(
                    value["result"]["structuredContent"]["data"]["supervisor"]["cleanup"]["confirmed"],
                    false
                );
            }
        }
    }
    fn assert_terminal_input_cannot_extend_queue(bytes: &[u8]) {
        let mut server = server();
        server.active = Some(unconfirmed_worker(false));
        server.complete().unwrap();
        let original = server.queue[0].bytes.clone();
        let original_deadline = server.queue[0].deadline;
        let original_drain_deadline = server.drain_deadline;
        let _ = server.frame(bytes, 1);
        assert_eq!(
            server.queue.len(),
            1,
            "terminal input must not append a response"
        );
        assert_eq!(server.queue[0].bytes, original);
        assert_eq!(server.queue[0].deadline, original_deadline);
        assert_eq!(server.drain_deadline, original_drain_deadline);
        assert!(server.active.is_none());
        assert!(server.cleanup_unconfirmed);
    }
    #[test]
    fn terminal_drain_ignores_malformed_json() {
        assert_terminal_input_cannot_extend_queue(b"{");
    }
    #[test]
    fn terminal_drain_ignores_non_object_json() {
        assert_terminal_input_cannot_extend_queue(b"[]");
    }
    #[test]
    fn terminal_drain_ignores_invalid_ids() {
        assert_terminal_input_cannot_extend_queue(
            br#"{"jsonrpc":"2.0","id":false,"method":"ping"}"#,
        );
    }
    #[test]
    fn terminal_drain_ignores_invalid_envelopes() {
        assert_terminal_input_cannot_extend_queue(br#"{"jsonrpc":"wrong","id":2,"method":"ping"}"#);
    }
    #[test]
    fn terminal_drain_paced_malformed_input_cannot_append_to_real_pipe_output() {
        use std::os::fd::FromRawFd;
        fn pipe() -> (File, File) {
            let mut fds = [0; 2];
            // SAFETY: pipe2 writes two new owned descriptors into the supplied array.
            let created =
                unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC | libc::O_NONBLOCK) };
            assert_eq!(created, 0);
            // SAFETY: both successfully-created descriptors are transferred exactly once.
            unsafe { (File::from_raw_fd(fds[0]), File::from_raw_fd(fds[1])) }
        }
        let (input, mut input_writer) = pipe();
        let (mut output_reader, output) = pipe();
        let mut server = server();
        server.active = Some(unconfirmed_worker_with_output(false, 128 * 1024));
        server.complete().unwrap();
        let expected = server.queue[0].bytes.clone();
        let signal = Arc::new(AtomicUsize::new(0));
        let thread_signal = signal.clone();
        let malformed: [&[u8]; 4] = [
            b"{\n",
            b"[]\n",
            b"{\"jsonrpc\":\"2.0\",\"id\":false,\"method\":\"ping\"}\n",
            b"{\"jsonrpc\":\"wrong\",\"id\":2,\"method\":\"ping\"}\n",
        ];
        input_writer.write_all(malformed[0]).unwrap();
        let worker = std::thread::spawn(move || {
            let status = server.run(input, output, &thread_signal);
            (status, server.cleanup(0))
        });
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut received = Vec::new();
        let mut chunk = [0; 4096];
        let mut next_input = 1;
        let mut output_closed = false;
        while Instant::now() < deadline {
            match output_reader.read(&mut chunk) {
                Ok(0) => {
                    output_closed = true;
                    break;
                }
                Ok(count) => {
                    received.extend_from_slice(&chunk[..count]);
                    if next_input < malformed.len() {
                        let _ = input_writer.write_all(malformed[next_input]);
                        next_input += 1;
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(error) => panic!("owned test pipe read: {error}"),
            }
            // Pace actual pipe drainage so all malformed classes arrive before the original
            // receipt finishes; this is real nonblocking I/O rather than a simulated timer.
            std::thread::sleep(Duration::from_millis(5));
        }
        signal.store(15, Ordering::Relaxed);
        drop(input_writer);
        let (status, clean) = worker.join().unwrap();
        assert!(
            output_closed,
            "terminal drain did not stop after its original response"
        );
        assert_eq!(
            next_input,
            malformed.len(),
            "all paced inputs must reach the active drain"
        );
        assert_eq!(status, 1);
        assert!(!clean);
        assert_eq!(
            received.len(),
            expected.len(),
            "terminal input must neither append responses nor truncate the original receipt"
        );
        assert!(
            received == expected,
            "the original honest receipt must remain unchanged"
        );
    }
    #[test]
    fn terminal_drain_ignores_work_and_preserves_only_valid_matching_cancellation() {
        assert_terminal_input_cannot_extend_queue(&task(2));
        for input in [
            br#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":42,"_meta":false}}"#.as_slice(),
            br#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":42,"reason":false}}"#.as_slice(),
            br#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":42,"requestId":1}}"#.as_slice(),
            br#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":1}}"#.as_slice(),
            br#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#.as_slice(),
        ] {assert_terminal_input_cannot_extend_queue(input);}
        for written in [0, 1] {
            let mut server = server();
            server.active = Some(unconfirmed_worker(false));
            server.complete().unwrap();
            let deadline = server.drain_deadline;
            server.queue[0].written = written;
            let result=server.frame(br#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":42}}"#,1);
            if written == 0 {
                assert!(result.is_ok());
                assert!(server.queue.is_empty());
            } else {
                assert!(
                    result.is_err(),
                    "partial frame cancellation must close the transport"
                );
            }
            assert_eq!(server.drain_deadline, deadline);
            assert!(!server.cleanup(0));
        }
    }
    #[test]
    fn terminal_drain_deadline_is_absolute_under_paced_malformed_input() {
        use std::os::fd::FromRawFd;
        use std::sync::atomic::AtomicBool;
        fn pipe() -> (File, File) {
            let mut fds = [0; 2];
            // SAFETY: pipe2 fills two valid descriptor slots; no preexisting descriptor is reused.
            let created =
                unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC | libc::O_NONBLOCK) };
            assert_eq!(created, 0);
            // SAFETY: these new descriptors each become exactly one owned File.
            unsafe { (File::from_raw_fd(fds[0]), File::from_raw_fd(fds[1])) }
        }
        let (input, mut input_writer) = pipe();
        let (mut output_reader, output) = pipe();
        let mut server = server();
        server.active = Some(unconfirmed_worker_with_output(false, 128 * 1024));
        server.complete().unwrap();
        let original = server.queue[0].bytes.clone();
        // Shorten only this private test deadline. Production remains five seconds, and the
        // original frame still has its later five-second delivery deadline.
        let start = Instant::now();
        server.drain_deadline = Some(start + Duration::from_millis(150));
        let finished = Arc::new(AtomicBool::new(false));
        let feeding_finished = finished.clone();
        let feeder = std::thread::spawn(move || {
            while !feeding_finished.load(Ordering::Relaxed) {
                if input_writer.write_all(b"[]\n").is_err() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        });
        let status = server.run(input, output, &AtomicUsize::new(0));
        let elapsed = start.elapsed();
        finished.store(true, Ordering::Relaxed);
        feeder.join().unwrap();
        let mut received = Vec::new();
        output_reader.read_to_end(&mut received).unwrap();
        assert_eq!(status, 1);
        assert!(!server.cleanup(0));
        assert!(
            elapsed >= Duration::from_millis(100),
            "must exercise the fixed deadline, not an input/queue refusal"
        );
        assert!(
            elapsed < Duration::from_secs(1),
            "paced input must not renew the terminal deadline"
        );
        assert!(
            !received.is_empty() && received.len() < original.len(),
            "deadline must interrupt actual partial delivery"
        );
        assert!(
            original.starts_with(&received),
            "only original receipt bytes may be written during terminal drain"
        );
    }
}
