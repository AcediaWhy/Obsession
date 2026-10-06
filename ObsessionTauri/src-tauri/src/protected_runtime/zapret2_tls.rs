//! Fresh, authenticated TLS for two large ClientHello layouts. No journal bytes,
//! saved key shares, credentials, or custom certificate verification are used.
use serde::Serialize;
use std::{
    io,
    pin::Pin,
    sync::{Arc, OnceLock},
    task::{Context, Poll},
};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

pub(super) const POLICY: &str = "zapret2_large_clienthello_early_late_v1";
const MAX_LAYOUT_ATTEMPTS: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Kind {
    LargeEarlySni,
    LargeLateSni,
}
pub(super) const KINDS: [Kind; 2] = [Kind::LargeEarlySni, Kind::LargeLateSni];

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Metadata {
    pub kind: Kind,
    pub handshake_bytes: usize,
    pub sni_offset: usize,
    pub hybrid_key_bytes: usize,
    pub authenticated_tls: bool,
    pub application_ok: bool,
}
impl Metadata {
    fn matches_layout(&self) -> bool {
        (1500..=4096).contains(&self.handshake_bytes)
            && self.hybrid_key_bytes == 1216
            && match self.kind {
                Kind::LargeEarlySni => self.sni_offset < 512,
                Kind::LargeLateSni => self.sni_offset >= 1024,
            }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Evidence {
    pub policy: String,
    pub handshakes: Vec<Metadata>,
}
impl Evidence {
    pub fn passed(&self) -> bool {
        self.policy == POLICY
            && self.handshakes.len() == KINDS.len()
            && self.handshakes.iter().zip(KINDS).all(|(row, kind)| {
                row.kind == kind
                    && row.matches_layout()
                    && row.authenticated_tls
                    && row.application_ok
            })
    }
}

pub(super) fn config() -> Arc<rustls::ClientConfig> {
    static CONFIG: OnceLock<Arc<rustls::ClientConfig>> = OnceLock::new();
    CONFIG
        .get_or_init(|| {
            config_with_roots(rustls::RootCertStore::from_iter(
                webpki_roots::TLS_SERVER_ROOTS.iter().cloned(),
            ))
        })
        .clone()
}

pub(super) fn config_with_roots(roots: rustls::RootCertStore) -> Arc<rustls::ClientConfig> {
    use rustls::crypto::{aws_lc_rs, hpke::HpkePublicKey};
    let mut provider = aws_lc_rs::default_provider();
    // Choose the same hybrid group size observed in contemporary Chromium.
    provider.kx_groups = vec![
        aws_lc_rs::kx_group::X25519MLKEM768,
        aws_lc_rs::kx_group::X25519,
        aws_lc_rs::kx_group::SECP256R1,
    ];
    let placeholder = aws_lc_rs::kx_group::X25519.start().expect("bundled X25519");
    let grease = rustls::client::EchGreaseConfig::new(
        aws_lc_rs::hpke::DH_KEM_X25519_HKDF_SHA256_AES_128,
        HpkePublicKey(placeholder.pub_key().to_vec()),
    );
    let mut config = rustls::ClientConfig::builder_with_provider(Arc::new(provider))
        .with_ech(rustls::client::EchMode::Grease(grease))
        .expect("bundled GREASE suite")
        .with_root_certificates(roots)
        .with_no_client_auth();
    config.resumption = rustls::client::Resumption::disabled();
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    Arc::new(config)
}

pub(super) struct Prepared {
    connection: rustls::ClientConnection,
    wire: Vec<u8>,
    pub metadata: Metadata,
}

pub(super) fn prepare(
    host: &str,
    config: Arc<rustls::ClientConfig>,
    kind: Kind,
) -> io::Result<Prepared> {
    let name = rustls::pki_types::ServerName::try_from(host.to_owned())
        .map_err(|_| io::Error::other("invalid compatibility TLS name"))?;
    // Rustls randomizes extension order. Keep the complete state of the fresh
    // connection whose layout matches; never rewrite bytes or its transcript.
    for _ in 0..MAX_LAYOUT_ATTEMPTS {
        let mut connection = rustls::ClientConnection::new(config.clone(), name.clone())
            .map_err(io::Error::other)?;
        let mut wire = Vec::new();
        connection.write_tls(&mut wire)?;
        if let Some(metadata) = summarize(&wire, host, kind).filter(Metadata::matches_layout) {
            return Ok(Prepared {
                connection,
                wire,
                metadata,
            });
        }
    }
    Err(io::Error::other(
        "required large ClientHello layout could not be generated",
    ))
}

fn summarize(wire: &[u8], host: &str, kind: Kind) -> Option<Metadata> {
    // These are fresh messages from our own TLS library, not unbounded NetLog.
    if wire.len() < 9 || wire.len() > 8192 || wire[0] != 22 || wire[5] != 1 {
        return None;
    }
    let u16_at = |bytes: &[u8], pos| {
        Some(u16::from_be_bytes(bytes.get(pos..pos + 2)?.try_into().ok()?) as usize)
    };
    let record_len = u16_at(wire, 3)?;
    let bytes = wire.get(5..5 + record_len)?;
    let len = ((bytes[1] as usize) << 16) | ((bytes[2] as usize) << 8) | bytes[3] as usize;
    if len + 4 != bytes.len() {
        return None;
    }
    let mut pos = 39usize.checked_add(*bytes.get(38)? as usize)?;
    pos = pos.checked_add(2 + u16_at(bytes, pos)?)?;
    pos = pos.checked_add(1 + *bytes.get(pos)? as usize)?;
    let end = pos.checked_add(2 + u16_at(bytes, pos)?)?;
    pos += 2;
    if end != bytes.len() {
        return None;
    }
    let mut sni_offset = None;
    let mut hybrid_key_bytes = 0;
    while pos < end {
        let typ = u16_at(bytes, pos)?;
        let size = u16_at(bytes, pos + 2)?;
        let start = pos + 4;
        let next = start.checked_add(size)?;
        let ext = bytes.get(start..next)?;
        match typ {
            0 => {
                if u16_at(ext, 0)? + 2 != size || *ext.get(2)? != 0 {
                    return None;
                }
                let name_len = u16_at(ext, 3)?;
                if 5 + name_len != size || ext.get(5..)? != host.as_bytes() {
                    return None;
                }
                sni_offset = Some(start + 5);
            }
            51 => {
                if u16_at(ext, 0)? + 2 != size {
                    return None;
                }
                let mut cursor = 2;
                while cursor < ext.len() {
                    let group = u16_at(ext, cursor)?;
                    let key_len = u16_at(ext, cursor + 2)?;
                    cursor += 4;
                    ext.get(cursor..cursor + key_len)?;
                    if group == 4588 {
                        hybrid_key_bytes = key_len;
                    }
                    cursor += key_len;
                }
                if cursor != size {
                    return None;
                }
            }
            41 | 42 => return None, // No PSK/resumption or early data in a fresh experiment.
            _ => {}
        }
        pos = next;
    }
    Some(Metadata {
        kind,
        handshake_bytes: bytes.len(),
        sni_offset: sni_offset?,
        hybrid_key_bytes,
        authenticated_tls: false,
        application_ok: false,
    })
}

/// write_tls drained only the initial flight for inspection. PrefixIo delivers
/// those exact bytes once before reading server data or writing later flights.
/// The original Rustls connection still owns every key and transcript state.
pub(super) fn connect<IO: AsyncRead + AsyncWrite + Unpin>(
    io: IO,
    name: rustls::pki_types::ServerName<'static>,
    config: Arc<rustls::ClientConfig>,
    prepared: Option<Prepared>,
) -> tokio_rustls::Connect<PrefixIo<IO>> {
    let (prefix, connection) = match prepared {
        Some(prepared) => (prepared.wire, Some(prepared.connection)),
        None => (Vec::new(), None),
    };
    tokio_rustls::TlsConnector::from(config).connect_with(
        name,
        PrefixIo {
            inner: io,
            prefix,
            sent: 0,
        },
        move |session| {
            if let Some(connection) = connection {
                *session = connection;
            }
        },
    )
}

pub(super) struct PrefixIo<IO> {
    inner: IO,
    prefix: Vec<u8>,
    sent: usize,
}
impl<IO: AsyncWrite + Unpin> PrefixIo<IO> {
    fn poll_prefix(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        while self.sent < self.prefix.len() {
            let n = std::task::ready!(
                Pin::new(&mut self.inner).poll_write(cx, &self.prefix[self.sent..])
            )?;
            if n == 0 {
                return Poll::Ready(Err(io::ErrorKind::WriteZero.into()));
            }
            self.sent += n;
        }
        Poll::Ready(Ok(()))
    }
}
impl<IO: AsyncRead + AsyncWrite + Unpin> AsyncRead for PrefixIo<IO> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        std::task::ready!(self.poll_prefix(cx))?;
        Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}
impl<IO: AsyncWrite + Unpin> AsyncWrite for PrefixIo<IO> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        std::task::ready!(self.poll_prefix(cx))?;
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        std::task::ready!(self.poll_prefix(cx))?;
        Pin::new(&mut self.inner).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

pub(super) async fn public_get<IO>(
    stream: IO,
    host: &str,
    path: &str,
    limit: usize,
) -> io::Result<(u16, Vec<u8>)>
where
    IO: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    use http_body_util::{BodyExt, Empty};
    let (mut sender, connection) = hyper::client::conn::http1::Builder::new()
        .max_buf_size(16 * 1024)
        .handshake(hyper_util::rt::TokioIo::new(stream))
        .await
        .map_err(io::Error::other)?;
    struct Driver(tokio::task::JoinHandle<Result<(), hyper::Error>>);
    impl Drop for Driver {
        fn drop(&mut self) {
            self.0.abort();
        }
    }
    let _driver = Driver(tokio::spawn(connection));
    let request = hyper::Request::builder()
        .uri(path)
        .header("Host", host)
        .header("Connection", "close")
        .header("Cache-Control", "no-cache")
        .header("Accept-Encoding", "identity")
        .body(Empty::<hyper::body::Bytes>::new())
        .map_err(io::Error::other)?;
    let response = sender
        .send_request(request)
        .await
        .map_err(io::Error::other)?;
    let status = response.status().as_u16();
    let mut body = response.into_body();
    let mut bytes = Vec::new();
    while let Some(frame) = body.frame().await {
        if let Ok(data) = frame.map_err(io::Error::other)?.into_data() {
            if bytes.len() + data.len() > limit {
                return Err(io::Error::other("public TLS response exceeded byte limit"));
            }
            bytes.extend_from_slice(&data);
        }
    }
    Ok((status, bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fresh_layouts_have_new_keys_and_require_both_authenticated_responses() {
        let mut evidence = Evidence {
            policy: POLICY.into(),
            handshakes: vec![],
        };
        for kind in KINDS {
            let first = prepare("discord.com", config(), kind).unwrap();
            let second = prepare("discord.com", config(), kind).unwrap();
            assert_ne!(first.wire, second.wire);
            assert!(first.metadata.matches_layout());
            assert!(!first.metadata.authenticated_tls);
            evidence.handshakes.push(first.metadata);
        }
        assert!(!evidence.passed());
        for row in &mut evidence.handshakes {
            row.authenticated_tls = true;
            row.application_ok = true;
        }
        assert!(evidence.passed());
        let mut same = evidence.clone();
        same.handshakes[1] = same.handshakes[0].clone();
        assert!(!same.passed());
        let mut failed = evidence.clone();
        failed.handshakes[1].application_ok = false;
        assert!(!failed.passed());
        let mut small = evidence.clone();
        small.handshakes[0].handshake_bytes = 249;
        assert!(!small.passed());
        let mut missing = evidence;
        missing.handshakes.pop();
        assert!(!missing.passed());
    }
    #[test]
    fn truncated_hellos_and_small_ring_requests_cannot_pass() {
        let large = prepare("discord.com", config(), Kind::LargeEarlySni).unwrap();
        for end in [0, 4, 8, large.wire.len() - 1] {
            assert!(summarize(&large.wire[..end], "discord.com", Kind::LargeEarlySni).is_none());
        }
        let small = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_root_certificates(rustls::RootCertStore::empty())
        .with_no_client_auth();
        assert!(prepare("discord.com", Arc::new(small), Kind::LargeEarlySni).is_err());
    }
    #[tokio::test]
    async fn both_prepared_connections_finish_verified_tls_and_fetch_a_document() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let generated =
            rcgen::generate_simple_self_signed(vec!["discord.fixture.invalid".into()]).unwrap();
        let cert = generated.cert.der().clone();
        let key = rustls::pki_types::PrivatePkcs8KeyDer::from(generated.key_pair.serialize_der());
        let server = rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(vec![cert.clone()], key.into())
        .unwrap();
        let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(server));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let remote = listener.local_addr().unwrap();
        let peer = tokio::spawn(async move {
            for _ in KINDS {
                let (tcp, _) = listener.accept().await.unwrap();
                let mut tls = acceptor.accept(tcp).await.unwrap();
                assert_eq!(
                    tls.get_ref().1.server_name(),
                    Some("discord.fixture.invalid")
                );
                let mut request = [0; 2048];
                tls.read(&mut request).await.unwrap();
                tls.write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\ntest",
                )
                .await
                .unwrap();
                tls.shutdown().await.unwrap();
            }
        });
        let mut roots = rustls::RootCertStore::empty();
        roots.add(cert).unwrap();
        let trusted = config_with_roots(roots);
        for kind in KINDS {
            let prepared = prepare("discord.fixture.invalid", trusted.clone(), kind).unwrap();
            let tcp = tokio::net::TcpStream::connect(remote).await.unwrap();
            let name = rustls::pki_types::ServerName::try_from("discord.fixture.invalid").unwrap();
            let tls = connect(tcp, name, trusted.clone(), Some(prepared))
                .await
                .unwrap();
            assert_eq!(
                public_get(tls, "discord.fixture.invalid", "/", 4)
                    .await
                    .unwrap(),
                (200, b"test".to_vec())
            );
        }
        peer.await.unwrap();
    }
    #[tokio::test]
    async fn public_get_bounds_chunked_bodies_and_drops_its_owned_driver() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        for limit in [3, 4] {
            let (client, mut server) = tokio::io::duplex(4096);
            let peer = tokio::spawn(async move {
                let mut request = [0; 2048];
                server.read(&mut request).await.unwrap();
                server.write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\ntest\r\n0\r\n\r\n").await.unwrap();
                let mut tail = [0];
                server.read(&mut tail).await.unwrap_or(0)
            });
            let result = public_get(client, "discord.com", "/api/v10/gateway", limit).await;
            if limit == 3 {
                assert!(result.is_err());
            } else {
                assert_eq!(result.unwrap(), (200, b"test".to_vec()));
            }
            assert_eq!(
                tokio::time::timeout(std::time::Duration::from_secs(1), peer)
                    .await
                    .unwrap()
                    .unwrap(),
                0
            );
        }
    }
}
