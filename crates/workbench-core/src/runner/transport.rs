//! One bounded RPC per mutually authenticated TLS connection.
use rustls::{
    ClientConfig, ClientConnection, RootCertStore, ServerConfig, ServerConnection, StreamOwned,
    pki_types::{CertificateDer, PrivatePkcs8KeyDer, ServerName},
};
use sha2::{Digest, Sha256};
use std::{
    io::{self, Read, Write},
    net::{SocketAddr, TcpStream},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

const ALPN: &[u8] = b"save-runner/1";
const MAX_REQUEST: usize = 64 * 1024;
const MAX_RESPONSE: usize = 2 * 1024 * 1024;

pub(super) struct Material {
    pub ca: Vec<u8>,
    pub certificate: Vec<u8>,
    pub private_key: Vec<u8>,
}

#[derive(Debug)]
struct NoTickets;
impl rustls::server::ProducesTickets for NoTickets {
    fn enabled(&self) -> bool {
        false
    }
    fn lifetime(&self) -> u32 {
        0
    }
    fn encrypt(&self, _plain: &[u8]) -> Option<Vec<u8>> {
        None
    }
    fn decrypt(&self, _cipher: &[u8]) -> Option<Vec<u8>> {
        None
    }
}

fn roots(ca: Vec<u8>) -> Result<RootCertStore, &'static str> {
    let mut roots = RootCertStore::empty();
    roots
        .add(CertificateDer::from(ca))
        .map_err(|_| "runner_tls_material_invalid")?;
    Ok(roots)
}

pub(super) fn client_config(material: Material) -> Result<Arc<ClientConfig>, &'static str> {
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let mut config = ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(|_| "runner_tls_material_invalid")?
        .with_root_certificates(roots(material.ca)?)
        .with_client_auth_cert(
            vec![CertificateDer::from(material.certificate)],
            PrivatePkcs8KeyDer::from(material.private_key).into(),
        )
        .map_err(|_| "runner_tls_material_invalid")?;
    config.alpn_protocols = vec![ALPN.to_vec()];
    // Every RPC authenticates its certificate afresh. Early data is replayable;
    // it cannot carry operation admission or cancellation authority.
    config.resumption = rustls::client::Resumption::disabled();
    config.enable_early_data = false;
    Ok(Arc::new(config))
}

pub(super) fn server_config(material: Material) -> Result<Arc<ServerConfig>, &'static str> {
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let verifier = rustls::server::WebPkiClientVerifier::builder_with_provider(
        Arc::new(roots(material.ca)?),
        Arc::clone(&provider),
    )
    .build()
    .map_err(|_| "runner_tls_material_invalid")?;
    let mut config = ServerConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(|_| "runner_tls_material_invalid")?
        .with_client_cert_verifier(verifier)
        .with_single_cert(
            vec![CertificateDer::from(material.certificate)],
            PrivatePkcs8KeyDer::from(material.private_key).into(),
        )
        .map_err(|_| "runner_tls_material_invalid")?;
    config.alpn_protocols = vec![ALPN.to_vec()];
    config.session_storage = Arc::new(rustls::server::NoServerSessionStorage {});
    config.ticketer = Arc::new(NoTickets);
    config.send_tls13_tickets = 0;
    config.max_early_data_size = 0;
    config.send_half_rtt_data = false;
    Ok(Arc::new(config))
}

fn remaining(deadline: Instant) -> io::Result<Duration> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|duration| !duration.is_zero())
        .ok_or_else(|| io::Error::from(io::ErrorKind::TimedOut))
}
fn io_code(error: io::Error, deadline: Instant, otherwise: &'static str) -> &'static str {
    if error.to_string() == "runner_cancelled" {
        return "runner_cancelled";
    }
    if Instant::now() >= deadline
        || matches!(
            error.kind(),
            io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
        )
    {
        "runner_deadline_exceeded"
    } else {
        otherwise
    }
}

/// Rustls may perform several reads inside complete_io. Recompute the same
/// absolute deadline before every underlying syscall, including handshake I/O.
struct DeadlineSocket {
    tcp: TcpStream,
    deadline: Instant,
    signal: Option<Arc<AtomicUsize>>,
}
impl DeadlineSocket {
    fn checked_timeout(&self) -> io::Result<Duration> {
        if self
            .signal
            .as_ref()
            .is_some_and(|s| s.load(Ordering::Relaxed) != 0)
        {
            return Err(io::Error::other("runner_cancelled"));
        }
        let left = remaining(self.deadline)?;
        Ok(if self.signal.is_some() {
            left.min(Duration::from_millis(100))
        } else {
            left
        })
    }
}
impl Read for DeadlineSocket {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        loop {
            self.tcp.set_read_timeout(Some(self.checked_timeout()?))?;
            match self.tcp.read(buffer) {
                Err(e)
                    if self.signal.is_some()
                        && matches!(
                            e.kind(),
                            io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                        ) =>
                {
                    continue;
                }
                other => {
                    self.checked_timeout()?;
                    return other;
                }
            }
        }
    }
}
impl Write for DeadlineSocket {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        loop {
            self.tcp.set_write_timeout(Some(self.checked_timeout()?))?;
            match self.tcp.write(buffer) {
                Err(e)
                    if self.signal.is_some()
                        && matches!(
                            e.kind(),
                            io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                        ) =>
                {
                    continue;
                }
                other => {
                    self.checked_timeout()?;
                    return other;
                }
            }
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        self.checked_timeout()?;
        self.tcp.flush()
    }
}
fn connect(
    endpoint: SocketAddr,
    deadline: Instant,
    signal: Option<&Arc<AtomicUsize>>,
) -> io::Result<TcpStream> {
    use std::os::fd::{AsRawFd, FromRawFd};
    let preflight = || -> io::Result<()> {
        if signal.is_some_and(|value| value.load(Ordering::Relaxed) != 0) {
            return Err(io::Error::other("runner_cancelled"));
        }
        remaining(deadline).map(|_| ())
    };
    preflight()?;
    // SAFETY: socket takes scalar parameters and returns an exclusively owned descriptor.
    let fd = unsafe {
        libc::socket(
            if endpoint.is_ipv4() {
                libc::AF_INET
            } else {
                libc::AF_INET6
            },
            libc::SOCK_STREAM | libc::SOCK_CLOEXEC | libc::SOCK_NONBLOCK,
            0,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: fd is newly owned and valid; TcpStream closes it on every return path.
    let tcp = unsafe { TcpStream::from_raw_fd(fd) };
    preflight()?;
    let status = match endpoint {
        SocketAddr::V4(a) => {
            let address = libc::sockaddr_in {
                sin_family: libc::AF_INET as _,
                sin_port: a.port().to_be(),
                sin_addr: libc::in_addr {
                    s_addr: u32::from_ne_bytes(a.ip().octets()),
                },
                sin_zero: [0; 8],
            };
            // SAFETY: address is initialized and valid for the exact supplied length during connect.
            unsafe {
                libc::connect(
                    fd,
                    std::ptr::from_ref(&address).cast(),
                    std::mem::size_of_val(&address) as _,
                )
            }
        }
        SocketAddr::V6(a) => {
            let address = libc::sockaddr_in6 {
                sin6_family: libc::AF_INET6 as _,
                sin6_port: a.port().to_be(),
                sin6_flowinfo: a.flowinfo(),
                sin6_addr: libc::in6_addr {
                    s6_addr: a.ip().octets(),
                },
                sin6_scope_id: a.scope_id(),
            };
            // SAFETY: address is initialized and valid for the exact supplied length during connect.
            unsafe {
                libc::connect(
                    fd,
                    std::ptr::from_ref(&address).cast(),
                    std::mem::size_of_val(&address) as _,
                )
            }
        }
    };
    if status < 0 && io::Error::last_os_error().raw_os_error() != Some(libc::EINPROGRESS) {
        return Err(io::Error::last_os_error());
    }
    loop {
        if signal.is_some_and(|s| s.load(Ordering::Relaxed) != 0) {
            return Err(io::Error::other("runner_cancelled"));
        }
        let timeout = remaining(deadline)?.min(Duration::from_millis(100));
        let mut poll = libc::pollfd {
            fd: tcp.as_raw_fd(),
            events: libc::POLLOUT,
            revents: 0,
        };
        // SAFETY: one initialized pollfd is borrowed exclusively for this bounded syscall.
        let ready = unsafe { libc::poll(&mut poll, 1, timeout.as_millis().max(1) as i32) };
        if ready < 0 {
            let e = io::Error::last_os_error();
            if e.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(e);
        }
        if ready == 0 {
            continue;
        }
        if let Some(error) = tcp.take_error()? {
            return Err(error);
        }
        tcp.set_nonblocking(false)?;
        return Ok(tcp);
    }
}

fn read_exact(
    reader: &mut impl Read,
    mut bytes: &mut [u8],
    deadline: Instant,
) -> Result<(), &'static str> {
    while !bytes.is_empty() {
        remaining(deadline).map_err(|_| "runner_deadline_exceeded")?;
        match reader.read(bytes) {
            Ok(0) => return Err("runner_frame_incomplete"),
            Ok(count) => bytes = &mut bytes[count..],
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(io_code(error, deadline, "runner_transport_io")),
        }
    }
    remaining(deadline).map_err(|_| "runner_deadline_exceeded")?;
    Ok(())
}
fn read_frame(
    reader: &mut impl Read,
    maximum: usize,
    deadline: Instant,
) -> Result<Vec<u8>, &'static str> {
    let mut header = [0; 4];
    read_exact(reader, &mut header, deadline)?;
    let length = u32::from_be_bytes(header) as usize;
    if length == 0 || length > maximum {
        return Err("runner_frame_invalid");
    }
    let mut bytes = vec![0; length];
    read_exact(reader, &mut bytes, deadline)?;
    Ok(bytes)
}
fn write_all(
    writer: &mut impl Write,
    mut bytes: &[u8],
    deadline: Instant,
) -> Result<(), &'static str> {
    while !bytes.is_empty() {
        remaining(deadline).map_err(|_| "runner_deadline_exceeded")?;
        match writer.write(bytes) {
            Ok(0) => return Err("runner_transport_io"),
            Ok(count) => bytes = &bytes[count..],
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(io_code(error, deadline, "runner_transport_io")),
        }
    }
    remaining(deadline).map_err(|_| "runner_deadline_exceeded")?;
    Ok(())
}
fn write_frame(
    writer: &mut impl Write,
    bytes: &[u8],
    maximum: usize,
    deadline: Instant,
) -> Result<(), &'static str> {
    if bytes.is_empty() || bytes.len() > maximum {
        return Err("runner_frame_invalid");
    }
    write_all(writer, &(bytes.len() as u32).to_be_bytes(), deadline)?;
    write_all(writer, bytes, deadline)?;
    writer
        .flush()
        .map_err(|error| io_code(error, deadline, "runner_transport_io"))?;
    remaining(deadline).map_err(|_| "runner_deadline_exceeded")?;
    Ok(())
}
fn leaf_sha256(certificates: Option<&[CertificateDer<'_>]>) -> Result<String, &'static str> {
    let leaf = certificates
        .and_then(|chain| chain.first())
        .ok_or("runner_tls_peer_missing")?;
    let hex: String = Sha256::digest(leaf.as_ref())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    Ok(format!("sha256:{hex}"))
}

pub(super) struct ServerChannel {
    stream: StreamOwned<ServerConnection, DeadlineSocket>,
    peer_sha256: String,
    read_attempted: bool,
    request_complete: bool,
    response_attempted: bool,
}
impl ServerChannel {
    pub fn peer_sha256(&self) -> &str {
        &self.peer_sha256
    }
    pub fn read_request(&mut self) -> Result<Vec<u8>, &'static str> {
        if self.read_attempted {
            return Err("runner_connection_reused");
        }
        self.read_attempted = true;
        let deadline = self.stream.sock.deadline;
        let request = read_frame(&mut self.stream, MAX_REQUEST, deadline)?;
        self.request_complete = true;
        Ok(request)
    }
    pub fn write_response(&mut self, bytes: &[u8]) -> Result<(), &'static str> {
        if !self.request_complete || self.response_attempted {
            return Err("runner_connection_reused");
        }
        self.response_attempted = true;
        let deadline = self.stream.sock.deadline;
        write_frame(&mut self.stream, bytes, MAX_RESPONSE, deadline)
    }
}

pub(super) fn accept(
    tcp: TcpStream,
    config: Arc<ServerConfig>,
    deadline: Instant,
) -> Result<ServerChannel, &'static str> {
    remaining(deadline).map_err(|_| "runner_deadline_exceeded")?;
    tcp.set_nonblocking(false)
        .map_err(|_| "runner_transport_io")?;
    let mut socket = DeadlineSocket {
        tcp,
        deadline,
        signal: None,
    };
    let mut connection =
        ServerConnection::new(config).map_err(|_| "runner_tls_material_invalid")?;
    while connection.is_handshaking() {
        connection
            .complete_io(&mut socket)
            .map_err(|error| io_code(error, deadline, "runner_tls_failed"))?;
    }
    remaining(deadline).map_err(|_| "runner_deadline_exceeded")?;
    if connection.protocol_version() != Some(rustls::ProtocolVersion::TLSv1_3)
        || connection.alpn_protocol() != Some(ALPN)
    {
        return Err("runner_tls_protocol_mismatch");
    }
    let peer_sha256 = leaf_sha256(connection.peer_certificates())?;
    Ok(ServerChannel {
        stream: StreamOwned::new(connection, socket),
        peer_sha256,
        read_attempted: false,
        request_complete: false,
        response_attempted: false,
    })
}

#[derive(Debug)]
pub(super) struct ExchangeError {
    pub code: &'static str,
    pub request_may_have_been_sent: bool,
}

pub(super) fn exchange_controlled(
    endpoint: SocketAddr,
    server_name: &str,
    expected_server_sha256: &str,
    config: Arc<ClientConfig>,
    request: &[u8],
    deadline: Instant,
    signal: Option<Arc<AtomicUsize>>,
) -> Result<Vec<u8>, ExchangeError> {
    let before_request = |code| ExchangeError {
        code,
        request_may_have_been_sent: false,
    };
    if request.is_empty() || request.len() > MAX_REQUEST {
        return Err(before_request("runner_frame_invalid"));
    }
    let name = ServerName::try_from(server_name.to_owned())
        .map_err(|_| before_request("runner_server_name_invalid"))?;
    if !matches!(name, ServerName::DnsName(_)) {
        return Err(before_request("runner_server_name_invalid"));
    }
    let tcp = connect(endpoint, deadline, signal.as_ref())
        .map_err(|error| before_request(io_code(error, deadline, "runner_connect_failed")))?;
    let mut socket = DeadlineSocket {
        tcp,
        deadline,
        signal,
    };
    let mut connection = ClientConnection::new(config, name)
        .map_err(|_| before_request("runner_tls_material_invalid"))?;
    while connection.is_handshaking() {
        connection
            .complete_io(&mut socket)
            .map_err(|error| before_request(io_code(error, deadline, "runner_tls_failed")))?;
    }
    remaining(deadline).map_err(|_| before_request("runner_deadline_exceeded"))?;
    if connection.protocol_version() != Some(rustls::ProtocolVersion::TLSv1_3)
        || connection.alpn_protocol() != Some(ALPN)
    {
        return Err(before_request("runner_tls_protocol_mismatch"));
    }
    if leaf_sha256(connection.peer_certificates()).map_err(before_request)?
        != expected_server_sha256
    {
        return Err(before_request("runner_server_identity_mismatch"));
    }
    // Pin verification precedes even the length prefix. Once this boundary is
    // crossed, any transport loss conservatively leaves delivery uncertain.
    let after_request = |code| ExchangeError {
        code,
        request_may_have_been_sent: true,
    };
    let mut stream = StreamOwned::new(connection, socket);
    write_frame(&mut stream, request, MAX_REQUEST, deadline).map_err(after_request)?;
    read_frame(&mut stream, MAX_RESPONSE, deadline).map_err(after_request)
}

#[cfg(test)]
fn exchange(
    endpoint: SocketAddr,
    server_name: &str,
    pin: &str,
    config: Arc<ClientConfig>,
    request: &[u8],
    deadline: Instant,
) -> Result<Vec<u8>, ExchangeError> {
    exchange_controlled(endpoint, server_name, pin, config, request, deadline, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rcgen::{
        BasicConstraints, CertificateParams, CertifiedIssuer, ExtendedKeyUsagePurpose, IsCa,
        KeyPair, KeyUsagePurpose,
    };
    use std::{io::Cursor, net::TcpListener, thread};

    struct TlsFixture {
        server: Arc<ServerConfig>,
        client: Arc<ClientConfig>,
        server_pin: String,
        client_pin: String,
        ca: Vec<u8>,
    }
    impl TlsFixture {
        fn new() -> Self {
            let mut ca_params = CertificateParams::new(Vec::<String>::new()).unwrap();
            ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
            ca_params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
            let ca = CertifiedIssuer::self_signed(ca_params, KeyPair::generate().unwrap()).unwrap();
            let material = |name: &str, usage| {
                let mut params = CertificateParams::new(vec![name.to_owned()]).unwrap();
                params.extended_key_usages = vec![usage];
                let key = KeyPair::generate().unwrap();
                let certificate = params.signed_by(&key, &ca).unwrap();
                let pin = leaf_sha256(Some(std::slice::from_ref(certificate.der()))).unwrap();
                (
                    Material {
                        ca: ca.der().to_vec(),
                        certificate: certificate.der().to_vec(),
                        private_key: key.serialize_der(),
                    },
                    pin,
                )
            };
            let (server, server_pin) = material("runner.test", ExtendedKeyUsagePurpose::ServerAuth);
            let (client, client_pin) =
                material("operator.test", ExtendedKeyUsagePurpose::ClientAuth);
            Self {
                server: server_config(server).unwrap(),
                client: client_config(client).unwrap(),
                server_pin,
                client_pin,
                ca: ca.der().to_vec(),
            }
        }
    }
    fn deadline() -> Instant {
        Instant::now() + Duration::from_secs(5)
    }

    #[test]
    fn tls_roundtrip_authenticates_both_leaf_identities_and_allows_one_exchange() {
        let fixture = TlsFixture::new();
        assert!(!fixture.server.ticketer.enabled());
        assert!(!fixture.server.session_storage.can_cache());
        assert_eq!(fixture.server.send_tls13_tickets, 0);
        assert_eq!(fixture.server.max_early_data_size, 0);
        assert!(!fixture.server.send_half_rtt_data);
        assert!(!fixture.client.enable_early_data);
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (tcp, _) = listener.accept().unwrap();
            let mut channel = accept(tcp, fixture.server, deadline()).unwrap();
            assert_eq!(channel.peer_sha256(), fixture.client_pin);
            assert_eq!(
                channel.write_response(b"premature"),
                Err("runner_connection_reused")
            );
            assert_eq!(
                channel.read_request().unwrap(),
                b"{\"operation\":\"status\"}"
            );
            assert_eq!(
                channel.read_request().unwrap_err(),
                "runner_connection_reused"
            );
            channel.write_response(b"{\"accepted\":true}").unwrap();
            assert_eq!(
                channel.write_response(b"second"),
                Err("runner_connection_reused")
            );
        });
        let response = exchange(
            endpoint,
            "runner.test",
            &fixture.server_pin,
            fixture.client,
            b"{\"operation\":\"status\"}",
            deadline(),
        )
        .unwrap();
        assert_eq!(response, b"{\"accepted\":true}");
        server.join().unwrap();
    }

    #[test]
    fn wrong_server_pin_sends_no_application_frame_after_valid_mtls() {
        let fixture = TlsFixture::new();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (tcp, _) = listener.accept().unwrap();
            let mut channel = accept(tcp, fixture.server, deadline()).unwrap();
            assert_eq!(channel.peer_sha256(), fixture.client_pin);
            // Even a partial length prefix would remain buffered as plaintext.
            // The failed pin closes without writing any application bytes.
            let mut byte = [0];
            let read = channel.stream.read(&mut byte);
            assert!(matches!(read, Ok(0)) || read.is_err());
        });
        let failure = exchange(
            endpoint,
            "runner.test",
            &format!("sha256:{}", "0".repeat(64)),
            fixture.client,
            b"must not reach server",
            deadline(),
        )
        .unwrap_err();
        assert_eq!(failure.code, "runner_server_identity_mismatch");
        assert!(!failure.request_may_have_been_sent);
        server.join().unwrap();
    }

    #[test]
    fn certificate_name_and_alpn_mismatch_refuse_before_rpc() {
        for bad_name in [true, false] {
            let fixture = TlsFixture::new();
            let mut client = (*fixture.client).clone();
            if !bad_name {
                client.alpn_protocols = vec![b"unrelated-protocol".to_vec()];
            }
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let endpoint = listener.local_addr().unwrap();
            let server = thread::spawn(move || {
                let (tcp, _) = listener.accept().unwrap();
                assert!(accept(tcp, fixture.server, deadline()).is_err());
            });
            let failure = exchange(
                endpoint,
                if bad_name {
                    "wrong.test"
                } else {
                    "runner.test"
                },
                &fixture.server_pin,
                Arc::new(client),
                b"no dispatch",
                deadline(),
            )
            .unwrap_err();
            assert_eq!(failure.code, "runner_tls_failed");
            assert!(!failure.request_may_have_been_sent);
            server.join().unwrap();
        }
    }

    #[test]
    fn client_certificate_is_mandatory_before_server_exposes_channel() {
        let fixture = TlsFixture::new();
        let mut anonymous = ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::aws_lc_rs::default_provider(),
        ))
        .with_protocol_versions(&[&rustls::version::TLS13])
        .unwrap()
        .with_root_certificates(roots(fixture.ca).unwrap())
        .with_no_client_auth();
        anonymous.alpn_protocols = vec![ALPN.to_vec()];
        anonymous.resumption = rustls::client::Resumption::disabled();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (tcp, _) = listener.accept().unwrap();
            assert!(matches!(
                accept(tcp, fixture.server, deadline()),
                Err("runner_tls_failed")
            ));
        });
        assert!(
            exchange(
                endpoint,
                "runner.test",
                &fixture.server_pin,
                Arc::new(anonymous),
                b"no authorized caller",
                deadline()
            )
            .is_err()
        );
        server.join().unwrap();
    }

    #[test]
    fn framing_bounds_declared_sizes_and_truncated_payloads_before_allocation() {
        for (length, maximum) in [
            (0u32, MAX_REQUEST),
            (MAX_REQUEST as u32 + 1, MAX_REQUEST),
            (MAX_RESPONSE as u32 + 1, MAX_RESPONSE),
        ] {
            assert_eq!(
                read_frame(&mut Cursor::new(length.to_be_bytes()), maximum, deadline()),
                Err("runner_frame_invalid")
            );
        }
        assert_eq!(
            read_frame(&mut Cursor::new(vec![0, 0, 0]), MAX_REQUEST, deadline()),
            Err("runner_frame_incomplete")
        );
        assert_eq!(
            read_frame(
                &mut Cursor::new(vec![0, 0, 0, 2, b'x']),
                MAX_REQUEST,
                deadline()
            ),
            Err("runner_frame_incomplete")
        );
        for maximum in [MAX_REQUEST, MAX_RESPONSE] {
            let payload = vec![b'x'; maximum];
            let mut wire = Vec::new();
            write_frame(&mut wire, &payload, maximum, deadline()).unwrap();
            assert_eq!(wire.len(), maximum + 4);
            assert_eq!(
                read_frame(&mut Cursor::new(wire), maximum, deadline()).unwrap(),
                payload
            );
        }
        let mut wire = Vec::new();
        assert_eq!(
            write_frame(
                &mut wire,
                &vec![0; MAX_REQUEST + 1],
                MAX_REQUEST,
                deadline()
            ),
            Err("runner_frame_invalid")
        );
        assert!(wire.is_empty());
    }

    #[test]
    fn slow_dribble_cannot_restart_absolute_frame_deadline() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = listener.local_addr().unwrap();
        let sender = thread::spawn(move || {
            let (mut tcp, _) = listener.accept().unwrap();
            tcp.write_all(&20u32.to_be_bytes()).unwrap();
            for _ in 0..20 {
                thread::sleep(Duration::from_millis(30));
                if tcp.write_all(b"x").is_err() {
                    break;
                }
            }
        });
        let started = Instant::now();
        let stop = started + Duration::from_millis(150);
        let tcp = TcpStream::connect_timeout(&endpoint, Duration::from_secs(1)).unwrap();
        let mut socket = DeadlineSocket {
            tcp,
            deadline: stop,
            signal: None,
        };
        assert_eq!(
            read_frame(&mut socket, MAX_REQUEST, stop),
            Err("runner_deadline_exceeded")
        );
        assert!(started.elapsed() < Duration::from_secs(2));
        drop(socket);
        sender.join().unwrap();
    }

    #[test]
    fn preflight_expiry_and_request_oversize_never_connect_or_claim_delivery() {
        let fixture = TlsFixture::new();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let endpoint = listener.local_addr().unwrap();
        let failure = exchange(
            endpoint,
            "runner.test",
            &fixture.server_pin,
            Arc::clone(&fixture.client),
            b"request",
            Instant::now(),
        )
        .unwrap_err();
        assert_eq!(failure.code, "runner_deadline_exceeded");
        assert!(!failure.request_may_have_been_sent);
        let failure = exchange(
            endpoint,
            "runner.test",
            &fixture.server_pin,
            fixture.client,
            &vec![0; MAX_REQUEST + 1],
            deadline(),
        )
        .unwrap_err();
        assert_eq!(failure.code, "runner_frame_invalid");
        assert!(!failure.request_may_have_been_sent);
        assert!(
            matches!(listener.accept(), Err(error) if error.kind() == io::ErrorKind::WouldBlock)
        );
    }
    #[test]
    fn controlled_wait_cancels_promptly_after_request_without_claiming_no_delivery() {
        let fixture = TlsFixture::new();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = listener.local_addr().unwrap();
        let signal = Arc::new(AtomicUsize::new(0));
        let trigger = signal.clone();
        let (release, wait) = std::sync::mpsc::channel();
        let server = thread::spawn(move || {
            let (tcp, _) = listener.accept().unwrap();
            let mut channel = accept(tcp, fixture.server, deadline()).unwrap();
            assert_eq!(channel.read_request().unwrap(), b"admit");
            trigger.store(2, Ordering::Relaxed);
            wait.recv_timeout(Duration::from_secs(3)).unwrap();
        });
        let start = Instant::now();
        let error = exchange_controlled(
            endpoint,
            "runner.test",
            &fixture.server_pin,
            fixture.client,
            b"admit",
            deadline(),
            Some(signal),
        )
        .unwrap_err();
        assert_eq!(error.code, "runner_cancelled");
        assert!(error.request_may_have_been_sent);
        assert!(start.elapsed() < Duration::from_secs(2));
        release.send(()).unwrap();
        server.join().unwrap();
    }
    #[test]
    fn already_cancelled_connect_has_no_application_delivery() {
        let fixture = TlsFixture::new();
        let signal = Arc::new(AtomicUsize::new(15));
        let error = exchange_controlled(
            "127.0.0.1:9".parse().unwrap(),
            "runner.test",
            &fixture.server_pin,
            fixture.client,
            b"admit",
            deadline(),
            Some(signal),
        )
        .unwrap_err();
        assert!(!error.request_may_have_been_sent);
        assert_eq!(error.code, "runner_cancelled");
    }
}
