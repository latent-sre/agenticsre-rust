//! Version-1 JSONL dialogue. No child or filesystem handles belong to this state machine.
use super::*;
use crate::process::Dialogue;
const FRAME: usize = 64 * 1024;
const TOTAL: usize = 256 * 1024;
pub(super) struct Protocol {
    request_id: String,
    run_id: String,
    outbound: Vec<u8>,
    written: usize,
    request: Vec<u8>,
    hello_written: bool,
    request_sent: bool,
    acknowledged: bool,
    buffer: Vec<u8>,
    bytes: usize,
    sequence: u64,
    frames: u64,
    error: Option<&'static str>,
    eof: bool,
    output_schema: Value,
    pub progress: Vec<Value>,
    pub terminal: Option<Value>,
}
impl Protocol {
    pub fn new(
        request: &Request,
        result: &OperationResult,
        output_schema: Value,
        remaining: u64,
    ) -> Result<Self, Fault> {
        let hello = json!({"protocol":1,"request_id":result.request_id,"run_id":result.run_id,"sequence":0,"kind":"hello"});
        let work = json!({"protocol":1,"request_id":result.request_id,"run_id":result.run_id,"sequence":1,"kind":"request",
            "operation":request.operation,"operation_version":request.operation_version,"inputs":request.inputs,
            "limits":{"timeout_ms":remaining,"max_output_bytes":result.effective_limits.max_output_bytes}});
        fn frame(value: Value) -> Result<Vec<u8>, Fault> {
            let mut encoded = serde_json::to_vec(&value).expect("frame");
            encoded.push(b'\n');
            if encoded.len() > FRAME {
                return Err(Fault::new("extension_request_too_large"));
            }
            Ok(encoded)
        }
        Ok(Self {
            request_id: result.request_id.clone(),
            run_id: result.run_id.clone(),
            outbound: frame(hello)?,
            written: 0,
            request: frame(work)?,
            hello_written: false,
            request_sent: false,
            acknowledged: false,
            buffer: vec![],
            bytes: 0,
            sequence: 0,
            frames: 0,
            error: None,
            eof: false,
            output_schema,
            progress: vec![],
            terminal: None,
        })
    }
    fn fail(&mut self, code: &'static str) {
        if self.error.is_none() {
            self.error = Some(code);
        }
    }
    fn frame(&mut self, bytes: &[u8]) {
        if self.frames == 64 {
            self.fail("frame_count_exceeded");
            return;
        }
        self.frames += 1;
        let Ok(value) = crate::request::parse_bounded_json(bytes, 8) else {
            self.fail("invalid_json_frame");
            return;
        };
        if !value.is_object()
            || value["protocol"] != 1
            || value["request_id"] != self.request_id
            || value["run_id"] != self.run_id
            || value["sequence"].as_u64() != Some(self.sequence)
        {
            self.fail("invalid_frame_identity");
            return;
        }
        let base = ["protocol", "request_id", "run_id", "sequence", "kind"];
        if !self.acknowledged {
            if !self.hello_written || value["kind"] != "hello" || !keys(&value, &base, &[]) {
                self.fail("invalid_hello");
                return;
            }
            self.acknowledged = true;
            self.outbound = std::mem::take(&mut self.request);
            self.written = 0;
            self.sequence += 1;
            return;
        }
        if !self.request_sent || self.terminal.is_some() {
            self.fail("unexpected_work_frame");
            return;
        }
        match value["kind"].as_str() {
            Some("progress")
                if keys(&value, &base, &["message"])
                    && value["message"]
                        .as_str()
                        .is_some_and(|s| s.chars().count() <= 1024) =>
            {
                self.progress.push(
                    json!({"sequence":self.sequence,"message":value["message"],"untrusted":true}),
                );
            }
            Some("result") => {
                let fields = [
                    "protocol",
                    "request_id",
                    "run_id",
                    "sequence",
                    "kind",
                    "status",
                    "data",
                    "error",
                ];
                let exact = value.as_object().is_some_and(|m| {
                    m.len() == fields.len() && fields.iter().all(|k| m.contains_key(*k))
                });
                let valid = match value["status"].as_str() {
                    Some("succeeded") => {
                        value["error"].is_null()
                            && package::validate_values(&self.output_schema, &value["data"])
                    }
                    Some("failed") => {
                        value["data"].is_null()
                            && keys(&value["error"], &["code", "message"], &[])
                            && value["error"]["code"].as_str().is_some_and(|s| {
                                !s.is_empty()
                                    && s.chars().count() <= 64
                                    && !s.chars().any(char::is_control)
                            })
                            && value["error"]["message"]
                                .as_str()
                                .is_some_and(|s| !s.is_empty() && s.chars().count() <= 1024)
                    }
                    _ => false,
                };
                if !exact || !valid {
                    self.fail("invalid_terminal");
                    return;
                }
                self.terminal = Some(
                    json!({"sequence":self.sequence,"status":value["status"],"data":value["data"],"error":value["error"],"untrusted":true}),
                );
            }
            _ => {
                self.fail("invalid_work_frame");
                return;
            }
        }
        self.sequence += 1;
    }
    pub fn metadata(&self) -> Value {
        json!({"name":"save-extension","version":1,"state":if self.error.is_some(){"invalid"}else if self.terminal.is_some(){"terminal"}else if self.request_sent{"request_sent"}else{"awaiting_hello"},"frames_received":self.frames,"terminal_received":self.terminal.is_some(),"error":self.error})
    }
    pub fn complete(&self) -> bool {
        self.eof && self.error.is_none() && self.terminal.is_some()
    }
}
impl Dialogue for Protocol {
    fn pending_write(&self) -> &[u8] {
        if self.error.is_some() {
            &[]
        } else {
            &self.outbound[self.written..]
        }
    }
    fn written(&mut self, count: usize) {
        self.written += count;
        if self.written == self.outbound.len() {
            if self.acknowledged {
                self.request_sent = true;
            } else {
                self.hello_written = true;
            }
        }
    }
    fn receive(&mut self, bytes: &[u8]) {
        if self.error.is_some() {
            return;
        }
        self.bytes = self.bytes.saturating_add(bytes.len());
        if self.bytes > TOTAL {
            self.fail("stdout_limit_exceeded");
            return;
        }
        for &byte in bytes {
            self.buffer.push(byte);
            if self.buffer.len() > FRAME {
                self.fail("frame_limit_exceeded");
                return;
            }
            if byte == b'\n' {
                let frame = std::mem::take(&mut self.buffer);
                if frame.contains(&b'\r') {
                    self.fail("invalid_line_ending");
                    return;
                }
                self.frame(&frame[..frame.len() - 1]);
                if self.error.is_some() {
                    return;
                }
            }
        }
    }
    fn stdout_closed(&mut self) {
        self.eof = true;
        if !self.buffer.is_empty() {
            self.fail("unterminated_frame");
        }
        if self.terminal.is_none() {
            self.fail("missing_terminal");
        }
    }
    fn input_closed(&mut self) {
        self.fail("peer_input_closed");
    }
    fn input_finished(&self) -> bool {
        self.request_sent || self.error.is_some()
    }
    fn error(&self) -> Option<&'static str> {
        self.error
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn protocol() -> Protocol {
        let request = Request::new("ext.fixture.run", json!({}));
        let result = OperationResult::new(&request);
        Protocol::new(
            &request,
            &result,
            json!({"properties":{"value":{"type":"integer","minimum":0,"maximum":10}}}),
            1000,
        )
        .unwrap()
    }
    fn peer(p: &Protocol, sequence: u64, kind: &str, extra: Value) -> Vec<u8> {
        let mut value = json!({"protocol":1,"request_id":p.request_id,"run_id":p.run_id,"sequence":sequence,"kind":kind});
        value
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        let mut bytes = serde_json::to_vec(&value).unwrap();
        bytes.push(b'\n');
        bytes
    }
    fn ready(p: &mut Protocol) {
        let hello = p.pending_write().len();
        p.written(hello);
        p.receive(&peer(p, 0, "hello", json!({})));
        assert!(p.error().is_none());
        let request = p.pending_write().len();
        p.written(request);
        assert!(p.input_finished());
    }
    #[test]
    fn fragmented_handshake_success_and_eof_validate_exact_schema() {
        let mut p = protocol();
        let hello = p.pending_write().len();
        p.written(hello - 1);
        assert!(!p.hello_written);
        p.written(1);
        let ack = peer(&p, 0, "hello", json!({}));
        for byte in ack {
            p.receive(&[byte]);
        }
        let work = p.pending_write().len();
        p.written(work - 1);
        assert!(!p.request_sent);
        p.written(1);
        p.receive(&peer(
            &p,
            1,
            "progress",
            json!({"message":"synthetic observation"}),
        ));
        p.receive(&peer(
            &p,
            2,
            "result",
            json!({"status":"succeeded","data":{"value":5},"error":null}),
        ));
        assert!(!p.complete());
        p.stdout_closed();
        assert!(p.complete());
        assert_eq!(p.progress.len(), 1);
    }
    #[test]
    fn work_before_request_write_and_unsolicited_hello_fail() {
        let mut p = protocol();
        p.receive(&peer(&p, 0, "hello", json!({})));
        assert_eq!(p.error(), Some("invalid_hello"));
        let mut p = protocol();
        let hello = p.pending_write().len();
        p.written(hello);
        let mut frames = peer(&p, 0, "hello", json!({}));
        frames.extend(peer(
            &p,
            1,
            "result",
            json!({"status":"succeeded","data":{"value":1},"error":null}),
        ));
        p.receive(&frames);
        assert_eq!(p.error(), Some("unexpected_work_frame"));
    }
    #[test]
    fn duplicate_terminal_wrong_identity_unknown_fields_and_output_schema_fail() {
        for (extra, expected) in [
            (
                json!({"status":"succeeded","data":{"value":11},"error":null}),
                "invalid_terminal",
            ),
            (
                json!({"status":"succeeded","data":{"value":1,"extra":1},"error":null}),
                "invalid_terminal",
            ),
            (
                json!({"status":"failed","data":{},"error":{"code":"failed","message":"reason"}}),
                "invalid_terminal",
            ),
            (
                json!({"status":"succeeded","data":{"value":1},"error":null,"extra":true}),
                "invalid_terminal",
            ),
        ] {
            let mut p = protocol();
            ready(&mut p);
            p.receive(&peer(&p, 1, "result", extra));
            assert_eq!(p.error(), Some(expected));
        }
        let mut p = protocol();
        ready(&mut p);
        p.receive(&peer(
            &p,
            2,
            "progress",
            json!({"message":"wrong sequence"}),
        ));
        assert_eq!(p.error(), Some("invalid_frame_identity"));
        let mut p = protocol();
        ready(&mut p);
        let mut bytes = peer(
            &p,
            1,
            "result",
            json!({"status":"succeeded","data":{"value":1},"error":null}),
        );
        bytes.extend(peer(
            &p,
            2,
            "result",
            json!({"status":"succeeded","data":{"value":1},"error":null}),
        ));
        p.receive(&bytes);
        assert_eq!(p.error(), Some("unexpected_work_frame"));
    }
    #[test]
    fn failed_terminal_is_valid_but_trailing_incomplete_invalid_utf8_and_duplicate_keys_fail() {
        let mut p = protocol();
        ready(&mut p);
        p.receive(&peer(
            &p,
            1,
            "result",
            json!({"status":"failed","data":null,"error":{"code":"rejected","message":"reason"}}),
        ));
        p.stdout_closed();
        assert!(p.complete());
        let mut p = protocol();
        ready(&mut p);
        p.receive(b"{\"a\":1,\"a\":2}\n");
        assert_eq!(p.error(), Some("invalid_json_frame"));
        let mut p = protocol();
        ready(&mut p);
        p.receive(b"\xff\n");
        assert_eq!(p.error(), Some("invalid_json_frame"));
        let mut p = protocol();
        ready(&mut p);
        let mut frame = peer(
            &p,
            1,
            "result",
            json!({"status":"succeeded","data":{"value":1},"error":null}),
        );
        frame.pop();
        p.receive(&frame);
        p.stdout_closed();
        assert_eq!(p.error(), Some("unterminated_frame"));
    }
    #[test]
    fn frame_count_and_total_byte_limits_stop_unbounded_peers() {
        let mut p = protocol();
        ready(&mut p);
        for sequence in 1..=64 {
            p.receive(&peer(&p, sequence, "progress", json!({"message":"x"})));
        }
        assert_eq!(p.error(), Some("frame_count_exceeded"));
        assert_eq!(p.frames, 64);
        let mut p = protocol();
        ready(&mut p);
        p.receive(&vec![b' '; FRAME + 1]);
        assert_eq!(p.error(), Some("frame_limit_exceeded"));
        let mut p = protocol();
        ready(&mut p);
        p.receive(&vec![b' '; TOTAL + 1]);
        assert_eq!(p.error(), Some("stdout_limit_exceeded"));
    }
}
