//! Loopback-only browser adapter. The core remains the sole executor and receipt authority.
mod grants;
mod http;
mod input;
mod state;
mod assets {
    include!(concat!(env!("OUT_DIR"), "/assets.rs"));
}

use grants::Grants;
pub use grants::Options;
use hyper::server::conn::http1;
use hyper_util::rt::{TokioIo, TokioTimer};
use state::{SharedState, State, Work};
use std::{
    io::{Read, Write},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};
use tokio::{
    net::{TcpListener, TcpStream},
    task::JoinSet,
};

struct App {
    authority: String,
    origin: String,
    token: String,
    session: serde_json::Value,
    grants: Arc<Grants>,
    state: SharedState,
    sender: Mutex<Option<mpsc::SyncSender<Work>>>,
    diagnostics: Mutex<Option<std::fs::File>>,
    failed: AtomicBool,
}

fn diagnostics() -> Option<std::fs::File> {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // Reopening creates our own nonblocking file description, leaving the caller's stderr
        // flags unchanged. A full pipe drops bounded metadata instead of blocking HTTP handling.
        // Each independent diagnostic sink appends so it cannot overwrite existing log records.
        std::fs::OpenOptions::new()
            .append(true)
            .custom_flags(libc::O_NONBLOCK)
            .open("/proc/self/fd/2")
            .ok()
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

fn token() -> Result<String, &'static str> {
    let mut bytes = [0u8; 32];
    // Linux's kernel CSPRNG; no clock, process ID, deterministic seed or fallback.
    std::fs::File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut bytes))
        .map_err(|_| "kernel randomness is unavailable; no server was started")?;
    use base64::Engine;
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes))
}

/// Start the process-scoped local session. Startup errors never disclose policy or token bytes.
pub fn serve(options: Options) -> Result<u8, &'static str> {
    if !cfg!(target_os = "linux") {
        return Err("the browser adapter currently requires Linux");
    }
    let grants = Arc::new(Grants::new(&options)?);
    let token = token()?;
    let signal = Arc::new(AtomicUsize::new(0));
    #[cfg(unix)]
    let registrations = {
        let mut registered = Vec::new();
        for number in [signal_hook::consts::SIGINT, signal_hook::consts::SIGTERM] {
            match signal_hook::flag::register_usize(number, Arc::clone(&signal), number as usize) {
                Ok(id) => registered.push(id),
                Err(_) => {
                    for id in registered {
                        signal_hook::low_level::unregister(id);
                    }
                    return Err("shutdown signal handlers could not be initialized");
                }
            }
        }
        registered
    };
    let panic_sink = Mutex::new(diagnostics());
    let previous_panic_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |_| {
        // Never emit panic payloads/backtraces containing request data or token-bearing state.
        if let Ok(mut sink) = panic_sink.lock()
            && let Some(sink) = sink.as_mut()
        {
            let _ =
                sink.write(b"{\"event\":\"ui_internal_failure\",\"outcome\":\"unconfirmed\"}\n");
        }
    }));
    let result = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| "HTTP runtime could not be initialized")
        .and_then(|runtime| runtime.block_on(run(options.port, token, grants, signal)));
    std::panic::set_hook(previous_panic_hook);
    #[cfg(unix)]
    for id in registrations {
        signal_hook::low_level::unregister(id);
    }
    result
}

async fn run(
    port: u16,
    token: String,
    grants: Arc<Grants>,
    signal: Arc<AtomicUsize>,
) -> Result<u8, &'static str> {
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port))
        .await
        .map_err(|_| "the requested127.0.0.1 port could not be bound")?;
    let authority = listener
        .local_addr()
        .map_err(|_| "bound listener identity unavailable")?
        .to_string();
    let state = Arc::new(Mutex::new(State::default()));
    let (sender, receiver) = mpsc::sync_channel::<Work>(1);
    let worker_state = Arc::clone(&state);
    let worker_grants = Arc::clone(&grants);
    let worker = std::thread::Builder::new()
        .name("workbench-ui-core".into())
        .spawn(move || {
            run_worker(receiver, worker_state, |request, control| {
                workbench_core::execute_with_policy_identity(
                    request,
                    control,
                    &worker_grants.context,
                    worker_grants.policy_digest.as_deref(),
                )
            });
        })
        .map_err(|_| "the owned execution worker could not be started")?;
    let app = Arc::new(App {
        origin: format!("http://{authority}"),
        authority,
        session: grants.session(&workbench_core::result::new_id("session")),
        token,
        grants,
        state,
        sender: Mutex::new(Some(sender)),
        diagnostics: Mutex::new(diagnostics()),
        failed: AtomicBool::new(false),
    });
    let launch = format!("{}/#token={}\n", app.origin, app.token);
    if std::io::stdout()
        .write_all(launch.as_bytes())
        .and_then(|_| std::io::stdout().flush())
        .is_err()
    {
        app.sender.lock().expect("sender lock").take();
        let _ = worker.join();
        return Err("the launch link could not be delivered");
    }
    let mut connections = JoinSet::new();
    let mut tick = tokio::time::interval(Duration::from_millis(25));
    let mut accept_failed = false;
    loop {
        tokio::select! {
            _ = tick.tick() => {
                if signal.load(Ordering::Relaxed) !=0 || worker.is_finished() || app.failed.load(Ordering::Relaxed) { break; }
            },
            Some(_) = connections.join_next(), if !connections.is_empty() => {},
            accepted = listener.accept() => {
                let (socket,_) = match accepted { Ok(pair) => pair, Err(_) => { accept_failed=true; break; } };
                if connections.len() >=16 {
                    // No overload task or queue: parse/reject one bounded request within100ms.
                    serve_connection(socket, None, Duration::from_millis(100)).await;
                    continue;
                }
                let app=Arc::clone(&app);
                connections.spawn(serve_connection(socket, Some(app), Duration::from_secs(5)));
            }
        }
    }
    drop(listener);
    app.state
        .lock()
        .expect("state lock")
        .shutdown(match signal.load(Ordering::Relaxed) {
            0 => 15,
            number => number,
        });
    app.sender.lock().expect("sender lock").take();
    connections.abort_all();
    while connections.join_next().await.is_some() {}
    let deadline = Instant::now() + Duration::from_secs(5);
    while !worker.is_finished() && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    if !worker.is_finished() {
        return Err("worker cleanup exceeded five seconds; cleanup is unconfirmed");
    }
    if worker.join().is_err() {
        return Err("worker failed; cleanup is unconfirmed");
    }
    if app.failed.load(Ordering::Relaxed) {
        return Err("the execution worker became unavailable; outcome and cleanup are unconfirmed");
    }
    if accept_failed {
        return Err("the local listener failed; session stopped");
    }
    if app.state.lock().expect("state lock").stopped && signal.load(Ordering::Relaxed) == 0 {
        return Err("execution worker stopped unexpectedly; session stopped");
    }
    Ok(match signal.load(Ordering::Relaxed) {
        2 => 130,
        15 => 143,
        _ => 0,
    })
}

// The injected function is an internal seam for deterministic worker-failure tests. Production
// always passes the shared dispatcher; no wire field or startup flag can replace it.
fn run_worker(
    receiver: mpsc::Receiver<Work>,
    state: SharedState,
    mut execute: impl FnMut(
        workbench_core::request::Request,
        &workbench_core::RunControl,
    ) -> workbench_core::result::OperationResult,
) {
    while let Ok(work) = receiver.recv() {
        let receipt = execute(work.request, &work.control);
        state
            .lock()
            .expect("state lock")
            .finish(work.index, receipt);
    }
}

async fn serve_connection(socket: TcpStream, app: Option<Arc<App>>, deadline: Duration) {
    let service = hyper::service::service_fn(move |request| {
        let app = app.clone();
        async move {
            match app {
                Some(app) => http::handle(request, app).await,
                None => Ok::<_, std::convert::Infallible>(http::overload()),
            }
        }
    });
    let mut builder = http1::Builder::new();
    builder
        .keep_alive(false)
        .max_headers(32)
        .max_buf_size(16384)
        .timer(TokioTimer::new())
        .header_read_timeout(Duration::from_secs(2));
    let _ = tokio::time::timeout(
        deadline,
        builder.serve_connection(TokioIo::new(socket), service),
    )
    .await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[cfg(target_os = "linux")]
    #[test]
    fn diagnostics_preserve_redirected_log_and_stderr_flags() {
        use std::{
            fs::OpenOptions,
            os::fd::AsRawFd,
            process::{Command, Stdio},
        };
        const HTTP: &[u8] = b"{\"event\":\"ui_http\"}\n";
        const PANIC: &[u8] = b"{\"event\":\"ui_internal_failure\",\"outcome\":\"unconfirmed\"}\n";
        if std::env::var_os("WORKBENCH_TEST_DIAGNOSTICS_CHILD").is_some() {
            // SAFETY: stderr is the open fixture file inherited by this isolated test child.
            let original_flags = unsafe { libc::fcntl(2, libc::F_GETFL) };
            assert!(original_flags >= 0 && original_flags & libc::O_APPEND != 0);
            // Match serve(): the panic sink opens first, HTTP diagnostics are written first.
            let mut panic_sink = diagnostics().unwrap();
            let mut http_sink = diagnostics().unwrap();
            for sink in [&panic_sink, &http_sink] {
                // SAFETY: each borrowed File keeps this descriptor valid for the query.
                let flags = unsafe { libc::fcntl(sink.as_raw_fd(), libc::F_GETFL) };
                assert!(flags >= 0 && flags & libc::O_NONBLOCK != 0);
            }
            http_sink.write_all(HTTP).unwrap();
            panic_sink.write_all(PANIC).unwrap();
            // SAFETY: the inherited stderr descriptor remains open throughout this child.
            assert_eq!(unsafe { libc::fcntl(2, libc::F_GETFL) }, original_flags);
            return;
        }
        let path = std::env::temp_dir().join(format!(
            "workbench-ui-log-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut log = OpenOptions::new()
            .create_new(true)
            .append(true)
            .open(&path)
            .unwrap();
        log.write_all(b"existing-log-record\n").unwrap();
        let status = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "tests::diagnostics_preserve_redirected_log_and_stderr_flags",
            ])
            .env("WORKBENCH_TEST_DIAGNOSTICS_CHILD", "1")
            .stdout(Stdio::null())
            .stderr(log)
            .status()
            .unwrap();
        let contents = std::fs::read(&path).unwrap();
        std::fs::remove_file(path).unwrap();
        assert!(status.success(), "isolated diagnostics child failed");
        assert_eq!(
            contents,
            [b"existing-log-record\n".as_slice(), HTTP, PANIC].concat()
        );
    }

    #[test]
    fn worker_panic_never_manufactures_a_receipt_and_shutdown_signals_owned_control() {
        let state = Arc::new(Mutex::new(State::default()));
        let (summary, work) = state
            .lock()
            .unwrap()
            .submit(crate::input::Normalized {
                submission_id: "failure-test".into(),
                request: workbench_core::request::Request::new("task.run", json!({})),
                root_id: None,
                label: "Error budget",
                identity: [0; 32],
            })
            .unwrap_or_else(|_| panic!("admission"));
        let work = work.unwrap();
        let signal = Arc::clone(&work.control.signal);
        let (sender, receiver) = mpsc::sync_channel(1);
        let worker_state = Arc::clone(&state);
        let worker = std::thread::spawn(move || {
            run_worker(receiver, worker_state, |_, _| {
                panic!("synthetic worker failure")
            })
        });
        sender.send(work).unwrap_or_else(|_| panic!("worker send"));
        assert!(worker.join().is_err());
        let mut state = state.lock().unwrap();
        assert_eq!(state.receipt(&summary.id).err().unwrap().status, 409);
        state.shutdown(15);
        assert_eq!(signal.load(Ordering::Relaxed), 15);
        assert!(state.stopped);
    }
}
