//! Explicitly enabled, ephemeral TLS transport for the phone work console.
//! Tokens and private keys live only in memory. The owner must also invalidate its
//! execution generation on stop: an already dispatched command can outlive HTTP.
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rand_core::{OsRng, RngCore};
use rustls::{ServerConfig, ServerConnection, StreamOwned, pki_types::PrivatePkcs8KeyDer};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    io::{Read, Write},
    net::{IpAddr, Shutdown, TcpListener, TcpStream},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use subtle::ConstantTimeEq;

const MAX_CLIENTS: usize = 8;
const MAX_HEADER: usize = 8192;
const MAX_BODY: usize = 16384;
const REQUEST_LIFETIME: Duration = Duration::from_secs(5);
type Handler = dyn Fn(Value) -> Result<Value, String> + Send + Sync;
type Sockets = Arc<Mutex<HashMap<u64, (Instant, TcpStream)>>>;

pub struct Transport {
    active: Arc<AtomicBool>,
    sockets: Sockets,
    listener: Mutex<Option<JoinHandle<()>>>,
}

impl Transport {
    pub fn start(host: &str, handler: Arc<Handler>) -> Result<(Self, String), String> {
        let ip = validate_host(host)?;
        let certificate = rcgen::generate_simple_self_signed(vec![ip.to_string()])
            .map_err(|_| "Could not generate console certificate")?;
        let fingerprint = hex(&Sha256::digest(certificate.cert.der()));
        let config =
            ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
                .with_safe_default_protocol_versions()
                .map_err(|_| "TLS protocol unavailable")?
                .with_no_client_auth()
                .with_single_cert(
                    vec![certificate.cert.der().clone()],
                    PrivatePkcs8KeyDer::from(certificate.key_pair.serialize_der()).into(),
                )
                .map_err(|_| "Could not initialize console TLS")?;
        let mut random = [0_u8; 32];
        OsRng
            .try_fill_bytes(&mut random)
            .map_err(|_| "Secure randomness unavailable")?;
        let token = URL_SAFE_NO_PAD.encode(random);
        let token_hash: [u8; 32] = Sha256::digest(token.as_bytes()).into();
        let address = if ip.is_ipv4() { "0.0.0.0:0" } else { "[::]:0" };
        let listener = TcpListener::bind(address).map_err(|_| "Could not open console port")?;
        let port = listener
            .local_addr()
            .map_err(|_| "Could not read console port")?
            .port();
        listener
            .set_nonblocking(true)
            .map_err(|_| "Could not initialize console listener")?;
        let qr_payload = format!("repose://console/v1/{}", URL_SAFE_NO_PAD.encode(
            serde_json::to_vec(&json!({"v":1,"host":ip.to_string(),"port":port,"token":token,"fingerprint":fingerprint}))
                .map_err(|_| "Could not encode console pairing")?));
        let active = Arc::new(AtomicBool::new(true));
        let sockets: Sockets = Arc::new(Mutex::new(HashMap::new()));
        let next_id = AtomicU64::new(1);
        let worker_active = active.clone();
        let worker_sockets = sockets.clone();
        let config = Arc::new(config);
        let handle = thread::Builder::new()
            .name("console-listener".into())
            .spawn(move || {
                while worker_active.load(Ordering::Acquire) {
                    // A wall-clock limit also bounds peers that trickle TLS bytes fast
                    // enough to avoid the socket's per-read timeout.
                    if let Ok(clients) = worker_sockets.lock() {
                        for (deadline, socket) in clients.values() {
                            if Instant::now() >= *deadline {
                                let _ = socket.shutdown(Shutdown::Both);
                            }
                        }
                    }
                    match listener.accept() {
                        Ok((socket, _)) => {
                            let id = next_id.fetch_add(1, Ordering::Relaxed);
                            let Ok(copy) = socket.try_clone() else {
                                continue;
                            };
                            {
                                let Ok(mut clients) = worker_sockets.lock() else {
                                    break;
                                };
                                if clients.len() >= MAX_CLIENTS {
                                    let _ = socket.shutdown(Shutdown::Both);
                                    continue;
                                }
                                clients.insert(id, (Instant::now() + REQUEST_LIFETIME, copy));
                            }
                            let active = worker_active.clone();
                            let sockets = worker_sockets.clone();
                            let handler = handler.clone();
                            let config = config.clone();
                            let cleanup_sockets = worker_sockets.clone();
                            if thread::Builder::new()
                                .name("console-request".into())
                                .spawn(move || {
                                    let _client = ClientGuard { id, sockets };
                                    if socket.set_nonblocking(false).is_err() {
                                        return;
                                    }
                                    let _ = socket.set_read_timeout(Some(Duration::from_secs(2)));
                                    let _ = socket.set_write_timeout(Some(Duration::from_secs(2)));
                                    let Ok(connection) = ServerConnection::new(config) else {
                                        return;
                                    };
                                    let mut stream = StreamOwned::new(connection, socket);
                                    let response = read_request(&mut stream, &token_hash).and_then(
                                        |request| {
                                            if !active.load(Ordering::Acquire) {
                                                return Err((503, "Console connection closed"));
                                            }
                                            Ok(match handler(request) {
                                                Ok(data) => (200, json!({"ok":true,"data":data})),
                                                Err(error) => {
                                                    (400, json!({"ok":false,"error":error}))
                                                }
                                            })
                                        },
                                    );
                                    let (code, body) = response.unwrap_or_else(|(code, error)| {
                                        (code, json!({"ok":false,"error":error}))
                                    });
                                    let _ = write_response(&mut stream, code, &body);
                                    stream.conn.send_close_notify();
                                    let _ = stream.flush();
                                })
                                .is_err()
                                && let Ok(mut clients) = cleanup_sockets.lock()
                            {
                                clients.remove(&id);
                            }
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(20))
                        }
                        Err(_) => break,
                    }
                }
                worker_active.store(false, Ordering::Release);
                close_sockets(&worker_sockets);
            })
            .map_err(|_| "Could not start console listener")?;
        Ok((
            Self {
                active,
                sockets,
                listener: Mutex::new(Some(handle)),
            },
            qr_payload,
        ))
    }

    /// Revoke the token, close sockets, and stop accepting. The command owner
    /// separately cancels any keyboard sequence that was already dispatched.
    pub fn stop(&self) {
        self.active.store(false, Ordering::Release);
        close_sockets(&self.sockets);
        if let Ok(mut listener) = self.listener.lock()
            && let Some(handle) = listener.take()
        {
            let _ = handle.join();
        }
    }
}
impl Drop for Transport {
    fn drop(&mut self) {
        self.stop();
    }
}
struct ClientGuard {
    id: u64,
    sockets: Sockets,
}
impl Drop for ClientGuard {
    fn drop(&mut self) {
        if let Ok(mut clients) = self.sockets.lock() {
            clients.remove(&self.id);
        }
    }
}
fn close_sockets(sockets: &Sockets) {
    if let Ok(mut clients) = sockets.lock() {
        for (_, socket) in clients.values() {
            let _ = socket.shutdown(Shutdown::Both);
        }
        clients.clear();
    }
}
fn validate_host(host: &str) -> Result<IpAddr, String> {
    let ip: IpAddr = host
        .parse()
        .map_err(|_| "Enter the Mac's local network IP address")?;
    let valid = match ip {
        IpAddr::V4(ip) => ip.is_private() || ip.is_link_local() || ip.is_loopback(),
        IpAddr::V6(ip) => ip.is_unique_local() || ip.is_loopback(),
    };
    if valid {
        Ok(ip)
    } else {
        Err("Use a private LAN IP address (IPv6 link-local scopes are not supported)".into())
    }
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
fn read_request(
    reader: &mut impl Read,
    token_hash: &[u8; 32],
) -> Result<Value, (u16, &'static str)> {
    let mut header = Vec::with_capacity(1024);
    while !header.ends_with(b"\r\n\r\n") {
        if header.len() >= MAX_HEADER {
            return Err((431, "Request headers too large"));
        }
        let mut byte = [0];
        reader
            .read_exact(&mut byte)
            .map_err(|_| (400, "Incomplete request"))?;
        header.push(byte[0]);
    }
    let header = std::str::from_utf8(&header).map_err(|_| (400, "Invalid request headers"))?;
    let mut lines = header.split("\r\n");
    if !matches!(
        lines.next(),
        Some("POST /console HTTP/1.1" | "POST /console HTTP/1.0")
    ) {
        return Err((404, "Unknown endpoint"));
    }
    let mut authorization = None;
    let mut content_length = None;
    let mut content_type = None;
    for line in lines.take_while(|line| !line.is_empty()) {
        if line.starts_with([' ', '\t']) {
            return Err((400, "Invalid request headers"));
        }
        let (name, value) = line
            .split_once(':')
            .ok_or((400, "Invalid request headers"))?;
        let value = value.trim();
        if name.eq_ignore_ascii_case("authorization") {
            if authorization.replace(value).is_some() {
                return Err((400, "Duplicate authorization"));
            }
        } else if name.eq_ignore_ascii_case("content-length") {
            if content_length.is_some()
                || value.is_empty()
                || !value.bytes().all(|b| b.is_ascii_digit())
            {
                return Err((400, "Invalid content length"));
            }
            content_length = Some(
                value
                    .parse::<usize>()
                    .map_err(|_| (413, "Request body too large"))?,
            );
        } else if name.eq_ignore_ascii_case("transfer-encoding") {
            return Err((400, "Chunked requests are not supported"));
        } else if name.eq_ignore_ascii_case("content-type") && content_type.replace(value).is_some()
        {
            return Err((400, "Duplicate content type"));
        }
    }
    let supplied = authorization
        .and_then(|auth| auth.strip_prefix("Bearer "))
        .ok_or((401, "Unauthorized"))?;
    let supplied_hash = Sha256::digest(supplied.as_bytes());
    if !bool::from(supplied_hash.as_slice().ct_eq(token_hash)) {
        return Err((401, "Unauthorized"));
    }
    if !content_type.is_some_and(|value| {
        value
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .eq_ignore_ascii_case("application/json")
    }) {
        return Err((415, "Use application/json"));
    }
    let length = content_length.ok_or((411, "Content length required"))?;
    if length == 0 || length > MAX_BODY {
        return Err((413, "Request body size out of range"));
    }
    let mut body = vec![0; length];
    reader
        .read_exact(&mut body)
        .map_err(|_| (400, "Incomplete request body"))?;
    let value: Value = serde_json::from_slice(&body).map_err(|_| (400, "Invalid JSON"))?;
    if !value.is_object() {
        return Err((400, "Expected request object"));
    }
    Ok(value)
}
fn write_response(writer: &mut impl Write, code: u16, body: &Value) -> std::io::Result<()> {
    let body = serde_json::to_vec(body)?;
    let reason = if code == 200 {
        "OK"
    } else {
        "Request rejected"
    };
    write!(
        writer,
        "HTTP/1.1 {code} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n\r\n",
        body.len()
    )?;
    writer.write_all(&body)?;
    writer.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustls::{
        ClientConfig, ClientConnection, DigitallySignedStruct, RootCertStore, SignatureScheme,
        client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
        pki_types::{CertificateDer, ServerName, UnixTime},
    };
    use std::io::Cursor;
    fn wire(token: &str, body: &str) -> Vec<u8> {
        format!("POST /console HTTP/1.1\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}", body.len()).into_bytes()
    }
    #[test]
    fn rejects_unauthorized_before_reading_body() {
        let hash = Sha256::digest(b"secret").into();
        assert_eq!(
            read_request(&mut Cursor::new(wire("wrong", "{}")), &hash)
                .unwrap_err()
                .0,
            401
        );
        assert_eq!(
            read_request(
                &mut Cursor::new(wire("secret", "{\"type\":\"status\"}")),
                &hash
            )
            .unwrap()["type"],
            "status"
        );
    }
    #[test]
    fn bounds_and_validates_http_and_json() {
        let hash = Sha256::digest(b"secret").into();
        for body in ["[]", "null", "{bad"] {
            assert_eq!(
                read_request(&mut Cursor::new(wire("secret", body)), &hash)
                    .unwrap_err()
                    .0,
                400
            );
        }
        let mut large = wire("secret", &"x".repeat(MAX_BODY + 1));
        assert_eq!(
            read_request(&mut Cursor::new(&mut large), &hash)
                .unwrap_err()
                .0,
            413
        );
        let header = format!("POST /console HTTP/1.1\r\nX: {}", "x".repeat(MAX_HEADER));
        assert_eq!(
            read_request(&mut Cursor::new(header), &hash).unwrap_err().0,
            431
        );
        let duplicate = String::from_utf8(wire("secret", "{}")).unwrap().replace(
            "Content-Length: 2",
            "Content-Length: 2\r\nContent-Length: 2",
        );
        assert_eq!(
            read_request(&mut Cursor::new(duplicate), &hash)
                .unwrap_err()
                .0,
            400
        );
    }
    #[test]
    fn only_literal_private_hosts() {
        for host in [
            "localhost",
            "https://192.168.1.2",
            "8.8.8.8",
            "0.0.0.0",
            "::",
            "fe80::1%en0",
        ] {
            assert!(validate_host(host).is_err(), "{host}");
        }
        for host in [
            "192.168.1.2",
            "10.0.1.1",
            "172.16.0.1",
            "169.254.1.2",
            "127.0.0.1",
            "fd00::1",
            "::1",
        ] {
            assert!(validate_host(host).is_ok(), "{host}");
        }
    }
    #[derive(Debug)]
    struct Pin(String);
    impl ServerCertVerifier for Pin {
        fn verify_server_cert(
            &self,
            certificate: &CertificateDer<'_>,
            _: &[CertificateDer<'_>],
            _: &ServerName<'_>,
            _: &[u8],
            _: UnixTime,
        ) -> Result<ServerCertVerified, rustls::Error> {
            if hex(&Sha256::digest(certificate)) != self.0 {
                return Err(rustls::Error::General("Pin mismatch".into()));
            }
            Ok(ServerCertVerified::assertion())
        }
        fn verify_tls12_signature(
            &self,
            message: &[u8],
            certificate: &CertificateDer<'_>,
            signature: &DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, rustls::Error> {
            rustls::crypto::verify_tls12_signature(
                message,
                certificate,
                signature,
                &rustls::crypto::ring::default_provider().signature_verification_algorithms,
            )
        }
        fn verify_tls13_signature(
            &self,
            message: &[u8],
            certificate: &CertificateDer<'_>,
            signature: &DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, rustls::Error> {
            rustls::crypto::verify_tls13_signature(
                message,
                certificate,
                signature,
                &rustls::crypto::ring::default_provider().signature_verification_algorithms,
            )
        }
        fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
            rustls::crypto::ring::default_provider()
                .signature_verification_algorithms
                .supported_schemes()
        }
    }
    fn client(qr: &Value, pin: String) -> StreamOwned<ClientConnection, TcpStream> {
        let mut config =
            ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
                .with_safe_default_protocol_versions()
                .unwrap()
                .with_root_certificates(RootCertStore::empty())
                .with_no_client_auth();
        config
            .dangerous()
            .set_certificate_verifier(Arc::new(Pin(pin)));
        let connection =
            ClientConnection::new(Arc::new(config), ServerName::try_from("127.0.0.1").unwrap())
                .unwrap();
        let socket =
            TcpStream::connect(("127.0.0.1", qr["port"].as_u64().unwrap() as u16)).unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        StreamOwned::new(connection, socket)
    }
    #[test]
    fn tls_pin_auth_dispatch_and_revocation() {
        let calls = Arc::new(AtomicU64::new(0));
        let count = calls.clone();
        let (transport, qr) = Transport::start(
            "127.0.0.1",
            Arc::new(move |_| {
                count.fetch_add(1, Ordering::Relaxed);
                Ok(json!({"enabled":true}))
            }),
        )
        .unwrap();
        let qr: Value = serde_json::from_slice(
            &URL_SAFE_NO_PAD
                .decode(qr.strip_prefix("repose://console/v1/").unwrap())
                .unwrap(),
        )
        .unwrap();
        assert_eq!(qr["fingerprint"].as_str().unwrap().len(), 64);
        assert_eq!(
            URL_SAFE_NO_PAD
                .decode(qr["token"].as_str().unwrap())
                .unwrap()
                .len(),
            32
        );
        let mut wrong_pin = client(&qr, "0".repeat(64));
        assert!(
            wrong_pin
                .write_all(&wire(
                    qr["token"].as_str().unwrap(),
                    "{\"type\":\"status\"}"
                ))
                .is_err()
        );
        for (token, expected) in [("wrong", "401"), (qr["token"].as_str().unwrap(), "200")] {
            let mut connection = client(&qr, qr["fingerprint"].as_str().unwrap().to_owned());
            connection
                .write_all(&wire(token, "{\"type\":\"status\"}"))
                .unwrap();
            let mut response = String::new();
            connection.read_to_string(&mut response).unwrap();
            assert!(
                response.starts_with(&format!("HTTP/1.1 {expected}")),
                "{response}"
            );
        }
        assert_eq!(calls.load(Ordering::Relaxed), 1);
        let mut pending = client(&qr, qr["fingerprint"].as_str().unwrap().to_owned());
        pending.write_all(b"POST /console HTTP/1.1\r\n").unwrap();
        transport.stop();
        let _ = pending.write_all(&wire(qr["token"].as_str().unwrap(), "{}"));
        assert_eq!(calls.load(Ordering::Relaxed), 1);
        assert!(TcpStream::connect(("127.0.0.1", qr["port"].as_u64().unwrap() as u16)).is_err());
    }
    #[test]
    fn caps_pending_clients_and_closes_them_on_stop() {
        let (transport, qr) = Transport::start(
            "127.0.0.1",
            Arc::new(|_| panic!("Unfinished TLS must not dispatch")),
        )
        .unwrap();
        let qr: Value = serde_json::from_slice(
            &URL_SAFE_NO_PAD
                .decode(qr.strip_prefix("repose://console/v1/").unwrap())
                .unwrap(),
        )
        .unwrap();
        let address = ("127.0.0.1", qr["port"].as_u64().unwrap() as u16);
        let mut pending = Vec::new();
        for _ in 0..MAX_CLIENTS {
            pending.push(TcpStream::connect(address).unwrap());
        }
        let deadline = Instant::now() + Duration::from_secs(1);
        while transport.sockets.lock().unwrap().len() < MAX_CLIENTS && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(transport.sockets.lock().unwrap().len(), MAX_CLIENTS);
        let mut overflow = TcpStream::connect(address).unwrap();
        overflow
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        assert_eq!(overflow.read(&mut [0]).unwrap(), 0);
        transport.stop();
        assert!(transport.sockets.lock().unwrap().is_empty());
        for mut socket in pending {
            socket
                .set_read_timeout(Some(Duration::from_secs(1)))
                .unwrap();
            assert_eq!(socket.read(&mut [0]).unwrap(), 0);
        }
    }
}
