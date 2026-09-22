//! Bounded application-data checks. Never retain response contents or credentials.

use std::{net::SocketAddr, sync::Arc};

use bytes::Buf;
use tokio::time::{timeout_at, Instant};

pub(super) const BODY_SAMPLE_LIMIT: usize = 256 * 1024;

#[derive(Debug)]
pub(super) struct HttpEvidence {
    pub status: u16,
    pub bytes: usize,
    pub complete: bool,
}

impl HttpEvidence {
    pub fn detail(&self, protocol: &str) -> String {
        format!(
            "{protocol} {}; body {} bytes ({})",
            self.status,
            self.bytes,
            if self.complete {
                "complete"
            } else {
                "sample limit"
            }
        )
    }
}

pub(super) async fn read_http_body(
    mut response: reqwest::Response,
    deadline: Instant,
) -> Result<HttpEvidence, String> {
    let mut evidence = HttpEvidence {
        status: response.status().as_u16(),
        bytes: 0,
        complete: false,
    };
    while evidence.bytes < BODY_SAMPLE_LIMIT {
        let chunk = timeout_at(deadline, response.chunk())
            .await
            .map_err(|_| format!("body timeout after {} bytes", evidence.bytes))?
            .map_err(|error| format!("body interrupted after {} bytes: {error}", evidence.bytes))?;
        match chunk {
            Some(chunk) => evidence.bytes += chunk.len().min(BODY_SAMPLE_LIMIT - evidence.bytes),
            None => {
                evidence.complete = true;
                break;
            }
        }
    }
    Ok(evidence)
}

#[derive(Debug)]
pub(super) struct QuicFailure {
    pub established: bool,
    pub detail: String,
}

impl QuicFailure {
    fn connect(error: impl std::fmt::Display) -> Self {
        Self {
            established: false,
            detail: error.to_string(),
        }
    }
    fn http(error: impl std::fmt::Display) -> Self {
        Self {
            established: true,
            detail: error.to_string(),
        }
    }
}

pub(super) fn quic_config(
    roots: rustls::RootCertStore,
) -> Result<quinn::ClientConfig, QuicFailure> {
    let mut crypto = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    crypto.alpn_protocols = vec![b"h3".to_vec()];
    let crypto =
        quinn::crypto::rustls::QuicClientConfig::try_from(crypto).map_err(QuicFailure::connect)?;
    Ok(quinn::ClientConfig::new(Arc::new(crypto)))
}

pub(super) async fn probe_http3(
    addresses: &[SocketAddr],
    server_name: &str,
    path: &str,
    deadline: Instant,
) -> Result<HttpEvidence, QuicFailure> {
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let config = quic_config(roots)?;
    let mut attempts = tokio::task::JoinSet::new();
    for (index, address) in addresses
        .iter()
        .find(|a| a.is_ipv6())
        .into_iter()
        .chain(addresses.iter().find(|a| a.is_ipv4()))
        .copied()
        .enumerate()
    {
        let config = config.clone();
        let server_name = server_name.to_owned();
        let path = path.to_owned();
        attempts.spawn(async move {
            if index > 0 {
                timeout_at(
                    deadline,
                    tokio::time::sleep(std::time::Duration::from_millis(150)),
                )
                .await
                .map_err(|_| QuicFailure::connect("address attempt budget exhausted"))?;
            }
            probe_http3_address(address, &server_name, &path, deadline, config).await
        });
    }
    let mut last_error = QuicFailure::connect("no usable addresses");
    while let Some(result) = attempts.join_next().await {
        match result {
            Ok(Ok(evidence)) => {
                attempts.abort_all();
                return Ok(evidence);
            }
            Ok(Err(error)) => {
                // Preserve post-handshake failures over a failed parallel IP route.
                if error.established || !last_error.established {
                    last_error = error;
                }
            }
            Err(error) => {
                if !last_error.established {
                    last_error = QuicFailure::connect(error);
                }
            }
        }
    }
    Err(last_error)
}

async fn probe_http3_address(
    address: SocketAddr,
    server_name: &str,
    path: &str,
    deadline: Instant,
    config: quinn::ClientConfig,
) -> Result<HttpEvidence, QuicFailure> {
    let bind = if address.is_ipv4() {
        "0.0.0.0:0"
    } else {
        "[::]:0"
    };
    let mut endpoint =
        quinn::Endpoint::client(bind.parse().unwrap()).map_err(QuicFailure::connect)?;
    endpoint.set_default_client_config(config);
    let connecting = endpoint
        .connect(address, server_name)
        .map_err(QuicFailure::connect)?;
    let connection = timeout_at(deadline, connecting)
        .await
        .map_err(|_| QuicFailure::connect("QUIC handshake timeout"))?
        .map_err(QuicFailure::connect)?;
    let response = timeout_at(
        deadline,
        exchange_http3(connection.clone(), server_name, address.port(), path),
    )
    .await
    .map_err(|_| QuicFailure::http("HTTP/3 response/body timeout"))?;
    connection.close(0u32.into(), b"probe complete");
    response.map_err(QuicFailure::http)
}

async fn exchange_http3(
    connection: quinn::Connection,
    server_name: &str,
    port: u16,
    path: &str,
) -> Result<HttpEvidence, String> {
    let (mut driver, mut sender) = h3::client::new(h3_quinn::Connection::new(connection))
        .await
        .map_err(|error| error.to_string())?;
    let request = async move {
        let uri = format!("https://{server_name}:{port}{path}");
        let request = http::Request::get(uri)
            .body(())
            .map_err(|error| error.to_string())?;
        let mut stream = sender
            .send_request(request)
            .await
            .map_err(|error| error.to_string())?;
        stream.finish().await.map_err(|error| error.to_string())?;
        let response = stream
            .recv_response()
            .await
            .map_err(|error| error.to_string())?;
        let mut evidence = HttpEvidence {
            status: response.status().as_u16(),
            bytes: 0,
            complete: false,
        };
        while evidence.bytes < BODY_SAMPLE_LIMIT {
            match stream
                .recv_data()
                .await
                .map_err(|error| error.to_string())?
            {
                Some(chunk) => {
                    evidence.bytes += chunk.remaining().min(BODY_SAMPLE_LIMIT - evidence.bytes)
                }
                None => {
                    evidence.complete = true;
                    break;
                }
            }
        }
        Ok(evidence)
    };
    // Both futures are dropped on cancellation; no detached driver survives a probe.
    tokio::select! {
        biased;
        result = request => result,
        error = std::future::poll_fn(|cx| driver.poll_close(cx)) => Err(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    async fn http_response(
        actual: usize,
        declared: usize,
        stall: bool,
    ) -> (reqwest::Response, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let worker = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0; 2048];
            socket.read(&mut request).await.unwrap();
            let headers = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {declared}\r\nConnection: close\r\n\r\n"
            );
            socket.write_all(headers.as_bytes()).await.unwrap();
            let _ = socket.write_all(&vec![b'x'; actual]).await;
            if stall {
                std::future::pending::<()>().await;
            }
        });
        let response = reqwest::Client::builder()
            .no_proxy()
            .build()
            .unwrap()
            .get(format!("http://{address}/"))
            .send()
            .await
            .unwrap();
        (response, worker)
    }

    #[tokio::test]
    async fn reads_complete_body_past_the_first_16k() {
        let (response, worker) = http_response(65536, 65536, false).await;
        let evidence = read_http_body(response, Instant::now() + Duration::from_secs(2))
            .await
            .unwrap();
        assert_eq!(evidence.bytes, 65536);
        assert!(evidence.complete);
        worker.await.unwrap();
    }

    #[tokio::test]
    async fn headers_and_16k_do_not_hide_truncation_or_stall() {
        for stall in [false, true] {
            let (response, worker) = http_response(16384, 65536, stall).await;
            let result =
                read_http_body(response, Instant::now() + Duration::from_millis(150)).await;
            worker.abort();
            assert!(result.is_err(), "incomplete body must fail, stall={stall}");
        }
    }

    #[tokio::test]
    async fn large_response_is_bounded_and_reported_as_a_sample() {
        let (response, worker) =
            http_response(BODY_SAMPLE_LIMIT * 2, BODY_SAMPLE_LIMIT * 2, false).await;
        let evidence = read_http_body(response, Instant::now() + Duration::from_secs(2))
            .await
            .unwrap();
        assert_eq!(evidence.bytes, BODY_SAMPLE_LIMIT);
        assert!(!evidence.complete);
        worker.abort();
    }

    fn local_quic() -> (quinn::Endpoint, quinn::ClientConfig) {
        let cert = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
        let key = rustls::pki_types::PrivatePkcs8KeyDer::from(cert.key_pair.serialize_der());
        let mut tls = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(vec![cert.cert.der().clone()], key.into())
            .unwrap();
        tls.alpn_protocols = vec![b"h3".to_vec()];
        let server = quinn::ServerConfig::with_crypto(Arc::new(
            quinn::crypto::rustls::QuicServerConfig::try_from(tls).unwrap(),
        ));
        let endpoint = quinn::Endpoint::server(server, "127.0.0.1:0".parse().unwrap()).unwrap();
        let mut roots = rustls::RootCertStore::empty();
        roots.add(cert.cert.der().clone()).unwrap();
        (endpoint, quic_config(roots).unwrap())
    }

    #[tokio::test]
    async fn http3_requires_request_response_and_body_after_quic_handshake() {
        let (endpoint, client) = local_quic();
        let address = endpoint.local_addr().unwrap();
        let server_endpoint = endpoint.clone();
        let server = tokio::spawn(async move {
            let connection = server_endpoint.accept().await.unwrap().await.unwrap();
            let mut server =
                h3::server::Connection::new(h3_quinn::Connection::new(connection.clone()))
                    .await
                    .unwrap();
            let resolver = server.accept().await.unwrap().unwrap();
            let (request, mut stream) = resolver.resolve_request().await.unwrap();
            assert_eq!(request.uri().path(), "/payload");
            stream
                .send_response(http::Response::builder().status(200).body(()).unwrap())
                .await
                .unwrap();
            stream
                .send_data(bytes::Bytes::from(vec![b'x'; 65536]))
                .await
                .unwrap();
            stream.finish().await.unwrap();
            connection.closed().await;
        });
        let result = probe_http3_address(
            address,
            "localhost",
            "/payload",
            Instant::now() + Duration::from_secs(3),
            client,
        )
        .await;
        server.abort();
        let evidence = result.unwrap();
        assert_eq!(evidence.status, 200);
        assert_eq!(evidence.bytes, 65536);
        assert!(evidence.complete);
    }

    #[tokio::test]
    async fn quic_handshake_without_http3_is_not_a_success() {
        let (endpoint, client) = local_quic();
        let address = endpoint.local_addr().unwrap();
        let server_endpoint = endpoint.clone();
        let server = tokio::spawn(async move {
            let connection = server_endpoint.accept().await.unwrap().await.unwrap();
            connection.closed().await;
        });
        let result = probe_http3_address(
            address,
            "localhost",
            "/",
            Instant::now() + Duration::from_millis(300),
            client,
        )
        .await;
        server.abort();
        let error = result.unwrap_err();
        assert!(error.established);
        assert!(error.detail.contains("timeout"));
    }

    #[tokio::test]
    async fn http3_body_stall_is_a_failure_after_successful_handshake_and_headers() {
        let (endpoint, client) = local_quic();
        let address = endpoint.local_addr().unwrap();
        let server_endpoint = endpoint.clone();
        let server = tokio::spawn(async move {
            let connection = server_endpoint.accept().await.unwrap().await.unwrap();
            let mut server =
                h3::server::Connection::new(h3_quinn::Connection::new(connection.clone()))
                    .await
                    .unwrap();
            let (_, mut stream) = server
                .accept()
                .await
                .unwrap()
                .unwrap()
                .resolve_request()
                .await
                .unwrap();
            stream
                .send_response(http::Response::builder().status(200).body(()).unwrap())
                .await
                .unwrap();
            stream
                .send_data(bytes::Bytes::from(vec![b'x'; 16384]))
                .await
                .unwrap();
            connection.closed().await;
        });
        let result = probe_http3_address(
            address,
            "localhost",
            "/",
            Instant::now() + Duration::from_millis(300),
            client,
        )
        .await;
        server.abort();
        assert!(result.unwrap_err().established);
    }
}
