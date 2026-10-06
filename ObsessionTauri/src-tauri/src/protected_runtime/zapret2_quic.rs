//! Fixed, public YouTube QUIC connectivity checks. Certificate and TLS Finished
//! are verified; no HTTP/3 request, video, account or voice traffic is sent.
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant};

use quinn::crypto::rustls::QuicClientConfig;
use tauri::AppHandle;

use super::legacy_test::{self, ProbeResult, TestReport};

const TARGETS: [&str; 2] = ["www.youtube.com", "i.ytimg.com"];

fn client_config(roots: rustls::RootCertStore) -> Result<quinn::ClientConfig, String> {
    let mut tls = rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_protocol_versions(&[&rustls::version::TLS13]).map_err(|error| error.to_string())?
        .with_root_certificates(roots).with_no_client_auth();
    tls.alpn_protocols = vec![b"h3".to_vec()];
    Ok(quinn::ClientConfig::new(Arc::new(QuicClientConfig::try_from(tls).map_err(|error| error.to_string())?)))
}

fn bounded_addresses(addresses: impl IntoIterator<Item = SocketAddr>) -> Vec<SocketAddr> {
    let mut ipv4 = None;
    let mut ipv6 = None;
    for address in addresses {
        if address.port() != 443 || address.ip().is_unspecified() || address.ip().is_multicast() { continue; }
        match address.ip() {
            IpAddr::V4(ip) if !ip.is_broadcast() && ipv4.is_none() => ipv4 = Some(address),
            IpAddr::V6(_) if ipv6.is_none() => ipv6 = Some(address),
            _ => {}
        }
    }
    ipv4.into_iter().chain(ipv6).collect()
}

// Abort of a task runs Drop before JoinSet is drained, so endpoints of a
// previous profile cannot keep sending while the next profile is installed.
struct ClosingEndpoint(quinn::Endpoint);
impl Drop for ClosingEndpoint {
    fn drop(&mut self) { self.0.close(0u32.into(), b"local connectivity check ended"); }
}

async fn connect(address: SocketAddr, host: &str, config: quinn::ClientConfig,
    deadline: tokio::time::Instant) -> Result<(), String> {
    let bind = if address.is_ipv4() { SocketAddr::new(Ipv4Addr::UNSPECIFIED.into(), 0) }
        else { SocketAddr::new(Ipv6Addr::UNSPECIFIED.into(), 0) };
    let endpoint = ClosingEndpoint(quinn::Endpoint::client(bind).map_err(|error| format!("QUIC UDP: {error}"))?);
    let connecting = endpoint.0.connect_with(config, address, host).map_err(|error| format!("QUIC: {error}"))?;
    let connection = tokio::time::timeout_at(deadline, connecting).await
        .map_err(|_| "Истёк срок QUIC handshake.".to_owned())?
        .map_err(|error| format!("QUIC TLS/certificate: {error}"))?;
    connection.close(0u32.into(), b"authenticated handshake complete");
    Ok(())
}

async fn target(host: &'static str, config: quinn::ClientConfig, deadline: tokio::time::Instant) -> ProbeResult {
    let started = Instant::now();
    let mut check = ProbeResult { url: format!("quic://{host}:443"), passed: false, elapsed_ms: 0,
        bytes: 0, attempts: 0, http_status: None, error: None };
    let addresses = match tokio::time::timeout_at(deadline, tokio::net::lookup_host((host, 443))).await {
        Ok(Ok(addresses)) => bounded_addresses(addresses),
        Ok(Err(error)) => { check.error = Some(format!("Системный DNS: {error}")); Vec::new() }
        Err(_) => { check.error = Some("Истёк общий срок проверки при системном DNS.".into()); Vec::new() }
    };
    if addresses.is_empty() && check.error.is_none() { check.error = Some("DNS не вернул допустимые QUIC адреса.".into()); }
    for (index, address) in addresses.iter().enumerate() {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() { break; }
        let attempt_deadline = tokio::time::Instant::now() + remaining / (addresses.len() - index) as u32;
        check.attempts += 1;
        match connect(*address, host, config.clone(), attempt_deadline).await {
            Ok(()) => { check.passed = true; check.error = None; break; }
            Err(error) => check.error = Some(error),
        }
    }
    check.elapsed_ms = started.elapsed().as_millis().min(u64::MAX as u128) as u64;
    check
}

pub(super) async fn probe(app: &AppHandle, category: &str, epoch: u64, timeout: Duration) -> Result<TestReport, String> {
    if category != "youtube_twitch" { return Err("Проверка QUIC доступна только для YouTube; Gaming QUIC выбирается вручную.".into()); }
    let started = Instant::now();
    let deadline = tokio::time::Instant::now() + timeout.min(Duration::from_secs(15));
    let config = client_config(rustls::RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned()))?;
    let mut tasks = tokio::task::JoinSet::new();
    for (index, host) in TARGETS.iter().enumerate() {
        let config = config.clone();
        let host = *host;
        tasks.spawn(async move { (index, target(host, config, deadline).await) });
    }
    let mut checks: Vec<Option<ProbeResult>> = (0..TARGETS.len()).map(|_| None).collect();
    let mut was_cancelled = false;
    while !tasks.is_empty() {
        if legacy_test::cancelled(app, epoch) { was_cancelled = true; break; }
        tokio::select! {
            biased;
            _ = tokio::time::sleep_until(deadline) => break,
            _ = tokio::time::sleep(Duration::from_millis(100)) => {},
            joined = tasks.join_next() => {
                let Some(Ok((index, check))) = joined else { break; };
                checks[index] = Some(check);
            }
        }
    }
    tasks.abort_all();
    while tasks.join_next().await.is_some() {}
    let checks = checks.into_iter().enumerate().map(|(index, check)| check.unwrap_or_else(|| ProbeResult {
        url: format!("quic://{}:443", TARGETS[index]), passed: false,
        elapsed_ms: started.elapsed().as_millis().min(u64::MAX as u128) as u64, bytes: 0,
        attempts: 0, http_status: None, error: Some(if was_cancelled { "Проверка отменена." } else { "Истёк общий срок QUIC handshake." }.into()),
    })).collect();
    Ok(legacy_test::report(checks, was_cancelled))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_at_most_one_unicast_address_per_family() {
        let addresses = ["0.0.0.0:443","224.0.0.1:443","255.255.255.255:443","[::]:443","[ff02::1]:443",
            "192.0.2.1:444","192.0.2.1:443","192.0.2.2:443","[2001:db8::1]:443","[2001:db8::2]:443"]
            .map(|value| value.parse::<SocketAddr>().unwrap());
        assert_eq!(bounded_addresses(addresses),vec!["192.0.2.1:443".parse().unwrap(),"[2001:db8::1]:443".parse().unwrap()]);
    }

    #[test]
    fn explicit_ring_tls13_config_builds_with_public_roots() {
        assert!(client_config(rustls::RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned())).is_ok());
        assert_eq!(TARGETS,["www.youtube.com","i.ytimg.com"]);
    }

    #[tokio::test]
    async fn localhost_quic_requires_the_correct_certificate_and_name() {
        let generated = rcgen::generate_simple_self_signed(vec!["quic-fixture.test".into()]).unwrap();
        let cert = generated.cert.der().clone();
        let key = rustls::pki_types::PrivatePkcs8KeyDer::from(generated.key_pair.serialize_der());
        let mut tls = rustls::ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_protocol_versions(&[&rustls::version::TLS13]).unwrap().with_no_client_auth()
            .with_single_cert(vec![cert.clone()],key.into()).unwrap();
        tls.alpn_protocols = vec![b"h3".to_vec()];
        let server_config = quinn::ServerConfig::with_crypto(Arc::new(quinn::crypto::rustls::QuicServerConfig::try_from(tls).unwrap()));
        let server = quinn::Endpoint::server(server_config, "127.0.0.1:0".parse().unwrap()).unwrap();
        let address = server.local_addr().unwrap();
        let server_endpoint = server.clone();
        let listener = tokio::spawn(async move {
            let mut tasks = tokio::task::JoinSet::new();
            for _ in 0..3 {
                let incoming = tokio::time::timeout(Duration::from_secs(2),server_endpoint.accept()).await.unwrap().unwrap();
                tasks.spawn(async move {
                    if let Ok(connection) = incoming.await { connection.closed().await; }
                });
            }
            while tasks.join_next().await.is_some() {}
        });
        let mut roots = rustls::RootCertStore::empty();
        roots.add(cert).unwrap();
        let trusted = client_config(roots).unwrap();
        assert!(connect(address,"quic-fixture.test",trusted.clone(),tokio::time::Instant::now()+Duration::from_secs(2)).await.is_ok());
        assert!(connect(address,"wrong-name.test",trusted,tokio::time::Instant::now()+Duration::from_secs(2)).await.is_err());
        assert!(connect(address,"quic-fixture.test",client_config(rustls::RootCertStore::empty()).unwrap(),
            tokio::time::Instant::now()+Duration::from_secs(2)).await.is_err());
        server.close(0u32.into(),b"fixture complete");
        tokio::time::timeout(Duration::from_secs(3),listener).await.unwrap().unwrap();
    }
}
