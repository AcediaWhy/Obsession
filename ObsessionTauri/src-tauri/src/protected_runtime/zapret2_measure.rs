//! Bounded application probes. A service start receipt is not proof that DPI
//! matched a flow; that limitation is retained in every measurement.
use super::legacy_test::{ProbeResult, TestReport};
use super::*;
use futures_util::{SinkExt, StreamExt};
use std::net::SocketAddr;
use std::time::Instant;

pub const TESTER_VERSION: &str = "zapret2-functions-v5-routing";
pub const MAX_ROUND_BYTES: usize = 2 * 1024 * 1024;
pub(super) const TRANSFER_BYTES: usize = 512 * 1024;
const MIN_TRANSFER_BYTES: usize = 256 * 1024;
const MIN_TRANSFER_BYTES_PER_SEC: u64 = 256 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Passed,
    Failed,
    Inconclusive,
    NotTested,
    Cancelled,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FunctionEvidence {
    pub function: String,
    pub label: String,
    pub required: bool,
    pub outcome: Outcome,
    pub proof: &'static str,
    pub stage: &'static str,
    pub reason: Option<String>,
    pub target: String,
    pub remote_address: Option<String>,
    pub ip_family: Option<&'static str>,
    pub elapsed_ms: u64,
    pub bytes: usize,
    pub transfer_bytes: usize,
    pub transfer_ms: Option<u64>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tls_handshakes: Vec<super::zapret2_tls::Metadata>,
    pub http_status: Option<u16>,
    pub(super) connections: Vec<super::zapret2_routing::ConnectionEvidence>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoundMeasurement {
    pub run_id: String,
    pub profile_id: String,
    pub fingerprint: String,
    pub runtime_generation: Option<u64>,
    pub ordinal: usize,
    pub phase: String,
    pub transport: String,
    pub tester_version: &'static str,
    pub execution: &'static str,
    pub network_before: Option<String>,
    pub network_after: Option<String>,
    pub comparable: bool,
    pub context_reason: Option<&'static str>,
    pub functions: Vec<FunctionEvidence>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub packet_capture: Option<obsession_runtime_protocol::DpiPacketCapture>,
}

impl RoundMeasurement {
    pub fn complete(&self) -> bool {
        self.runtime_generation.is_some()
            && (self.transport != "tls"
                || !matches!(
                    self.phase.as_str(),
                    "screening" | "confirmation" | "composition"
                )
                || (self.execution == "flow_routing_verified"
                    && self
                        .functions
                        .iter()
                        .filter(|f| f.required)
                        .all(FunctionEvidence::routing_verified)))
            && self.comparable
            && self.functions.iter().any(|f| f.required)
            && self
                .functions
                .iter()
                .filter(|f| f.required)
                .all(FunctionEvidence::passed)
    }
    pub fn bytes(&self) -> usize {
        self.functions.iter().map(|f| f.bytes).sum()
    }
}

impl FunctionEvidence {
    pub(super) fn routing_verified(&self) -> bool {
        !self.connections.is_empty()
            && self
                .connections
                .iter()
                .all(|connection| connection.status == "verified")
    }
    pub(super) fn passed(&self) -> bool {
        self.outcome == Outcome::Passed
            && self.proof != "none"
            && (!matches!(self.function.as_str(), "api_large_tls" | "cdn_large_tls")
                || (self.proof == "large_tls_application"
                    && super::zapret2_tls::Evidence {
                        policy: super::zapret2_tls::POLICY.into(),
                        handshakes: self.tls_handshakes.clone(),
                    }
                    .passed()))
    }
}

#[derive(Clone, Copy)]
pub(super) struct Target {
    pub function: &'static str,
    label: &'static str,
    url: &'static str,
    kind: &'static str,
    required: bool,
    limit: usize,
}

fn targets(category: &str) -> Result<Vec<Target>, String> {
    let target = |function, label, url, kind, required, limit| Target {
        function,
        label,
        url,
        kind,
        required,
        limit,
    };
    Ok(match category {
        "discord" => vec![
            target("api_large_tls", "API · большой TLS, две формы SNI", "https://discord.com/api/v10/gateway", "large_gateway_api", true, 32 * 1024),
            target("cdn_large_tls", "CDN · большой TLS, две формы SNI", "https://cdn.discordapp.com/embed/avatars/0.png", "large_png", true, 32 * 1024),
            target("api", "REST API", "https://discord.com/api/v10/gateway", "gateway_api", true, 16 * 1024),
            target("gateway", "Gateway WSS Hello", "wss://gateway.discord.gg/?v=10&encoding=json", "wss", true, 16 * 1024),
            target("cdn", "Картинка CDN", "https://cdn.discordapp.com/embed/avatars/0.png", "png", true, 256 * 1024),
            target("client_assets", "Загрузка файлов Discord", "https://discord.com/app", "discord_assets", true, 256 * 1024),
            target("updater", "Манифест обновления", "https://updates.discord.com/distributions/app/manifests/latest?channel=stable&platform=win&arch=x64", "updater", false, 128 * 1024),
            target("site", "Веб-страница", "https://discord.com/", "html", false, 32 * 1024),
        ],
        "youtube_twitch" => vec![
            target("site", "Страница YouTube", "https://www.youtube.com/", "youtube", true, 128 * 1024),
            target("images", "Картинка YouTube CDN", "https://i.ytimg.com/vi/jNQXAC9IVRw/hqdefault.jpg", "jpeg", true, 256 * 1024),
            target("player_assets", "Загрузка кода плеера", "https://www.youtube.com/watch?v=jNQXAC9IVRw", "youtube_assets", true, 256 * 1024),
        ],
        "gaming" => vec![
            target("epic", "Страница Epic", "https://www.epicgames.com/", "html", true, 32 * 1024),
            target("ea", "Страница EA", "https://signin.ea.com/", "html", true, 32 * 1024),
        ],
        _ => return Err("Неизвестный сценарий измерения Zapret2.".into()),
    })
}

fn pending(target: Target, outcome: Outcome, reason: &str) -> FunctionEvidence {
    FunctionEvidence {
        function: target.function.into(),
        label: target.label.into(),
        required: target.required,
        outcome,
        proof: "none",
        stage: "task",
        reason: Some(reason.into()),
        target: target.url.into(),
        remote_address: None,
        ip_family: None,
        elapsed_ms: 0,
        bytes: 0,
        transfer_bytes: 0,
        transfer_ms: None,
        tls_handshakes: vec![],
        http_status: None,
        connections: vec![],
    }
}

fn address(evidence: &mut FunctionEvidence, remote: SocketAddr) {
    evidence.remote_address = Some(remote.to_string());
    evidence.ip_family = Some(if remote.is_ipv4() { "ipv4" } else { "ipv6" });
}

fn validate_body(kind: &str, body: &[u8], complete: bool) -> Result<(), &'static str> {
    let good = match kind {
        "gateway_api" => {
            complete
                && crate::service_health::validate_discord_body("discord.com", 200, body).is_ok()
        }
        "updater" => {
            complete
                && crate::service_health::validate_discord_body("updates.discord.com", 200, body)
                    .is_ok()
        }
        "png" => {
            complete && body.starts_with(b"\x89PNG\r\n\x1a\n") && body.ends_with(b"IEND\xaeB`\x82")
        }
        "jpeg" => {
            complete && body.starts_with(&[0xff, 0xd8, 0xff]) && body.ends_with(&[0xff, 0xd9])
        }
        "youtube" => body
            .windows(10)
            .any(|v| v == b"ytcfg.set(" || v == b"ytInitialD"),
        "html" => {
            let text = String::from_utf8_lossy(body).to_ascii_lowercase();
            text.contains("<html") || text.contains("<!doctype html")
        }
        _ => false,
    };
    if good {
        Ok(())
    } else {
        Err("Цель вернула неожидаемый документ; пригодность пробы не подтверждена.")
    }
}

fn http_outcome(status: u16) -> Outcome {
    // Errors from the service, rate limits and unsuitable test content cannot
    // establish failure of a DPI mechanism.
    if (200..300).contains(&status) {
        Outcome::Passed
    } else {
        Outcome::Inconclusive
    }
}

fn tls_setup_error(error: &(dyn std::error::Error + 'static)) -> bool {
    let mut cause = Some(error);
    for _ in 0..16 {
        let Some(error) = cause else {
            break;
        };
        if error.downcast_ref::<rustls::Error>().is_some() {
            return true;
        }
        // Rustls may be wrapped in io::Error without a direct source().
        if let Some(error) = error.downcast_ref::<std::io::Error>() {
            if error
                .get_ref()
                .is_some_and(|inner| inner.downcast_ref::<rustls::Error>().is_some())
            {
                return true;
            }
        }
        cause = error.source();
    }
    false
}

#[derive(Clone)]
struct ProbeProgress(std::sync::Arc<Mutex<(FunctionEvidence, Option<Instant>)>>);
impl ProbeProgress {
    fn new(target: Target) -> Self {
        let mut result = pending(target, Outcome::NotTested, "Проба ещё не началась.");
        result.stage = "queued";
        Self(std::sync::Arc::new(Mutex::new((result, None))))
    }
    fn start(&self) {
        self.0.lock_recover().1 = Some(Instant::now());
    }
    fn elapsed_ms(&self) -> u64 {
        self.0
            .lock_recover()
            .1
            .map(|started| started.elapsed().as_millis() as u64)
            .unwrap_or(0)
    }
    fn update(&self, result: &FunctionEvidence) {
        self.0.lock_recover().0 = result.clone();
    }
    fn incomplete(&self, cancelled: bool, required_complete: bool) -> FunctionEvidence {
        let (mut result, started) = self.0.lock_recover().clone();
        result.outcome = if cancelled {
            Outcome::Cancelled
        } else if started.is_some() {
            Outcome::Inconclusive
        } else {
            Outcome::NotTested
        };
        result.proof = "none";
        result.reason = Some(
            if cancelled {
                "Проверка отменена."
            } else if required_complete {
                "Обязательные пробы завершены; ожидание вспомогательной пробы прекращено."
            } else if started.is_some() {
                "Проба не завершилась в пределах срока раунда; сохранён последний наблюдаемый этап."
            } else {
                "Проба не успела начаться в пределах срока раунда."
            }
            .into(),
        );
        result.elapsed_ms = started
            .map(|started| started.elapsed().as_millis() as u64)
            .unwrap_or(0);
        for connection in &mut result.connections {
            if let Some(http) = &mut connection.http {
                if http.stage != "complete" {
                    http.elapsed_ms = result.elapsed_ms.saturating_sub(http.started_after_ms);
                }
            }
        }
        result
    }
}

// Keep address attempts in this future: cancelling a probe drops every owned
// socket immediately, without detached resolver/connect tasks of our own.
async fn connect_addresses(
    mut addresses: Vec<SocketAddr>,
) -> std::io::Result<tokio::net::TcpStream> {
    addresses.sort_unstable();
    addresses.dedup();
    let mut attempts = futures_util::stream::FuturesUnordered::new();
    for (index, remote) in addresses.into_iter().take(8).enumerate() {
        attempts.push(async move {
            tokio::time::sleep(Duration::from_millis(index as u64 * 80)).await;
            tokio::time::timeout(
                Duration::from_millis(1200),
                tokio::net::TcpStream::connect(remote),
            )
            .await
            .unwrap_or_else(|_| {
                Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    format!("TCP {remote}: timeout"),
                ))
            })
        });
    }
    let mut last = std::io::Error::new(
        std::io::ErrorKind::AddrNotAvailable,
        "DNS returned no usable address",
    );
    while let Some(result) = attempts.next().await {
        match result {
            Ok(stream) => return Ok(stream),
            Err(error) => last = error,
        }
    }
    Err(last)
}

async fn connect_target(
    host: &str,
    result: &mut FunctionEvidence,
    progress: &ProbeProgress,
) -> Option<tokio::net::TcpStream> {
    result.stage = "dns";
    progress.update(result);
    let addresses = match tokio::net::lookup_host((host, 443)).await {
        Ok(values) => values.collect(),
        Err(_) => {
            result.reason = Some("Системный DNS не предоставил адрес цели.".into());
            return None;
        }
    };
    result.stage = "connect";
    progress.update(result);
    match connect_addresses(addresses).await {
        Ok(stream) => {
            if let Ok(remote) = stream.peer_addr() {
                address(result, remote);
            }
            progress.update(result);
            Some(stream)
        }
        Err(error) => {
            result.reason = Some(format!(
                "TCP-соединение с адресами цели не установлено: {error}"
            ));
            None
        }
    }
}

fn fresh_http_tls(roots: rustls::RootCertStore) -> std::sync::Arc<rustls::ClientConfig> {
    let mut tls = rustls::ClientConfig::builder_with_provider(std::sync::Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .expect("supported TLS")
    .with_root_certificates(roots)
    .with_no_client_auth();
    tls.resumption = rustls::client::Resumption::disabled();
    tls.alpn_protocols = vec![b"http/1.1".to_vec()];
    std::sync::Arc::new(tls)
}

struct HttpSample {
    status: u16,
    body: Vec<u8>,
    complete: bool,
    content_type: String,
}

fn http_connection_stage(
    result: &mut FunctionEvidence,
    progress: &ProbeProgress,
    index: usize,
    stage: &'static str,
) {
    let http = result.connections[index]
        .http
        .as_mut()
        .expect("owned HTTP socket");
    http.stage = stage;
    http.elapsed_ms = progress.elapsed_ms().saturating_sub(http.started_after_ms);
    if stage == "http_headers" {
        http.tls_ms = Some(http.elapsed_ms);
    }
    progress.update(result);
}
// Own the socket used for the request. A preliminary TCP connection followed
// by reqwest's unrelated socket cannot prove which engine block handled HTTP.
async fn owned_get(
    url: &reqwest::Url,
    pinned: Option<SocketAddr>,
    limit: usize,
    range: bool,
    purpose: &str,
    result: &mut FunctionEvidence,
    progress: &ProbeProgress,
) -> std::io::Result<HttpSample> {
    let roots = rustls::RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    owned_get_with_config(
        url,
        pinned,
        limit,
        range,
        purpose,
        result,
        progress,
        fresh_http_tls(roots),
    )
    .await
}
async fn owned_get_with_config(
    url: &reqwest::Url,
    pinned: Option<SocketAddr>,
    limit: usize,
    range: bool,
    purpose: &str,
    result: &mut FunctionEvidence,
    progress: &ProbeProgress,
    config: std::sync::Arc<rustls::ClientConfig>,
) -> std::io::Result<HttpSample> {
    use http_body_util::{BodyExt, Empty};
    // A second request is a new TLS exchange, even when pinned to the same IP.
    // Leave discovery's status on its own connection, not on the asset result.
    result.http_status = None;
    let host = url
        .host_str()
        .ok_or_else(|| std::io::Error::other("missing fixed host"))?;
    let stream = if let Some(remote) = pinned {
        result.stage = "connect";
        progress.update(result);
        tokio::time::timeout(
            Duration::from_secs(2),
            tokio::net::TcpStream::connect(remote),
        )
        .await
        .map_err(|_| std::io::Error::other("pinned TCP timeout"))??
    } else {
        connect_target(host, result, progress)
            .await
            .ok_or_else(|| std::io::Error::other("DNS/TCP unavailable"))?
    };
    address(result, stream.peer_addr()?);
    result
        .connections
        .push(super::zapret2_routing::ConnectionEvidence::new(
            &stream, host, purpose,
        )?);
    let connection_index = result.connections.len() - 1;
    result.connections[connection_index].http =
        Some(super::zapret2_routing::HttpConnectionProgress {
            stage: "tls",
            started_after_ms: progress.elapsed_ms(),
            elapsed_ms: 0,
            tls_ms: None,
            http_status: None,
            bytes: 0,
        });
    result.stage = "tls";
    http_connection_stage(result, progress, connection_index, "tls");
    let name =
        rustls::pki_types::ServerName::try_from(host.to_owned()).map_err(std::io::Error::other)?;
    let tls = tokio_rustls::TlsConnector::from(config)
        .connect(name, stream)
        .await?;
    result.stage = "http_headers";
    http_connection_stage(result, progress, connection_index, "http_headers");
    let (mut sender, connection) = hyper::client::conn::http1::Builder::new()
        .max_buf_size(16 * 1024)
        .handshake(hyper_util::rt::TokioIo::new(tls))
        .await
        .map_err(std::io::Error::other)?;
    struct Driver(tokio::task::JoinHandle<Result<(), hyper::Error>>);
    impl Drop for Driver {
        fn drop(&mut self) {
            self.0.abort();
        }
    }
    let _driver = Driver(tokio::spawn(connection));
    let path = match url.query() {
        Some(query) => format!("{}?{query}", url.path()),
        None => url.path().to_owned(),
    };
    let authority = match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_owned(),
    };
    let mut request = hyper::Request::builder()
        .uri(path)
        .header("Host", authority)
        .header("Connection", "close")
        .header("Cache-Control", "no-cache")
        .header("Accept-Encoding", "identity")
        .header("User-Agent", "Obsession-Diagnostics/1.2");
    if range {
        request = request.header("Range", format!("bytes=0-{}", limit - 1));
    }
    let request = request
        .body(Empty::<hyper::body::Bytes>::new())
        .map_err(std::io::Error::other)?;
    let response = sender
        .send_request(request)
        .await
        .map_err(std::io::Error::other)?;
    let status = response.status().as_u16();
    result.http_status = Some(status);
    result.connections[connection_index]
        .http
        .as_mut()
        .unwrap()
        .http_status = Some(status);
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_owned();
    result.stage = "http_body";
    http_connection_stage(result, progress, connection_index, "http_body");
    let before = result.bytes;
    let mut response = response.into_body();
    let mut body = Vec::new();
    let mut complete = false;
    while let Some(frame) = response.frame().await {
        if let Ok(data) = frame.map_err(std::io::Error::other)?.into_data() {
            body.extend_from_slice(&data[..data.len().min(limit - body.len())]);
            result.bytes = before + body.len();
            result.connections[connection_index]
                .http
                .as_mut()
                .unwrap()
                .bytes = body.len();
            http_connection_stage(result, progress, connection_index, "http_body");
            if body.len() == limit {
                break;
            }
        }
    }
    if body.len() < limit {
        complete = true;
    }
    http_connection_stage(result, progress, connection_index, "complete");
    Ok(HttpSample {
        status,
        body,
        complete,
        content_type,
    })
}

async fn http_probe(target: Target, progress: ProbeProgress) -> FunctionEvidence {
    if matches!(target.kind, "large_gateway_api" | "large_png") {
        return large_tls_probe(target, progress).await;
    }
    if matches!(target.kind, "discord_assets" | "youtube_assets") {
        return asset_probe(target, progress).await;
    }
    let started = Instant::now();
    let mut result = pending(target, Outcome::Inconclusive, "Проверка не завершена.");
    let url = reqwest::Url::parse(target.url).expect("fixed target URL");
    match owned_get(
        &url,
        None,
        target.limit,
        false,
        "document",
        &mut result,
        &progress,
    )
    .await
    {
        Ok(response) => {
            let status = response.status;
            result.outcome = http_outcome(status);
            if result.outcome != Outcome::Passed {
                result.reason = Some(format!("HTTP {status}: цель непригодна для оценки обхода."));
            } else {
                if let Err(reason) = validate_body(target.kind, &response.body, response.complete) {
                    result.outcome = Outcome::Inconclusive;
                    result.stage = "target";
                    progress.update(&result);
                    result.reason = Some(reason.into());
                } else {
                    result.proof = "http_document";
                    result.reason = None;
                    result.stage = "complete";
                    progress.update(&result);
                }
            }
        }
        Err(error) => {
            result.outcome = if tls_setup_error(&error) {
                Outcome::Inconclusive
            } else {
                Outcome::Failed
            };
            result.reason = Some(format!("HTTPS-поток не установлен: {}", error));
        }
    }
    result.elapsed_ms = started.elapsed().as_millis() as u64;
    result
}

async fn large_tls_probe(target: Target, progress: ProbeProgress) -> FunctionEvidence {
    use super::zapret2_tls as large;
    let started = Instant::now();
    let mut result = pending(
        target,
        Outcome::Inconclusive,
        "Большой TLS-запрос не завершён.",
    );
    let url = reqwest::Url::parse(target.url).expect("fixed public URL");
    let host = url.host_str().unwrap();
    let Some(first) = connect_target(host, &mut result, &progress).await else {
        return result;
    };
    let remote = first.peer_addr().expect("connected peer");
    let mut first = Some(first);
    for kind in large::KINDS {
        let config = large::config();
        let prepared = match large::prepare(host, config.clone(), kind) {
            Ok(value) => value,
            Err(_) => {
                result.stage = "setup";
                result.reason =
                    Some("Не удалось создать требуемую форму большого TLS-запроса.".into());
                break;
            }
        };
        result.tls_handshakes.push(prepared.metadata.clone());
        result.stage = match kind {
            large::Kind::LargeEarlySni => "tls_large_early_sni",
            large::Kind::LargeLateSni => "tls_large_late_sni",
        };
        progress.update(&result);
        let stream = if let Some(stream) = first.take() {
            stream
        } else {
            match tokio::time::timeout(
                Duration::from_secs(2),
                tokio::net::TcpStream::connect(remote),
            )
            .await
            {
                Ok(Ok(stream)) => stream,
                _ => {
                    result.outcome = Outcome::Failed;
                    result.reason =
                        Some("Повторный TCP к закреплённому адресу не установлен.".into());
                    break;
                }
            }
        };
        let _ = stream.set_nodelay(true);
        let purpose = match kind {
            large::Kind::LargeEarlySni => "large_early_sni",
            large::Kind::LargeLateSni => "large_late_sni",
        };
        if let Ok(connection) =
            super::zapret2_routing::ConnectionEvidence::new(&stream, host, purpose)
        {
            result.connections.push(connection);
            progress.update(&result);
        }
        let name =
            rustls::pki_types::ServerName::try_from(host.to_owned()).expect("fixed TLS host");
        let tls = match large::connect(stream, name, config, Some(prepared)).await {
            Ok(stream) => stream,
            Err(_) => {
                result.outcome = Outcome::Failed;
                result.reason =
                    Some("Свежий большой TLS-запрос не завершил проверенное рукопожатие.".into());
                break;
            }
        };
        result.tls_handshakes.last_mut().unwrap().authenticated_tls = true;
        result.stage = "large_tls_document";
        progress.update(&result);
        let (status, body) = match large::public_get(tls, host, url.path(), target.limit / 2).await
        {
            Ok(value) => value,
            Err(_) => {
                result.outcome = Outcome::Failed;
                result.reason =
                    Some("Ответ после большого TLS-запроса оборвался или превысил лимит.".into());
                break;
            }
        };
        result.bytes += body.len();
        result.http_status = Some(status);
        let kind = if target.kind == "large_gateway_api" {
            "gateway_api"
        } else {
            "png"
        };
        if status != 200 || validate_body(kind, &body, true).is_err() {
            result.reason =
                Some("Ответ цели после большого TLS-запроса не подтвердил API/CDN.".into());
            break;
        }
        result.tls_handshakes.last_mut().unwrap().application_ok = true;
        progress.update(&result);
    }
    let evidence = large::Evidence {
        policy: large::POLICY.into(),
        handshakes: result.tls_handshakes.clone(),
    };
    if evidence.passed() {
        result.outcome = Outcome::Passed;
        result.proof = "large_tls_application";
        result.stage = "complete";
        result.reason = None;
    }
    result.elapsed_ms = started.elapsed().as_millis() as u64;
    result
}

fn public_asset_url(kind: &str, page: &[u8]) -> Option<reqwest::Url> {
    // Only same-origin public script paths from the bounded page are accepted.
    // No redirects, arbitrary hosts, cookies or signed media URLs are followed.
    let text = String::from_utf8_lossy(page).replace("\\/", "/");
    let (host, pattern) = if kind == "discord_assets" {
        (
            "discord.com",
            r#"(?:src|href)=["'](/assets/[A-Za-z0-9_-][A-Za-z0-9_.-]{7,80}\.js)["']"#,
        )
    } else if kind == "youtube_assets" {
        (
            "www.youtube.com",
            r#"["'](/s/player/[A-Za-z0-9_./-]{1,180}/base\.js)["']"#,
        )
    } else {
        return None;
    };
    let expression = regex::Regex::new(pattern).expect("fixed script path expression");
    let path = expression.captures(&text)?.get(1)?.as_str();
    if path.split('/').any(|component| component == "..") {
        return None;
    }
    reqwest::Url::parse(&format!("https://{host}{path}")).ok()
}

fn transfer_passes(bytes: usize, elapsed: Duration) -> bool {
    bytes >= MIN_TRANSFER_BYTES
        && (bytes as u128 * 1000) >= MIN_TRANSFER_BYTES_PER_SEC as u128 * elapsed.as_millis().max(1)
}

async fn asset_probe(target: Target, progress: ProbeProgress) -> FunctionEvidence {
    let started = Instant::now();
    let mut result = pending(
        target,
        Outcome::Inconclusive,
        "Загрузка файлов не проверена.",
    );
    let response = async {
        let page_url = reqwest::Url::parse(target.url).expect("fixed page URL");
        let page = owned_get(
            &page_url,
            None,
            target.limit,
            false,
            "asset_page",
            &mut result,
            &progress,
        )
        .await?;
        if !(200..300).contains(&page.status) {
            return Ok::<_, std::io::Error>(None);
        }
        let remote = result
            .connections
            .last()
            .expect("owned page socket")
            .remote_address;
        let Some(url) = public_asset_url(target.kind, &page.body) else {
            result.stage = "target";
            result.reason = Some(
                "Страница не предоставила пригодный публичный файл; загрузка не подтверждена."
                    .into(),
            );
            return Ok(None);
        };
        result.target = url.to_string();
        result.stage = "asset_headers";
        progress.update(&result);
        let transfer_started = Instant::now();
        let response = owned_get(
            &url,
            Some(remote),
            TRANSFER_BYTES,
            true,
            "asset",
            &mut result,
            &progress,
        )
        .await?;
        if !matches!(response.status, 200 | 206) {
            return Ok(None);
        }
        if !response.content_type.contains("javascript")
            && !response.content_type.contains("ecmascript")
        {
            result.stage = "target";
            result.reason = Some("Сервер вернул содержимое другого типа вместо JavaScript.".into());
            return Ok(None);
        }
        Ok(Some((response.body.len(), transfer_started.elapsed())))
    }
    .await;
    match response {
        Ok(Some((bytes, elapsed))) if transfer_passes(bytes, elapsed) => {
            result.outcome = Outcome::Passed;
            result.proof = "http_transfer";
            result.reason = None;
            result.stage = "complete";
            // Account discovery bytes, but rank only the fixed script sample.
            result.transfer_bytes = bytes;
            result.transfer_ms = Some(elapsed.as_millis().max(1) as u64);
            result.elapsed_ms = started.elapsed().as_millis() as u64;
            return result;
        }
        Ok(Some((bytes, elapsed))) if bytes >= MIN_TRANSFER_BYTES => {
            result.outcome = Outcome::Failed;
            result.reason = Some(format!(
                "Файл загружается слишком медленно: {} КБ/с; минимум 256 КБ/с для этой пробы.",
                bytes as u128 * 1000 / elapsed.as_millis().max(1) / 1024
            ));
        }
        Ok(Some(_)) => {
            result.reason =
                Some("Размер файла недостаточен для оценки скорости; успех не засчитан.".into())
        }
        Ok(None) => {}
        Err(error) => {
            result.outcome = if tls_setup_error(&error) {
                Outcome::Inconclusive
            } else {
                Outcome::Failed
            };
            result.reason = Some(format!("Загрузка файлов не завершена: {error}"));
        }
    }
    result.elapsed_ms = started.elapsed().as_millis() as u64;
    result
}

fn valid_hello(body: &[u8]) -> bool {
    serde_json::from_slice::<serde_json::Value>(body)
        .ok()
        .is_some_and(|json| {
            json["op"].as_u64() == Some(10)
                && json["d"]["heartbeat_interval"]
                    .as_u64()
                    .is_some_and(|v| v > 0 && v <= 600_000)
        })
}

async fn gateway_probe(target: Target, progress: ProbeProgress) -> FunctionEvidence {
    let started = Instant::now();
    let mut result = pending(target, Outcome::Inconclusive, "Gateway не ответил.");
    let Some(stream) = connect_target("gateway.discord.gg", &mut result, &progress).await else {
        result.elapsed_ms = started.elapsed().as_millis() as u64;
        return result;
    };
    // Explicit provider and new config prevent TLS sessions from being shared
    // between candidates. No token, Identify or user session is sent.
    let roots = rustls::RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let tls = fresh_http_tls(roots);
    let mut result = gateway_exchange(target, stream, tls, result, &progress).await;
    result.elapsed_ms = started.elapsed().as_millis() as u64;
    result
}

async fn gateway_exchange(
    target: Target,
    stream: tokio::net::TcpStream,
    tls: std::sync::Arc<rustls::ClientConfig>,
    mut result: FunctionEvidence,
    progress: &ProbeProgress,
) -> FunctionEvidence {
    use tokio_tungstenite::{
        client_async_with_config,
        tungstenite::{protocol::WebSocketConfig, Message},
    };
    let config = WebSocketConfig::default()
        .read_buffer_size(4096)
        .write_buffer_size(4096)
        .max_write_buffer_size(8192)
        .max_message_size(Some(target.limit))
        .max_frame_size(Some(target.limit));
    let url = reqwest::Url::parse(target.url).expect("fixed Gateway URL");
    let host = url.host_str().unwrap();
    if let Ok(connection) =
        super::zapret2_routing::ConnectionEvidence::new(&stream, host, "gateway")
    {
        result.connections.push(connection);
    }
    result.stage = "tls";
    progress.update(&result);
    let name =
        rustls::pki_types::ServerName::try_from(host.to_owned()).expect("fixed Gateway host");
    let stream = match tokio_rustls::TlsConnector::from(tls)
        .connect(name, stream)
        .await
    {
        Ok(stream) => stream,
        Err(error) => {
            result.outcome = if tls_setup_error(&error) {
                Outcome::Inconclusive
            } else {
                Outcome::Failed
            };
            result.reason = Some("TLS Gateway не установлен; WebSocket ещё не проверялся.".into());
            return result;
        }
    };
    result.stage = "websocket_upgrade";
    progress.update(&result);
    match client_async_with_config(target.url, stream, Some(config)).await {
        Ok((mut socket, response)) => {
            result.http_status = Some(response.status().as_u16());
            result.stage = "gateway_hello";
            progress.update(&result);
            for _ in 0..4 {
                match socket.next().await {
                    Some(Ok(Message::Text(text))) => {
                        result.bytes += text.len();
                        if valid_hello(text.as_bytes()) {
                            result.outcome = Outcome::Passed;
                            result.proof = "gateway_hello";
                            result.stage = "complete";
                            progress.update(&result);
                            result.reason = None;
                        } else {
                            result.reason =
                                Some("Gateway прислал неожидаемое событие вместо Hello.".into());
                        }
                        break;
                    }
                    Some(Ok(Message::Ping(payload))) => {
                        let _ = socket.send(Message::Pong(payload)).await;
                    }
                    _ => {
                        result.reason = Some("Gateway закрыл поток до Hello.".into());
                        break;
                    }
                }
            }
            // Cleanup is bounded by the enclosing round deadline as well.
            let _ = tokio::time::timeout(Duration::from_millis(150), socket.close(None)).await;
        }
        Err(error) => {
            result.outcome = if tls_setup_error(&error)
                || matches!(
                    error,
                    tokio_tungstenite::tungstenite::Error::Tls(_)
                        | tokio_tungstenite::tungstenite::Error::Http(_)
                        | tokio_tungstenite::tungstenite::Error::Protocol(_)
                ) {
                Outcome::Inconclusive
            } else {
                Outcome::Failed
            };
            result.reason = Some("TLS подтверждён, но WebSocket Gateway не установлен.".into());
        }
    }
    result
}

pub(super) async fn network_key() -> Option<String> {
    crate::netid::resolve_local_read_only()
        .await
        .fingerprint
        .stable_key()
        .map(str::to_owned)
}

pub(super) fn set_context(round: &mut RoundMeasurement, initial: Option<&str>) {
    round.comparable = initial.is_some()
        && round.network_before.as_deref() == initial
        && round.network_after.as_deref() == initial;
    round.context_reason = if round.comparable {
        None
    } else if initial.is_none() || round.network_before.is_none() || round.network_after.is_none() {
        Some("network_unknown")
    } else {
        Some("network_changed")
    };
}

fn untested(category: &str, transport: &str) -> Vec<FunctionEvidence> {
    let values: &[(&str, &str)] = if category == "discord" {
        &[
            ("session", "Вход и готовность клиента"),
            ("voice", "Голосовой вызов"),
        ]
    } else if category == "youtube_twitch" {
        &[("media", "Медиаданные и воспроизведение видео")]
    } else {
        &[]
    };
    let mut result = values
        .iter()
        .map(|(id, label)| FunctionEvidence {
            function: (*id).into(),
            label: (*label).into(),
            required: false,
            outcome: Outcome::NotTested,
            proof: "none",
            stage: "scenario",
            reason: Some("Требуется отдельное испытание в приложении.".into()),
            target: String::new(),
            remote_address: None,
            ip_family: None,
            elapsed_ms: 0,
            bytes: 0,
            transfer_bytes: 0,
            transfer_ms: None,
            tls_handshakes: vec![],
            http_status: None,
            connections: vec![],
        })
        .collect::<Vec<_>>();
    if transport == "quic" {
        result.push(FunctionEvidence {
            function: "http3".into(),
            label: "HTTP/3-запрос".into(),
            required: false,
            outcome: Outcome::NotTested,
            proof: "none",
            stage: "scenario",
            reason: Some("Проверяется только QUIC handshake.".into()),
            target: String::new(),
            remote_address: None,
            ip_family: None,
            elapsed_ms: 0,
            bytes: 0,
            transfer_bytes: 0,
            transfer_ms: None,
            tls_handshakes: vec![],
            http_status: None,
            connections: vec![],
        });
    }
    result
}

pub(super) fn finish(
    mut round: RoundMeasurement,
    category: &str,
    transport: &str,
    cancelled: bool,
) -> TestReport {
    let routing_required = round.transport == "tls"
        && matches!(
            round.phase.as_str(),
            "screening" | "confirmation" | "composition"
        );
    let checks = round
        .functions
        .iter()
        .filter(|f| f.required)
        .map(|f| ProbeResult {
            url: f.target.clone(),
            passed: f.passed() && (!routing_required || f.routing_verified()),
            elapsed_ms: f.elapsed_ms,
            bytes: f.bytes,
            attempts: u8::from(f.outcome != Outcome::NotTested),
            http_status: f.http_status,
            error: if routing_required && f.passed() && !f.routing_verified() {
                Some("Ответ получен, но обработка нужным блоком winws2 не подтверждена.".into())
            } else {
                f.reason.clone()
            },
        })
        .collect();
    round.functions.extend(untested(category, transport));
    let mut report = super::legacy_test::report(checks, cancelled);
    if !cancelled && routing_required && !round.complete() && report.passed {
        report.passed = false;
        report.status = "inconclusive".into();
    }
    report.measurement = Some(round);
    report
}

pub(super) fn unavailable(
    mut round: RoundMeasurement,
    category: &str,
    transport: &str,
    reason: &str,
) -> TestReport {
    round.functions = targets(category)
        .unwrap_or_default()
        .into_iter()
        .map(|target| {
            let mut evidence = pending(target, Outcome::NotTested, reason);
            evidence.stage = "runtime";
            evidence
        })
        .collect();
    if transport == "quic" {
        for evidence in &mut round.functions {
            evidence.proof = "none";
            evidence.target = evidence.target.replace("https://", "quic://");
        }
    }
    round.comparable = false;
    finish(round, category, transport, false)
}

async fn collect_functions<C, F, P>(
    targets: &[Target],
    timeout: Duration,
    cancel: C,
    probe: F,
) -> (Vec<FunctionEvidence>, bool)
where
    C: std::future::Future<Output = ()>,
    F: Fn(Target, ProbeProgress) -> P,
    P: std::future::Future<Output = FunctionEvidence> + Send + 'static,
{
    let mut tasks = tokio::task::JoinSet::new();
    let permit = std::sync::Arc::new(tokio::sync::Semaphore::new(4));
    let progress: Vec<_> = targets.iter().copied().map(ProbeProgress::new).collect();
    for (index, target) in targets.iter().copied().enumerate() {
        let permit = permit.clone();
        let tracker = progress[index].clone();
        let future = probe(target, tracker.clone());
        tasks.spawn(async move {
            let _permit = permit.acquire().await.expect("owned semaphore");
            tracker.start();
            (index, future.await)
        });
    }
    let mut functions: Vec<Option<FunctionEvidence>> = targets.iter().map(|_| None).collect();
    let deadline = tokio::time::sleep(timeout);
    tokio::pin!(deadline, cancel);
    let mut cancelled = false;
    let required = targets.iter().filter(|target| target.required).count();
    let mut required_complete = false;
    while !tasks.is_empty() {
        tokio::select! {
            biased;
            _ = &mut cancel => { cancelled = true; break; },
            _ = &mut deadline => break,
            joined = tasks.join_next() => {
                if let Some(Ok((index, evidence))) = joined { functions[index] = Some(evidence); }
                if required > 0 && targets.iter().enumerate().filter(|(_, target)| target.required)
                    .all(|(index, _)| functions[index].is_some()) { required_complete = true; break; }
            }
        }
    }
    tasks.abort_all();
    while tasks.join_next().await.is_some() {}
    let results = progress
        .into_iter()
        .zip(functions)
        .map(|(progress, value)| {
            value.unwrap_or_else(|| progress.incomplete(cancelled, required_complete))
        })
        .collect();
    (results, cancelled)
}

pub(super) async fn measure(
    app: &AppHandle,
    category: &str,
    transport: &str,
    epoch: u64,
    timeout: Duration,
    mut round: RoundMeasurement,
    initial_network: Option<&str>,
) -> Result<TestReport, String> {
    round.network_before = network_key().await;
    let was_cancelled;
    if transport == "quic" {
        let report = super::zapret2_quic::probe(app, category, epoch, timeout).await?;
        was_cancelled = report.status == "cancelled";
        round.functions = report
            .checks
            .iter()
            .enumerate()
            .map(|(index, check)| FunctionEvidence {
                function: format!("quic_{}", index),
                label: if index == 0 {
                    "QUIC YouTube"
                } else {
                    "QUIC Image CDN"
                }
                .into(),
                required: true,
                outcome: if was_cancelled {
                    Outcome::Cancelled
                } else if check.passed {
                    Outcome::Passed
                } else {
                    Outcome::Inconclusive
                },
                proof: if check.passed {
                    "quic_handshake"
                } else {
                    "none"
                },
                stage: "quic_handshake",
                reason: check.error.clone(),
                target: check.url.clone(),
                remote_address: None,
                ip_family: None,
                elapsed_ms: check.elapsed_ms,
                bytes: check.bytes,
                transfer_bytes: 0,
                transfer_ms: None,
                tls_handshakes: vec![],
                http_status: None,
                connections: vec![],
            })
            .collect();
    } else {
        let targets = targets(category)?;
        let cancel = async {
            while !super::legacy_test::cancelled(app, epoch) {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        };
        let (functions, cancelled) =
            collect_functions(&targets, timeout, cancel, |target, progress| async move {
                if target.kind == "wss" {
                    gateway_probe(target, progress).await
                } else {
                    http_probe(target, progress).await
                }
            })
            .await;
        round.functions = functions;
        was_cancelled = cancelled;
        #[cfg(windows)]
        if matches!(
            round.phase.as_str(),
            "screening" | "confirmation" | "composition"
        ) && !was_cancelled
        {
            if let Some(generation) = round.runtime_generation {
                // Drain the bounded stdout reader before restoring/stopping
                // this exact generation. Never inspect the restored runtime.
                tokio::time::sleep(Duration::from_millis(75)).await;
                let response = tauri::async_runtime::spawn_blocking(move || {
                    super::call_service(
                        "zapret2-probe-routing",
                        Request::GetDpiDiagnostics(DpiStopRequest { generation }),
                        Duration::from_secs(2),
                    )
                })
                .await;
                match response {
                    Ok(Ok(Response::DpiDiagnostics(diagnostics))) => {
                        super::zapret2_routing::annotate(&mut round, Ok(&diagnostics))
                    }
                    _ => {
                        super::zapret2_routing::annotate(&mut round, Err("diagnostics_unavailable"))
                    }
                }
            }
            crate::util::write_diagnostic_log(
                app,
                "info",
                "zapret2-picker",
                &serde_json::json!({
                    "runId":round.run_id,"profileId":round.profile_id,"ordinal":round.ordinal,
                    "runtimeGeneration":round.runtime_generation,"execution":round.execution,
                    "functions":round.functions,"packetCapture":round.packet_capture
                })
                .to_string(),
            );
        }
    }
    round.network_after = network_key().await;
    set_context(&mut round, initial_network);
    Ok(finish(
        round,
        category,
        transport,
        was_cancelled || super::legacy_test::cancelled(app, epoch),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn public_assets_are_bounded_same_origin_paths_and_missing_media_is_not_success() {
        let discord = public_asset_url(
            "discord_assets",
            br#"<script src="/assets/web.12345678abcd.js"></script>"#,
        )
        .unwrap();
        assert_eq!(
            discord.as_str(),
            "https://discord.com/assets/web.12345678abcd.js"
        );
        let youtube = public_asset_url(
            "youtube_assets",
            br#"{"jsUrl":"\/s\/player\/1234abcd\/player_ias.vflset\/en_US\/base.js"}"#,
        )
        .unwrap();
        assert_eq!(youtube.host_str(), Some("www.youtube.com"));
        assert!(public_asset_url(
            "discord_assets",
            br#"<script src="https://foreign.invalid/assets/12345678abcd.js">"#
        )
        .is_none());
        assert!(
            public_asset_url("youtube_assets", br#""/s/player/../../secret/base.js""#).is_none()
        );
        assert!(
            public_asset_url("youtube_assets", b"Sign in to confirm you're not a bot").is_none()
        );
        assert!(!transfer_passes(1268, Duration::from_millis(1)));
        assert!(!transfer_passes(TRANSFER_BYTES, Duration::from_secs(3)));
        assert!(transfer_passes(TRANSFER_BYTES, Duration::from_secs(2)));
        for category in ["discord", "youtube_twitch", "gaming"] {
            let budget: usize = targets(category)
                .unwrap()
                .iter()
                .map(|t| {
                    t.limit
                        + if matches!(t.kind, "discord_assets" | "youtube_assets") {
                            TRANSFER_BYTES
                        } else {
                            0
                        }
                })
                .sum();
            assert!(budget <= MAX_ROUND_BYTES);
        }
    }
    #[cfg(windows)]
    #[tokio::test]
    #[ignore = "Explicit live candidate diagnostic; requires inactive DPI and opt-in environment"]
    async fn live_candidate_address_diagnostic() {
        use obsession_runtime_client::RuntimeClient;
        use obsession_runtime_protocol::*;
        assert_eq!(
            std::env::var("OBSESSION_ZAPRET2_LIVE_TEST").as_deref(),
            Ok("1")
        );
        let client = RuntimeClient::new(Duration::from_secs(5)).unwrap();
        struct Cleanup<'a> {
            client: &'a RuntimeClient,
            generation: Option<u64>,
        }
        impl Drop for Cleanup<'_> {
            fn drop(&mut self) {
                if let Some(generation) = self.generation {
                    let result = self.client.call(
                        "address-diagnostic-cleanup",
                        Request::DpiStop(DpiStopRequest { generation }),
                    );
                    eprintln!("Owned generation cleanup: {result:?}");
                }
            }
        }
        let bulk = std::env::var("OBSESSION_ZAPRET2_BULK_TEST").as_deref() == Ok("1");
        let browser = std::env::var("OBSESSION_ZAPRET2_BROWSER_TEST").as_deref() == Ok("1");
        let transfer = std::env::var("OBSESSION_ZAPRET2_TRANSFER_TEST").as_deref() == Ok("1");
        let large_tls = std::env::var("OBSESSION_ZAPRET2_LARGE_TLS_TEST").as_deref() == Ok("1");
        let profiles = if large_tls {
            vec![
                ("discord", "research_discord_tls_window_disorder"),
                ("discord", "discord_tls_multidisorder_alt"),
                ("discord", "research_discord_tls_hostfake_md5"),
                ("discord", "research_discord_tls_hostfake_md5"),
                ("discord", "research_discord_tls_hostfake_md5"),
            ]
        } else if transfer {
            vec![
                ("discord", "discord_tls_multidisorder_alt"),
                ("discord", "research_discord_tls_window_disorder"),
                ("discord", "discord_tls_multidisorder_alt"),
                ("discord", "research_discord_tls_window_disorder"),
                ("discord", "discord_tls_multidisorder_alt"),
                ("discord", "research_discord_tls_window_disorder"),
                ("youtube_twitch", "youtube_tls"),
            ]
        } else if browser {
            vec![
                ("discord", "research_discord_tls_window_disorder"),
                ("discord", "discord_tls_multidisorder_alt"),
            ]
        } else if bulk {
            vec![
                ("discord", "research_discord_tls_window_disorder"),
                ("discord", "discord_tls_multidisorder_alt"),
                ("discord", "research_discord_tls_early_disorder"),
                ("discord", "classic_discord_tls_fake_md5_disorder"),
                ("youtube_twitch", "youtube_tls"),
                (
                    "youtube_twitch",
                    "research_youtube_twitch_tls_early_disorder",
                ),
                ("youtube_twitch", "research_youtube_twitch_tls_hostfake_ack"),
            ]
        } else {
            vec![
                ("discord", "discord_tls_multidisorder_alt"),
                ("discord", "research_discord_tls_window_disorder"),
                ("discord", "discord_tls_multidisorder_alt"),
                ("youtube_twitch", "youtube_tls"),
                (
                    "youtube_twitch",
                    "research_youtube_twitch_tls_window_disorder",
                ),
            ]
        };
        for (category, profile) in profiles {
            let Response::RuntimeSnapshot(before) = client
                .call("address-diagnostic-before", Request::GetRuntimeSnapshot)
                .unwrap()
                .response
            else {
                panic!("snapshot unavailable")
            };
            assert!(before.dpi.is_none(), "Refusing to alter an active runtime");
            let mut request = DpiStartRequest {
                engine: DpiEngine::Zapret2,
                selections: vec![DpiSelection {
                    category: if category == "discord" {
                        DpiCategory::Discord
                    } else {
                        DpiCategory::YoutubeTwitch
                    },
                    strategy_id: if category == "discord" {
                        "builtin-discord"
                    } else {
                        "builtin-youtube-twitch"
                    }
                    .into(),
                }],
                options: DpiRuntimeOptions {
                    zapret2_level: 4,
                    zapret2_clienthello: false,
                    legacy_reliability: false,
                    zapret2_overrides: vec![],
                    zapret2_diagnostics: false,
                    zapret2_profiles: vec![Zapret2ProfileSelection {
                        category: category.into(),
                        transport: "tls".into(),
                        profile_id: profile.into(),
                    }],
                },
            };
            if browser {
                request.selections.push(DpiSelection {
                    category: DpiCategory::YoutubeTwitch,
                    strategy_id: "builtin-youtube-twitch".into(),
                });
            }
            let started = client
                .call("address-diagnostic-start", Request::DpiStart(request))
                .unwrap()
                .response;
            let Response::Started(started) = started else {
                panic!("Start rejected: {started:?}")
            };
            let mut cleanup = Cleanup {
                client: &client,
                generation: Some(started.generation),
            };
            tokio::time::sleep(Duration::from_millis(800)).await;
            let start = Instant::now();
            let (functions, _) = collect_functions(
                &targets(category).unwrap(),
                Duration::from_secs(3),
                std::future::pending(),
                |target, progress| async move {
                    if target.kind == "wss" {
                        gateway_probe(target, progress).await
                    } else {
                        http_probe(target, progress).await
                    }
                },
            )
            .await;
            println!(
                "{}",
                serde_json::json!({"category":category,"profile":profile,"generation":started.generation,"elapsedMs":start.elapsed().as_millis(),"functions":functions})
            );
            if browser {
                println!("BROWSER_DIAGNOSTIC_READY {profile}");
                let done = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .parent()
                    .unwrap()
                    .join(".dev-logs/zapret2-browser-complete.txt");
                let limit = tokio::time::Instant::now() + Duration::from_secs(90);
                while tokio::time::Instant::now() < limit {
                    if std::fs::read_to_string(&done).ok().as_deref() == Some(profile) {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(200)).await;
                }
            } else if bulk {
                let mut command = tokio::process::Command::new("python");
                command
                    .arg(".dev-logs/zapret2-bulk-probe.py")
                    .arg(category)
                    .current_dir(
                        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                            .parent()
                            .unwrap(),
                    )
                    .kill_on_drop(true);
                let output = tokio::time::timeout(Duration::from_secs(25), command.output()).await;
                match output {
                    Ok(Ok(output)) if output.status.success() => println!(
                        "{}",
                        serde_json::json!({
                            "profile": profile,
                            "bulk": serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap()
                        })
                    ),
                    other => panic!("Bounded bulk diagnostic failed: {other:?}"),
                }
            } else if category == "discord" && !transfer && !large_tls {
                let old_started = Instant::now();
                let old = tokio::time::timeout(Duration::from_secs(3), async {
                    let _ = tokio::net::lookup_host(("discord.com", 443)).await;
                    reqwest::Client::builder()
                        .no_proxy()
                        .http1_only()
                        .pool_max_idle_per_host(0)
                        .user_agent("Obsession-Diagnostics/1.1")
                        .redirect(reqwest::redirect::Policy::none())
                        .build()
                        .unwrap()
                        .get("https://discord.com/api/v10/gateway")
                        .header("Cache-Control", "no-cache")
                        .send()
                        .await
                })
                .await;
                let observation = match old {
                    Ok(Ok(response)) => {
                        serde_json::json!({"httpStatus":response.status().as_u16(),"remoteAddress":response.remote_addr()})
                    }
                    Ok(Err(error)) => serde_json::json!({"error":error.to_string()}),
                    Err(_) => serde_json::json!({"timeout":true}),
                };
                println!(
                    "{}",
                    serde_json::json!({"profile":profile,"legacyRequestHeaders":observation,"elapsedMs":old_started.elapsed().as_millis()})
                );
            }
            let stopped = client
                .call(
                    "address-diagnostic-stop",
                    Request::DpiStop(DpiStopRequest {
                        generation: started.generation,
                    }),
                )
                .unwrap()
                .response;
            assert!(
                matches!(stopped, Response::Stopped),
                "Cleanup failed: {stopped:?}"
            );
            cleanup.generation = None;
            let Response::RuntimeSnapshot(after) = client
                .call("address-diagnostic-after", Request::GetRuntimeSnapshot)
                .unwrap()
                .response
            else {
                panic!("snapshot unavailable")
            };
            assert!(after.dpi.is_none(), "Runtime state changed after cleanup");
        }
    }

    #[tokio::test]
    async fn unreachable_address_does_not_hide_a_working_peer() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let remote = listener.local_addr().unwrap();
        let stream = tokio::time::timeout(
            Duration::from_secs(1),
            connect_addresses(vec!["127.0.0.1:0".parse().unwrap(), remote, remote]),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(stream.peer_addr().unwrap(), remote);
        assert!(connect_addresses(vec![]).await.is_err());
    }

    #[tokio::test]
    async fn pinned_http_preserves_sni_host_and_certificate_validation() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let generated =
            rcgen::generate_simple_self_signed(vec!["pinned-fixture.invalid".into()]).unwrap();
        let cert = generated.cert.der().clone();
        let key = rustls::pki_types::PrivatePkcs8KeyDer::from(generated.key_pair.serialize_der());
        let config = rustls::ServerConfig::builder_with_provider(std::sync::Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(vec![cert.clone()], key.into())
        .unwrap();
        let acceptor = tokio_rustls::TlsAcceptor::from(std::sync::Arc::new(config));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let remote = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (stream, peer) = listener.accept().await.unwrap();
            let mut tls = acceptor.accept(stream).await.unwrap();
            assert_eq!(
                tls.get_ref().1.server_name(),
                Some("pinned-fixture.invalid")
            );
            let mut bytes = vec![0; 4096];
            let size = tls.read(&mut bytes).await.unwrap();
            assert!(String::from_utf8_lossy(&bytes[..size])
                .to_lowercase()
                .contains("host: pinned-fixture.invalid:"));
            tls.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nOK")
                .await
                .unwrap();
            tls.shutdown().await.unwrap();
            peer
        });
        let mut roots = rustls::RootCertStore::empty();
        roots.add(cert).unwrap();
        let target = targets("discord").unwrap()[2];
        let mut evidence = pending(target, Outcome::Inconclusive, "local TLS fixture");
        let url = reqwest::Url::parse(&format!(
            "https://pinned-fixture.invalid:{}/?probe=1",
            remote.port()
        ))
        .unwrap();
        let response = owned_get_with_config(
            &url,
            Some(remote),
            4096,
            false,
            "document",
            &mut evidence,
            &ProbeProgress::new(target),
            fresh_http_tls(roots),
        )
        .await
        .unwrap();
        assert_eq!(response.status, 200);
        assert_eq!(response.body, b"OK");
        assert!(response.complete);
        let server_peer = server.await.unwrap();
        assert_eq!(evidence.connections.len(), 1);
        assert_eq!(evidence.connections[0].local_address, server_peer);
        assert_eq!(evidence.connections[0].remote_address, remote);
        assert_eq!(evidence.connections[0].status, "unverified");
    }

    #[tokio::test]
    async fn completed_page_does_not_lend_its_http_status_to_a_stalled_asset_tls_socket() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let generated =
            rcgen::generate_simple_self_signed(vec!["asset-fixture.invalid".into()]).unwrap();
        let cert = generated.cert.der().clone();
        let key = rustls::pki_types::PrivatePkcs8KeyDer::from(generated.key_pair.serialize_der());
        let server_config = rustls::ServerConfig::builder_with_provider(std::sync::Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(vec![cert.clone()], key.into())
        .unwrap();
        let acceptor = tokio_rustls::TlsAcceptor::from(std::sync::Arc::new(server_config));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let remote = listener.local_addr().unwrap();
        let (peer_tx, peer_rx) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            let (page, page_peer) = listener.accept().await.unwrap();
            let mut page = acceptor.accept(page).await.unwrap();
            let mut request = vec![0; 4096];
            page.read(&mut request).await.unwrap();
            page.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nOK")
                .await
                .unwrap();
            page.shutdown().await.unwrap();
            let (mut asset, asset_peer) = listener.accept().await.unwrap();
            // Actual ClientHello arrives, but this local server deliberately
            // supplies no TLS response. Neither DPI nor winws2 is involved.
            let mut tls_header = [0; 5];
            asset.read_exact(&mut tls_header).await.unwrap();
            assert_eq!(tls_header[0], 22);
            peer_tx.send((page_peer, asset_peer)).unwrap();
            std::future::pending::<()>().await;
        });
        let mut roots = rustls::RootCertStore::empty();
        roots.add(cert).unwrap();
        let config = fresh_http_tls(roots);
        let target = targets("discord")
            .unwrap()
            .into_iter()
            .find(|t| t.function == "client_assets")
            .unwrap();
        let progress = ProbeProgress::new(target);
        progress.start();
        let mut result = pending(target, Outcome::Inconclusive, "local TLS fixture");
        let mut url = reqwest::Url::parse(&format!(
            "https://asset-fixture.invalid:{}/app",
            remote.port()
        ))
        .unwrap();
        let page = owned_get_with_config(
            &url,
            Some(remote),
            4096,
            false,
            "asset_page",
            &mut result,
            &progress,
            config.clone(),
        )
        .await
        .unwrap();
        assert_eq!(page.status, 200);
        url.set_path("/assets/test-script.12345678.js");
        result.target = url.to_string();
        assert!(tokio::time::timeout(
            Duration::from_millis(200),
            owned_get_with_config(
                &url,
                Some(remote),
                TRANSFER_BYTES,
                true,
                "asset",
                &mut result,
                &progress,
                config
            )
        )
        .await
        .is_err());
        let (page_peer, asset_peer) = peer_rx.await.unwrap();
        let snapshot = progress.incomplete(false, false);
        assert_eq!(snapshot.stage, "tls");
        assert_eq!(snapshot.http_status, None);
        assert_eq!(snapshot.bytes, 2); // Accounted discovery bytes, not asset bytes.
        assert_eq!(snapshot.outcome, Outcome::Inconclusive);
        assert_eq!(snapshot.proof, "none");
        assert_eq!(snapshot.connections.len(), 2);
        assert_eq!(snapshot.connections[0].local_address, page_peer);
        assert_eq!(snapshot.connections[1].local_address, asset_peer);
        let page = snapshot.connections[0].http.as_ref().unwrap();
        assert_eq!(
            (page.stage, page.http_status, page.bytes),
            ("complete", Some(200), 2)
        );
        assert!(page.tls_ms.is_some());
        let asset = snapshot.connections[1].http.as_ref().unwrap();
        assert_eq!(
            (asset.stage, asset.http_status, asset.bytes),
            ("tls", None, 0)
        );
        assert!(asset.tls_ms.is_none());
        assert!(asset.elapsed_ms >= 100);
        assert!(snapshot
            .connections
            .iter()
            .all(|c| c.status == "unverified"));
        server.abort();
        let _ = server.await;
    }

    #[tokio::test]
    async fn optional_endpoint_cannot_delay_completed_required_checks() {
        let mut targets = targets("youtube_twitch").unwrap();
        targets[1].required = false;
        let (functions, cancelled) = tokio::time::timeout(
            Duration::from_millis(500),
            collect_functions(
                &targets,
                Duration::from_secs(5),
                std::future::pending(),
                |target, _| async move {
                    if !target.required {
                        std::future::pending::<()>().await;
                    }
                    pending(target, Outcome::Passed, "fixture")
                },
            ),
        )
        .await
        .unwrap();
        assert!(!cancelled);
        assert_eq!(functions[0].outcome, Outcome::Passed);
        assert_ne!(functions[1].outcome, Outcome::Passed);
        assert!(functions[1]
            .reason
            .as_deref()
            .unwrap()
            .contains("Обязательные пробы завершены"));
    }

    #[tokio::test]
    #[ignore = "Live public endpoints; opt-in diagnostic without runtime mutations"]
    async fn live_functions_without_runtime_mutations() {
        for category in ["discord", "youtube_twitch"] {
            let targets = targets(category).unwrap();
            let (functions, _) = collect_functions(
                &targets,
                Duration::from_secs(15),
                std::future::pending(),
                |target, progress| async move {
                    if target.kind == "wss" {
                        gateway_probe(target, progress).await
                    } else {
                        http_probe(target, progress).await
                    }
                },
            )
            .await;
            println!(
                "{}",
                serde_json::json!({ "category": category, "execution": "unverified", "functions": functions })
            );
        }
    }
    #[test]
    fn gateway_hello_is_not_ready_and_http_failures_do_not_convict_a_recipe() {
        assert!(!tls_setup_error(&std::io::Error::from(
            std::io::ErrorKind::ConnectionReset
        )));
        assert!(tls_setup_error(&std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            rustls::Error::InvalidCertificate(rustls::CertificateError::NotValidForName)
        )));
        assert!(valid_hello(
            br#"{"op":10,"d":{"heartbeat_interval":45000}}"#
        ));
        assert!(!valid_hello(br#"{"op":0,"t":"READY","d":{}}"#));
        assert!(!valid_hello(br#"{"op":10,"d":{"heartbeat_interval":0}}"#));
        for code in [301, 403, 404, 429, 503] {
            assert_eq!(http_outcome(code), Outcome::Inconclusive);
        }
        assert!(targets("discord")
            .unwrap()
            .iter()
            .any(|t| t.function == "gateway" && t.kind == "wss"));
    }
    #[test]
    fn unknown_or_changed_networks_are_not_comparable() {
        let mut round = super::super::zapret2_tournament::tests_round("a", true);
        set_context(&mut round, None);
        assert!(!round.complete());
        round.network_before = Some("one".into());
        round.network_after = Some("two".into());
        set_context(&mut round, Some("one"));
        assert_eq!(round.context_reason, Some("network_changed"));
        round.network_after = Some("one".into());
        set_context(&mut round, Some("one"));
        assert!(round.complete());
    }
    #[test]
    fn scenarios_and_truncated_images_cannot_be_promoted_to_success() {
        assert!(untested("discord", "tls")
            .iter()
            .all(|f| f.outcome == Outcome::NotTested));
        assert!(untested("youtube_twitch", "quic")
            .iter()
            .any(|f| f.function == "http3" && f.outcome == Outcome::NotTested));
        assert!(validate_body("jpeg", &[0xff, 0xd8, 0xff, 0xff, 0xd9], false).is_err());
        for category in ["discord", "youtube_twitch", "gaming"] {
            assert!(
                targets(category)
                    .unwrap()
                    .iter()
                    .map(|t| t.limit)
                    .sum::<usize>()
                    <= MAX_ROUND_BYTES
            );
        }
    }

    #[tokio::test]
    async fn partial_failure_does_not_abort_cdn_and_timeout_does_not_invent_a_failure() {
        let targets = targets("youtube_twitch").unwrap();
        let (functions, cancelled) = collect_functions(
            &targets,
            Duration::from_secs(1),
            std::future::pending(),
            |target, _progress| async move {
                if target.function == "images" {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
                pending(
                    target,
                    if target.function == "site" {
                        Outcome::Failed
                    } else {
                        Outcome::Passed
                    },
                    "fixture",
                )
            },
        )
        .await;
        assert!(!cancelled);
        assert_eq!(functions[0].outcome, Outcome::Failed);
        assert_eq!(functions[1].outcome, Outcome::Passed);
        let (functions, cancelled) = collect_functions(
            &targets,
            Duration::from_millis(10),
            std::future::pending(),
            |target, _progress| async move {
                if target.function == "images" {
                    std::future::pending::<()>().await;
                }
                pending(target, Outcome::Passed, "fixture")
            },
        )
        .await;
        assert!(!cancelled);
        assert_eq!(functions[0].outcome, Outcome::Passed);
        assert_eq!(functions[1].outcome, Outcome::Inconclusive);
    }

    #[tokio::test]
    async fn timeout_retains_partial_evidence_and_distinguishes_queued_probes() {
        let targets = targets("discord").unwrap();
        let (functions, cancelled) = collect_functions(
            &targets,
            Duration::from_millis(25),
            std::future::pending(),
            |target, progress| async move {
                let mut partial = pending(target, Outcome::Passed, "fixture");
                partial.stage = "body";
                partial.bytes = 68;
                partial.http_status = Some(200);
                address(&mut partial, "127.0.0.1:443".parse().unwrap());
                progress.update(&partial);
                std::future::pending::<()>().await;
                partial
            },
        )
        .await;
        assert!(!cancelled);
        for evidence in &functions[..4] {
            assert_eq!(evidence.outcome, Outcome::Inconclusive);
            assert_eq!(evidence.stage, "body");
            assert_eq!(evidence.proof, "none");
            assert_eq!(evidence.bytes, 68);
            assert_eq!(evidence.http_status, Some(200));
            assert_eq!(evidence.remote_address.as_deref(), Some("127.0.0.1:443"));
            assert!(evidence.elapsed_ms > 0);
        }
        let queued = &functions[4];
        assert_eq!(queued.outcome, Outcome::NotTested);
        assert_eq!(queued.stage, "queued");
        assert_eq!(queued.bytes, 0);
        assert_eq!(queued.elapsed_ms, 0);
    }

    #[tokio::test]
    async fn cancellation_drops_all_network_work_before_returning() {
        struct Guard(std::sync::Arc<std::sync::atomic::AtomicUsize>);
        impl Drop for Guard {
            fn drop(&mut self) {
                self.0.fetch_sub(1, Ordering::SeqCst);
            }
        }
        let active = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let targets = targets("discord").unwrap();
        let (functions, cancelled) = collect_functions(
            &targets,
            Duration::from_secs(1),
            tokio::time::sleep(Duration::from_millis(10)),
            |target, _progress| {
                let active = active.clone();
                async move {
                    active.fetch_add(1, Ordering::SeqCst);
                    let _guard = Guard(active);
                    std::future::pending::<()>().await;
                    pending(target, Outcome::Passed, "unreachable")
                }
            },
        )
        .await;
        assert!(cancelled);
        assert!(functions.iter().all(|f| f.outcome == Outcome::Cancelled));
        assert_eq!(active.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn real_wss_requires_valid_certificate_and_gateway_hello_without_identify() {
        let generated =
            rcgen::generate_simple_self_signed(vec!["gateway-fixture.test".into()]).unwrap();
        let certificate = generated.cert.der().clone();
        let key = rustls::pki_types::PrivatePkcs8KeyDer::from(generated.key_pair.serialize_der());
        let server = rustls::ServerConfig::builder_with_provider(std::sync::Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(vec![certificate.clone()], key.into())
        .unwrap();
        let acceptor = tokio_rustls::TlsAcceptor::from(std::sync::Arc::new(server));
        let mut roots = rustls::RootCertStore::empty();
        roots.add(certificate).unwrap();
        let trusted = std::sync::Arc::new(
            rustls::ClientConfig::builder_with_provider(std::sync::Arc::new(
                rustls::crypto::ring::default_provider(),
            ))
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_root_certificates(roots)
            .with_no_client_auth(),
        );
        for (url, body, expected) in [
            (
                "wss://gateway-fixture.test/?v=10&encoding=json",
                r#"{"op":10,"d":{"heartbeat_interval":45000}}"#,
                Outcome::Passed,
            ),
            (
                "wss://gateway-fixture.test/?v=10&encoding=json",
                r#"{"op":0,"t":"READY","d":{}}"#,
                Outcome::Inconclusive,
            ),
            (
                "wss://wrong-name.test/?v=10&encoding=json",
                r#"{"op":10,"d":{"heartbeat_interval":45000}}"#,
                Outcome::Inconclusive,
            ),
        ] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let endpoint = listener.local_addr().unwrap();
            let acceptor = acceptor.clone();
            let server = tokio::spawn(async move {
                let (stream, _) = listener.accept().await.unwrap();
                if let Ok(stream) = acceptor.accept(stream).await {
                    let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
                    socket
                        .send(tokio_tungstenite::tungstenite::Message::Text(body.into()))
                        .await
                        .unwrap();
                    if let Some(Ok(message)) = socket.next().await {
                        assert!(message.is_close(), "probe sent a user session event");
                    }
                }
            });
            let mut target = targets("discord").unwrap()[1];
            target.url = url;
            let stream = tokio::net::TcpStream::connect(endpoint).await.unwrap();
            let result = tokio::time::timeout(
                Duration::from_secs(2),
                gateway_exchange(
                    target,
                    stream,
                    trusted.clone(),
                    pending(target, Outcome::Inconclusive, "fixture"),
                    &ProbeProgress::new(target),
                ),
            )
            .await
            .unwrap();
            assert_eq!(result.outcome, expected);
            if expected == Outcome::Passed {
                assert_eq!(result.proof, "gateway_hello");
            }
            tokio::time::timeout(Duration::from_secs(2), server)
                .await
                .unwrap()
                .unwrap();
        }
    }
}
