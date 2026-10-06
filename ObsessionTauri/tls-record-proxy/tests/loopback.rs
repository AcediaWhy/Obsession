use obsession_tls_record_proxy::{bind, prepare_client_hello, run, Config};
use rustls::{
    pki_types::{PrivatePkcs8KeyDer, ServerName},
    ClientConfig, RootCertStore, ServerConfig,
};
use std::{
    io,
    net::SocketAddr,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf},
    net::{TcpListener, TcpStream},
    sync::oneshot,
    task::JoinHandle,
    time::{sleep, timeout},
};
use tokio_rustls::{TlsAcceptor, TlsConnector};

const NAME: &str = "prototype.test";
const RESPONSE_BYTES: usize = 400_000;
const REQUEST_BYTES: usize = 200_000;

trait Io: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Io for T {}

struct RecordingIo<T> {
    inner: T,
    written: Arc<Mutex<Vec<u8>>>,
}
impl<T: AsyncRead + Unpin> AsyncRead for RecordingIo<T> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_read(cx, buffer)
    }
}
impl<T: AsyncWrite + Unpin> AsyncWrite for RecordingIo<T> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<io::Result<usize>> {
        let result = Pin::new(&mut self.inner).poll_write(cx, buffer);
        if let Poll::Ready(Ok(size)) = result {
            let mut captured = self.written.lock().unwrap();
            // Only the handshake flight is needed, never application payloads.
            if captured.len() < 8192 {
                let end = size.min(8192 - captured.len());
                captured.extend_from_slice(&buffer[..end]);
            }
        }
        result
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

struct PrefixIo<T> {
    prefix: Vec<u8>,
    position: usize,
    inner: T,
}
impl<T: AsyncRead + Unpin> AsyncRead for PrefixIo<T> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        output: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if self.position < self.prefix.len() {
            let end = (self.position + output.remaining()).min(self.prefix.len());
            output.put_slice(&self.prefix[self.position..end]);
            self.position = end;
            return Poll::Ready(Ok(()));
        }
        Pin::new(&mut self.inner).poll_read(cx, output)
    }
}
impl<T: AsyncWrite + Unpin> AsyncWrite for PrefixIo<T> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(cx, buffer)
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

// Independent wire decoder verifies both handshake identity and record boundaries.
fn decode_hello(wire: &[u8]) -> (Vec<u8>, Vec<usize>, usize) {
    let mut handshake = Vec::new();
    let mut boundaries = Vec::new();
    let mut position = 0;
    loop {
        assert_eq!(wire[position], 22);
        let size = u16::from_be_bytes([wire[position + 3], wire[position + 4]]) as usize;
        handshake.extend_from_slice(&wire[position + 5..position + 5 + size]);
        position += 5 + size;
        boundaries.push(handshake.len());
        if handshake.len() >= 4 {
            let length = 4
                + ((handshake[1] as usize) << 16)
                + ((handshake[2] as usize) << 8)
                + handshake[3] as usize;
            if handshake.len() >= length {
                handshake.truncate(length);
                return (handshake, boundaries, position);
            }
        }
    }
}

async fn read_hello_wire(stream: &mut TcpStream) -> Vec<u8> {
    let mut wire = Vec::new();
    let mut handshake = Vec::new();
    loop {
        let mut header = [0; 5];
        stream.read_exact(&mut header).await.unwrap();
        assert_eq!(header[0], 22);
        let size = u16::from_be_bytes([header[3], header[4]]) as usize;
        let mut payload = vec![0; size];
        stream.read_exact(&mut payload).await.unwrap();
        wire.extend_from_slice(&header);
        wire.extend_from_slice(&payload);
        handshake.extend_from_slice(&payload);
        if handshake.len() >= 4 {
            let length = 4
                + ((handshake[1] as usize) << 16)
                + ((handshake[2] as usize) << 8)
                + handshake[3] as usize;
            if handshake.len() >= length {
                return wire;
            }
        }
    }
}

async fn spawn_proxy(
    mut cfg: Config,
) -> (SocketAddr, oneshot::Sender<()>, JoinHandle<io::Result<()>>) {
    cfg.listen = "127.0.0.1:0".parse().unwrap();
    let listener = bind(&cfg).await.unwrap();
    let address = listener.local_addr().unwrap();
    let (send, receive) = oneshot::channel();
    let task = tokio::spawn(run(listener, cfg, async {
        let _ = receive.await;
    }));
    (address, send, task)
}

fn tls_configs(version: &'static rustls::SupportedProtocolVersion) -> (TlsAcceptor, TlsConnector) {
    let rcgen::CertifiedKey { cert, key_pair } =
        rcgen::generate_simple_self_signed(vec![NAME.to_owned()]).unwrap();
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut server = ServerConfig::builder_with_provider(provider.clone())
        .with_protocol_versions(&[version])
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(
            vec![cert.der().clone()],
            PrivatePkcs8KeyDer::from(key_pair.serialize_der()).into(),
        )
        .unwrap();
    server.alpn_protocols = vec![b"http/1.1".to_vec()];
    let mut roots = RootCertStore::empty();
    roots.add(cert.der().clone()).unwrap();
    let mut client = ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(&[version])
        .unwrap()
        .with_root_certificates(roots)
        .with_no_client_auth();
    client.alpn_protocols = vec![b"http/1.1".to_vec()];
    client.resumption = rustls::client::Resumption::disabled();
    (
        TlsAcceptor::from(Arc::new(server)),
        TlsConnector::from(Arc::new(client)),
    )
}

async fn tls_round_trips(version: &'static rustls::SupportedProtocolVersion, pre_fragment: bool) {
    timeout(Duration::from_secs(15), async {
        let (acceptor, connector) = tls_configs(version);
        let server = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let target = server.local_addr().unwrap();
        let observed = Arc::new(Mutex::new(Vec::new()));
        let server_observed = observed.clone();
        let server_task = tokio::spawn(async move {
            for _ in 0..2 {
                let (mut socket, _) = server.accept().await.unwrap();
                let wire = read_hello_wire(&mut socket).await;
                server_observed.lock().unwrap().push(wire.clone());
                let mut tls = acceptor.accept(PrefixIo { prefix: wire, position: 0, inner: socket }).await.unwrap();
                assert_eq!(tls.get_ref().1.protocol_version().unwrap(), version.version);
                let mut request = Vec::new();
                // A client write-half close must not discard the server response.
                tls.read_to_end(&mut request).await.unwrap();
                let delimiter = request.windows(4).position(|bytes| bytes == b"\r\n\r\n").unwrap() + 4;
                assert_eq!(&request[..4], b"POST");
                assert_eq!(&request[delimiter..], vec![b'q'; REQUEST_BYTES]);
                let response = format!("HTTP/1.1 200 OK\r\nContent-Length: {RESPONSE_BYTES}\r\nConnection: close\r\n\r\n");
                tls.write_all(response.as_bytes()).await.unwrap();
                tls.write_all(&vec![b'r'; RESPONSE_BYTES]).await.unwrap();
                tls.shutdown().await.unwrap();
            }
        });
        let mut cfg = Config::new("127.0.0.1:0".parse().unwrap(), target, NAME.to_string());
        cfg.fragment_bytes = 64;
        let (address, shutdown, proxy) = spawn_proxy(cfg).await;
        let mut original = Vec::new();
        let mut pumps = Vec::new();
        for _ in 0..2 {
            let wire = Arc::new(Mutex::new(Vec::new()));
            original.push(wire.clone());
            let socket: Box<dyn Io> = if pre_fragment {
                let (client, mut bridge) = tokio::io::duplex(64 * 1024);
                let pump = tokio::spawn(async move {
                    let mut network = TcpStream::connect(address).await.unwrap();
                    network.set_nodelay(true).unwrap();
                    let prepared = prepare_client_hello(&mut bridge, NAME, 32).await.unwrap();
                    // Exercise record header/body partial reads as well as fragmented input.
                    for byte in prepared.wire { network.write_all(&[byte]).await.unwrap(); }
                    tokio::io::copy_bidirectional(&mut bridge, &mut network).await.unwrap();
                });
                pumps.push(pump);
                Box::new(client)
            } else { Box::new(TcpStream::connect(address).await.unwrap()) };
            let mut tls = connector.connect(ServerName::try_from(NAME).unwrap(), RecordingIo { inner: socket, written: wire }).await.unwrap();
            assert_eq!(tls.get_ref().1.protocol_version().unwrap(), version.version);
            assert_eq!(tls.get_ref().1.alpn_protocol(), Some(b"http/1.1".as_slice()));
            let header = format!("POST /local-proof HTTP/1.1\r\nHost: {NAME}\r\nContent-Length: {REQUEST_BYTES}\r\n\r\n");
            tls.write_all(header.as_bytes()).await.unwrap();
            tls.write_all(&vec![b'q'; REQUEST_BYTES]).await.unwrap();
            tls.shutdown().await.unwrap();
            let mut response = Vec::new();
            tls.read_to_end(&mut response).await.unwrap();
            let delimiter = response.windows(4).position(|bytes| bytes == b"\r\n\r\n").unwrap() + 4;
            assert!(response.starts_with(b"HTTP/1.1 200 OK\r\n"));
            assert_eq!(&response[delimiter..], vec![b'r'; RESPONSE_BYTES]);
        }
        server_task.await.unwrap();
        for pump in pumps { pump.await.unwrap(); }
        {
            let observed = observed.lock().unwrap();
            for (index, capture) in original.iter().enumerate() {
                let (input, _, _) = decode_hello(&capture.lock().unwrap());
                let (output, boundaries, _) = decode_hello(&observed[index]);
                assert_eq!(output, input, "handshake bytes including key share must be identical");
                assert!(boundaries.len() > 1);
                let name_start = output.windows(NAME.len()).position(|bytes| bytes == NAME.as_bytes()).unwrap();
                assert!(boundaries.contains(&(name_start + NAME.len() / 2)), "hostname must cross record boundary");
                let mut previous = 0;
                for boundary in boundaries { assert!(boundary - previous <= 64); previous = boundary; }
            }
        }
        let _ = shutdown.send(());
        proxy.await.unwrap().unwrap();
    }).await.expect("local TLS proof must finish within 15 seconds");
}

#[tokio::test]
async fn tls13_two_cold_verified_handshakes_large_http_and_half_close() {
    tls_round_trips(&rustls::version::TLS13, false).await;
}

#[tokio::test]
async fn tls12_two_cold_verified_handshakes_large_http_and_half_close() {
    tls_round_trips(&rustls::version::TLS12, false).await;
}

#[tokio::test]
async fn pre_fragmented_and_bytewise_input_retains_full_tls13_session() {
    tls_round_trips(&rustls::version::TLS13, true).await;
}

fn minimal_hello(name: &str) -> Vec<u8> {
    let mut body = vec![3, 3];
    body.extend_from_slice(&[7; 32]);
    body.extend_from_slice(&[0, 0, 2, 0x13, 1, 1, 0]);
    let mut sni = Vec::new();
    sni.extend_from_slice(&((name.len() + 3) as u16).to_be_bytes());
    sni.push(0);
    sni.extend_from_slice(&(name.len() as u16).to_be_bytes());
    sni.extend_from_slice(name.as_bytes());
    body.extend_from_slice(&((sni.len() + 4) as u16).to_be_bytes());
    body.extend_from_slice(&[0, 0]);
    body.extend_from_slice(&(sni.len() as u16).to_be_bytes());
    body.extend_from_slice(&sni);
    let mut handshake = vec![1, 0, (body.len() >> 8) as u8, body.len() as u8];
    handshake.extend_from_slice(&body);
    handshake
}

fn record(payload: &[u8]) -> Vec<u8> {
    let mut wire = vec![22, 3, 1];
    wire.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    wire.extend_from_slice(payload);
    wire
}

#[tokio::test]
async fn preserves_tail_and_header_split_across_input_records() {
    let hello = minimal_hello(NAME);
    let mut wire = record(&hello[..1]);
    wire.extend_from_slice(&record(&hello[1..3]));
    let mut last = hello[3..].to_vec();
    let tail = [20, 0, 0, 2, 5, 6];
    last.extend_from_slice(&tail);
    wire.extend_from_slice(&record(&last));
    let prepared = prepare_client_hello(&mut wire.as_slice(), NAME, 32)
        .await
        .unwrap();
    assert_eq!(prepared.input_records, 3);
    let (decoded, _, end) = decode_hello(&prepared.wire);
    assert_eq!(decoded, hello);
    assert_eq!(&prepared.wire[end..], record(&tail));
}

#[tokio::test]
async fn reject_invalid_first_flight_before_opening_upstream() {
    let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut cfg = Config::new(
        "127.0.0.1:0".parse().unwrap(),
        upstream.local_addr().unwrap(),
        NAME.to_string(),
    );
    cfg.hello_timeout = Duration::from_millis(100);
    let (address, shutdown, proxy) = spawn_proxy(cfg).await;
    let mut cases = vec![
        b"GET / HTTP/1.1\r\n\r\n".to_vec(),
        record(&minimal_hello("other.test")),
        vec![22, 3, 1, 0xff, 0xff],
        record(&[1, 0x10, 0, 0]),
        record(&[2, 0, 0, 0]),
    ];
    let mut truncated = record(&minimal_hello(NAME));
    truncated.truncate(20);
    cases.push(truncated);
    for bytes in cases {
        let mut client = TcpStream::connect(address).await.unwrap();
        let _ = client.write_all(&bytes).await;
        let _ = client.shutdown().await;
        let mut reply = Vec::new();
        let result = timeout(Duration::from_secs(1), client.read_to_end(&mut reply)).await;
        assert!(result.is_ok());
        assert!(reply.is_empty());
    }
    assert!(
        timeout(Duration::from_millis(150), upstream.accept())
            .await
            .is_err(),
        "invalid input must never reach target"
    );
    let _ = shutdown.send(());
    proxy.await.unwrap().unwrap();
}

#[tokio::test]
async fn bounds_connection_count_and_hello_wait_then_releases_capacity() {
    let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut cfg = Config::new(
        "127.0.0.1:0".parse().unwrap(),
        upstream.local_addr().unwrap(),
        NAME.to_string(),
    );
    cfg.max_connections = 1;
    cfg.hello_timeout = Duration::from_millis(150);
    let (address, shutdown, proxy) = spawn_proxy(cfg).await;
    let mut first = TcpStream::connect(address).await.unwrap();
    first.write_all(&[22]).await.unwrap();
    sleep(Duration::from_millis(30)).await;
    let mut second = TcpStream::connect(address).await.unwrap();
    let result = timeout(Duration::from_millis(100), second.read(&mut [0]))
        .await
        .unwrap();
    assert!(matches!(result, Ok(0) | Err(_)));
    let result = timeout(Duration::from_millis(300), first.read(&mut [0]))
        .await
        .unwrap();
    assert!(matches!(result, Ok(0) | Err(_)));
    let mut third = TcpStream::connect(address).await.unwrap();
    third
        .write_all(&record(&minimal_hello(NAME)))
        .await
        .unwrap();
    let accepted = timeout(Duration::from_secs(1), upstream.accept())
        .await
        .unwrap()
        .unwrap();
    drop(accepted);
    let _ = shutdown.send(());
    proxy.await.unwrap().unwrap();
}

#[tokio::test]
async fn lifetime_closes_an_otherwise_live_connection() {
    let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut cfg = Config::new(
        "127.0.0.1:0".parse().unwrap(),
        upstream.local_addr().unwrap(),
        NAME.to_string(),
    );
    cfg.connection_lifetime = Duration::from_millis(120);
    cfg.idle_timeout = Duration::from_secs(1);
    let (address, shutdown, proxy) = spawn_proxy(cfg).await;
    let mut client = TcpStream::connect(address).await.unwrap();
    client
        .write_all(&record(&minimal_hello(NAME)))
        .await
        .unwrap();
    let (_server, _) = upstream.accept().await.unwrap();
    assert_eq!(
        timeout(Duration::from_millis(500), client.read(&mut [0]))
            .await
            .unwrap()
            .unwrap(),
        0
    );
    let _ = shutdown.send(());
    proxy.await.unwrap().unwrap();
}

#[tokio::test]
async fn idle_deadline_closes_without_response() {
    let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut cfg = Config::new(
        "127.0.0.1:0".parse().unwrap(),
        upstream.local_addr().unwrap(),
        NAME.to_string(),
    );
    cfg.idle_timeout = Duration::from_millis(80);
    let (address, shutdown, proxy) = spawn_proxy(cfg).await;
    let mut client = TcpStream::connect(address).await.unwrap();
    client
        .write_all(&record(&minimal_hello(NAME)))
        .await
        .unwrap();
    let (_server, _) = upstream.accept().await.unwrap();
    assert_eq!(
        timeout(Duration::from_millis(400), client.read(&mut [0]))
            .await
            .unwrap()
            .unwrap(),
        0
    );
    let _ = shutdown.send(());
    proxy.await.unwrap().unwrap();
}

#[test]
fn rejects_non_loopback_and_unbounded_parameters() {
    let mut cfg = Config::new(
        "0.0.0.0:18443".parse().unwrap(),
        "127.0.0.1:443".parse().unwrap(),
        NAME.to_string(),
    );
    assert!(cfg.validate().is_err());
    cfg.listen = "127.0.0.2:18443".parse().unwrap();
    assert!(cfg.validate().is_err());
    cfg.listen = "127.0.0.1:18443".parse().unwrap();
    cfg.fragment_bytes = 1;
    assert!(cfg.validate().is_err());
    cfg.fragment_bytes = 512;
    cfg.max_connections = 129;
    assert!(cfg.validate().is_err());
    cfg.max_connections = 16;
    cfg.server_name = "127.0.0.1".to_owned();
    assert!(cfg.validate().is_err());
}

#[tokio::test]
async fn tls13_websocket_upgrade_and_masked_ping_pong_stay_end_to_end() {
    timeout(Duration::from_secs(5), async {
        let (acceptor, connector) = tls_configs(&rustls::version::TLS13);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let cfg = Config::new("127.0.0.1:0".parse().unwrap(), listener.local_addr().unwrap(), NAME.to_owned());
        let (address, shutdown, proxy) = spawn_proxy(cfg).await;
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let mut tls = acceptor.accept(socket).await.unwrap();
            let mut request = Vec::new();
            loop {
                request.push(tls.read_u8().await.unwrap());
                if request.ends_with(b"\r\n\r\n") { break; }
                assert!(request.len() < 2048);
            }
            assert!(request.starts_with(b"GET /gateway HTTP/1.1\r\n"));
            assert!(request.windows(24).any(|bytes| bytes == b"dGhlIHNhbXBsZSBub25jZQ=="));
            tls.write_all(b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: s3pPLMBiTxaQ9kYGzzhZRbK+xOo=\r\n\r\n").await.unwrap();
            for _ in 0..3 {
                let mut ping = [0u8; 11];
                tls.read_exact(&mut ping).await.unwrap();
                assert_eq!(ping[..2], [0x89, 0x85]);
                let decoded: Vec<u8> = ping[6..].iter().enumerate().map(|(index, byte)| byte ^ ping[2 + index % 4]).collect();
                assert_eq!(decoded, b"proof");
                tls.write_all(b"\x8a\x05proof").await.unwrap();
            }
            tls.shutdown().await.unwrap();
        });
        let mut client = connector.connect(ServerName::try_from(NAME).unwrap(), TcpStream::connect(address).await.unwrap()).await.unwrap();
        client.write_all(b"GET /gateway HTTP/1.1\r\nHost: prototype.test\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n").await.unwrap();
        let mut response = Vec::new();
        while !response.ends_with(b"\r\n\r\n") {
            response.push(client.read_u8().await.unwrap());
            assert!(response.len() < 2048);
        }
        assert!(response.starts_with(b"HTTP/1.1 101"));
        assert!(response.windows(28).any(|bytes| bytes == b"s3pPLMBiTxaQ9kYGzzhZRbK+xOo="));
        for _ in 0..3 {
            let mask = [1, 2, 3, 4];
            let mut ping = vec![0x89, 0x85];
            ping.extend_from_slice(&mask);
            ping.extend(b"proof".iter().enumerate().map(|(index, byte)| byte ^ mask[index % 4]));
            client.write_all(&ping).await.unwrap();
            let mut pong = [0; 7];
            client.read_exact(&mut pong).await.unwrap();
            assert_eq!(&pong, b"\x8a\x05proof");
        }
        let mut trailing = Vec::new();
        client.read_to_end(&mut trailing).await.unwrap();
        assert!(trailing.is_empty());
        client.shutdown().await.unwrap();
        server.await.unwrap();
        let _ = shutdown.send(());
        proxy.await.unwrap().unwrap();
    }).await.expect("local WSS proof deadline");
}

// An independent native HTTPS client checks routing plus original-name certificate validation.
// curl is already present on this Windows host; no curl installation or trust-store modification.
#[tokio::test]
#[cfg(windows)]
async fn native_curl_connect_to_preserves_original_certificate_validation() {
    timeout(Duration::from_secs(8), async {
        let rcgen::CertifiedKey { cert, key_pair } =
            rcgen::generate_simple_self_signed(vec![NAME.to_owned()]).unwrap();
        let server =
            ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
                .with_protocol_versions(&[&rustls::version::TLS13])
                .unwrap()
                .with_no_client_auth()
                .with_single_cert(
                    vec![cert.der().clone()],
                    PrivatePkcs8KeyDer::from(key_pair.serialize_der()).into(),
                )
                .unwrap();
        let acceptor = TlsAcceptor::from(Arc::new(server));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let cfg = Config::new(
            "127.0.0.1:0".parse().unwrap(),
            listener.local_addr().unwrap(),
            NAME.to_owned(),
        );
        let (address, shutdown, proxy) = spawn_proxy(cfg).await;
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let mut tls = acceptor.accept(socket).await.unwrap();
            let mut header = Vec::new();
            while !header.ends_with(b"\r\n\r\n") {
                header.push(tls.read_u8().await.unwrap());
                assert!(header.len() < 4096);
            }
            assert!(header.starts_with(b"GET /native-proof HTTP/1.1\r\n"));
            tls.write_all(
                b"HTTP/1.1 200 OK\r\nContent-Length: 14\r\nConnection: close\r\n\r\nlocal-proof-ok",
            )
            .await
            .unwrap();
            tls.shutdown().await.unwrap();
        });
        let certificate_path = std::env::temp_dir().join(format!(
            "obsession-tls-proof-{}-{}.pem",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        struct RemoveCertificate(std::path::PathBuf);
        impl Drop for RemoveCertificate {
            fn drop(&mut self) {
                let _ = std::fs::remove_file(&self.0);
            }
        }
        // Only the public test certificate reaches disk; its private key stays in memory.
        std::fs::write(&certificate_path, cert.pem()).unwrap();
        let _remove_certificate = RemoveCertificate(certificate_path.clone());
        let curl = tokio::task::spawn_blocking(move || {
            let binary = if cfg!(windows) { "curl.exe" } else { "curl" };
            std::process::Command::new(binary)
                .args([
                    "--silent",
                    "--show-error",
                    "--fail",
                    "--max-time",
                    "5",
                    "--http1.1",
                    "--tlsv1.3",
                    "--tls-max",
                    "1.3",
                    "--noproxy",
                    "*",
                    "--ssl-no-revoke",
                    "--cacert",
                ])
                .arg(certificate_path)
                .arg("--connect-to")
                .arg(format!("{NAME}:443:127.0.0.1:{}", address.port()))
                .arg(format!("https://{NAME}/native-proof"))
                .output()
                .unwrap()
        })
        .await
        .unwrap();
        assert!(
            curl.status.success(),
            "native curl local proof failed: {}",
            String::from_utf8_lossy(&curl.stderr)
        );
        assert_eq!(curl.stdout, b"local-proof-ok");
        server.await.unwrap();
        let _ = shutdown.send(());
        proxy.await.unwrap().unwrap();
    })
    .await
    .expect("local native HTTPS proof deadline");
}
