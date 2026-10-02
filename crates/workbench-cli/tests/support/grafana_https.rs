use rcgen::{
    BasicConstraints, CertificateParams, CertifiedIssuer, ExtendedKeyUsagePurpose, IsCa, KeyPair,
    KeyUsagePurpose,
};
use rustls::{ServerConfig, ServerConnection, StreamOwned, pki_types::PrivatePkcs8KeyDer};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::PathBuf,
    process::{Command, Output},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

pub const SAVE: &str = env!("CARGO_BIN_EXE_save");
pub const TOKEN: &str = "synthetic-private-token";

pub struct Reply {
    pub status: u16,
    pub body: Vec<u8>,
    pub headers: Vec<(String, String)>,
    pub header_delay: Duration,
    pub body_delay: Duration,
    pub chunked: bool,
}
impl Reply {
    pub fn json(value: Value) -> Self {
        Self::raw(value.to_string().into_bytes())
    }
    pub fn raw(body: Vec<u8>) -> Self {
        Self {
            status: 200,
            body,
            headers: vec![],
            header_delay: Duration::ZERO,
            body_delay: Duration::ZERO,
            chunked: false,
        }
    }
    pub fn org() -> Self {
        Self::json(json!({"id":7}))
    }
    pub fn dashboard() -> Self {
        Self::json(json!({"dashboard":{"uid":"board-1","title":"Fixture"},"meta":{}}))
    }
    pub fn datasource(kind: &str) -> Self {
        Self::json(json!({"uid":"metrics-1","type":kind,"orgId":7}))
    }
    pub fn query() -> Self {
        Self::json(json!({"results":{"A":{"frames":[]}}}))
    }
}

#[derive(Clone, Debug)]
pub struct Received {
    pub method: String,
    pub path: String,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}

pub struct Fixture {
    pub directory: PathBuf,
    pub config: PathBuf,
    pub ca: PathBuf,
    pub provider: Value,
    requests: Arc<Mutex<Vec<Received>>>,
    stopped: Arc<AtomicBool>,
    server: Option<JoinHandle<()>>,
}
impl Fixture {
    pub fn new(replies: Vec<Reply>) -> Self {
        Self::with_hostname(replies, "127.0.0.1")
    }
    pub fn with_hostname(replies: Vec<Reply>, hostname: &str) -> Self {
        static COUNT: AtomicU64 = AtomicU64::new(0);
        let directory = std::env::temp_dir().join(format!(
            "grafana-https-{}-{}",
            std::process::id(),
            COUNT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&directory).unwrap();
        let mut ca_params = CertificateParams::new(Vec::<String>::new()).unwrap();
        ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        ca_params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
        let ca_cert =
            CertifiedIssuer::self_signed(ca_params, KeyPair::generate().unwrap()).unwrap();
        let mut leaf_params = CertificateParams::new(vec![hostname.to_owned()]).unwrap();
        leaf_params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        let leaf_key = KeyPair::generate().unwrap();
        let leaf = leaf_params.signed_by(&leaf_key, &ca_cert).unwrap();
        let tls = ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::aws_lc_rs::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(
            vec![leaf.der().clone()],
            PrivatePkcs8KeyDer::from(leaf_key.serialize_der()).into(),
        )
        .unwrap();
        let ca = directory.join("ca.pem");
        fs::write(&ca, ca_cert.pem()).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let origin = format!("https://{}/grafana", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stopped = Arc::new(AtomicBool::new(false));
        let server = {
            let requests = Arc::clone(&requests);
            let stopped = Arc::clone(&stopped);
            thread::spawn(move || {
                let tls = Arc::new(tls);
                let mut replies = replies.into_iter();
                while !stopped.load(Ordering::Relaxed) {
                    match listener.accept() {
                        Ok((tcp, _)) => {
                            tcp.set_read_timeout(Some(Duration::from_millis(500)))
                                .unwrap();
                            tcp.set_write_timeout(Some(Duration::from_millis(500)))
                                .unwrap();
                            let mut stream = StreamOwned::new(
                                ServerConnection::new(Arc::clone(&tls)).unwrap(),
                                tcp,
                            );
                            if let Some(request) = read_request(&mut stream) {
                                requests.lock().unwrap().push(request);
                                let reply = replies.next().unwrap_or_else(|| {
                                    let mut r = Reply::raw(vec![]);
                                    r.status = 500;
                                    r
                                });
                                let _ = write_reply(&mut stream, reply, &stopped);
                            }
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(2))
                        }
                        Err(error) => panic!("fixture accept: {error}"),
                    }
                }
            })
        };
        let config = directory.join("operator.json");
        fs::write(&config, json!({"spec_version":"0.1","record":"never","connections":{"fixture":{"origin":origin,"expected_org_id":7,"api_profile":"grafana-legacy-v1","credential_ref":"env:SAVE_GRAFANA_FIXTURE","tls_ca_file":ca}}}).to_string()).unwrap();
        let provider = json!({"origin":origin,"expected_org_id":7,"operations":["grafana.dashboard.get","grafana.query"],"auth":{"kind":"bearer","token":TOKEN}});
        Self {
            directory,
            config,
            ca,
            provider,
            requests,
            stopped,
            server: Some(server),
        }
    }
    pub fn command(&self) -> Command {
        let mut command = Command::new(SAVE);
        command
            .env_clear()
            .env("SAVE_GRAFANA_FIXTURE", self.provider.to_string())
            .args(["--json", "--config"])
            .arg(&self.config);
        command
    }
    pub fn dashboard(&self) -> Command {
        let mut command = self.command();
        command.args([
            "grafana",
            "dashboard",
            "get",
            "--target",
            "fixture",
            "--uid",
            "board-1",
        ]);
        command
    }
    pub fn query(&self, kind: &str) -> Command {
        let mut command = self.command();
        command.args([
            "grafana",
            "query",
            "--target",
            "fixture",
            "--datasource",
            "metrics-1",
            "--kind",
            kind,
            "--from",
            "2026-10-02T00:00:00Z",
            "--to",
            "2026-10-02T00:01:00Z",
            "--expr",
            "up{job=~\"café$\"}",
        ]);
        command
    }
    pub fn run(&self, command: &mut Command) -> (Output, Value) {
        let output = command.output().unwrap();
        let value = receipt(&output);
        (output, value)
    }
    pub fn requests(&self) -> Vec<Received> {
        self.requests.lock().unwrap().clone()
    }
    pub fn wait_for_requests(&self, minimum: usize) {
        let until = Instant::now() + Duration::from_secs(5);
        while self.requests().len() < minimum {
            assert!(Instant::now() < until, "fixture did not receive request");
            thread::sleep(Duration::from_millis(5));
        }
    }
    pub fn edit_config(&self, mutate: impl FnOnce(&mut Value)) {
        let mut value: Value = serde_json::from_slice(&fs::read(&self.config).unwrap()).unwrap();
        mutate(&mut value);
        fs::write(&self.config, value.to_string()).unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Relaxed);
        self.server.take().unwrap().join().unwrap();
        let _ = fs::remove_dir_all(&self.directory);
    }
}
pub fn receipt(output: &Output) -> Value {
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap_or_else(|e| {
        panic!(
            "bad receipt: {e}: {}",
            String::from_utf8_lossy(&output.stdout)
        )
    })
}
pub fn has_error(value: &Value, code: &str) -> bool {
    value["errors"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v["code"] == code)
}

fn read_request(stream: &mut StreamOwned<ServerConnection, TcpStream>) -> Option<Received> {
    let mut bytes = Vec::new();
    let header_end;
    loop {
        if bytes.len() > 128 * 1024 {
            return None;
        }
        if let Some(at) = bytes.windows(4).position(|s| s == b"\r\n\r\n") {
            header_end = at + 4;
            break;
        }
        let mut buffer = [0; 4096];
        let count = stream.read(&mut buffer).ok()?;
        if count == 0 {
            return None;
        }
        bytes.extend_from_slice(&buffer[..count]);
    }
    let head = std::str::from_utf8(&bytes[..header_end]).ok()?;
    let mut lines = head.split("\r\n");
    let mut first = lines.next()?.split(' ');
    let method = first.next()?.to_owned();
    let path = first.next()?.to_owned();
    let headers = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.to_ascii_lowercase(), v.trim().to_owned()))
        .collect::<BTreeMap<_, _>>();
    let length = headers
        .get("content-length")
        .map(|s| s.parse::<usize>().ok())
        .unwrap_or(Some(0))?;
    if length > 128 * 1024 {
        return None;
    }
    while bytes.len() < header_end + length {
        let mut buffer = [0; 4096];
        let count = stream.read(&mut buffer).ok()?;
        if count == 0 {
            return None;
        }
        bytes.extend_from_slice(&buffer[..count]);
    }
    Some(Received {
        method,
        path,
        headers,
        body: bytes[header_end..header_end + length].to_vec(),
    })
}
fn pause(duration: Duration, stopped: &AtomicBool) -> std::io::Result<()> {
    let until = Instant::now() + duration;
    while Instant::now() < until {
        if stopped.load(Ordering::Relaxed) {
            return Err(std::io::ErrorKind::Interrupted.into());
        }
        thread::sleep(Duration::from_millis(2));
    }
    Ok(())
}
fn write_reply(
    stream: &mut StreamOwned<ServerConnection, TcpStream>,
    reply: Reply,
    stopped: &AtomicBool,
) -> std::io::Result<()> {
    pause(reply.header_delay, stopped)?;
    write!(
        stream,
        "HTTP/1.1 {} Fixture\r\nContent-Type: application/json\r\nConnection: close\r\n",
        reply.status
    )?;
    for (key, value) in &reply.headers {
        write!(stream, "{key}: {value}\r\n")?;
    }
    if reply.chunked {
        write!(stream, "Transfer-Encoding: chunked\r\n\r\n")?;
    } else {
        write!(stream, "Content-Length: {}\r\n\r\n", reply.body.len())?;
    }
    stream.flush()?;
    let at = reply.body.len() / 2;
    for (index, bytes) in [&reply.body[..at], &reply.body[at..]]
        .into_iter()
        .enumerate()
    {
        if index == 1 {
            pause(reply.body_delay, stopped)?;
        }
        if reply.chunked {
            write!(stream, "{:x}\r\n", bytes.len())?;
        }
        stream.write_all(bytes)?;
        if reply.chunked {
            write!(stream, "\r\n")?;
        }
        stream.flush()?;
    }
    if reply.chunked {
        write!(stream, "0\r\n\r\n")?;
    }
    stream.flush()
}
