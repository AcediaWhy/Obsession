//! Обработка одного клиентского соединения: опциональный FakeTLS-слой,
//! obfuscated2-handshake, подключение к DC и двунаправленное
//! перешифрование (порт `_handle_client` + `bridge_ws_reencrypt`).

use std::collections::HashMap;
use std::io;
use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

use rand::rngs::OsRng;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::TcpStream;
use tokio::task::JoinSet;
use tokio_rustls::TlsConnector;

use crate::cfrelay::CfRelay;
use crate::crypto::CtrCipher;
use crate::dc;
use crate::fake_tls;
use crate::logger;
use crate::obf2::{self, CryptoCtx, ProtoTag};
use crate::splitter::{MsgSplitter, Proto};
use crate::upstream::{Framing, Upstream, UpstreamReader, UpstreamWriter};
use crate::ws;

const CLIENT_INIT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
const DRAIN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
const SESSION_WRITE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);
const WS_CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
const WS_RETRY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);
const DIRECT_RACE_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(900);
const TCP_CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(1200);
const WS_STAGGER: std::time::Duration = std::time::Duration::from_millis(100);
const FALLBACK_STAGGER: std::time::Duration = std::time::Duration::from_millis(200);
const RELAY_CONFIDENCE_TIME: std::time::Duration = std::time::Duration::from_secs(30);
// End-to-end tests show that a Worker-to-Telegram /apiws relay is not a viable
// transport for this proxy. Keep parsing the legacy flag for launch-contract
// compatibility, but never schedule the route. The separate public relay
// remains available when explicitly enabled.
const WORKER_ROUTES_ENABLED: bool = false;
const IP_FAIL_COOLDOWN: std::time::Duration = std::time::Duration::from_secs(2 * 60);
const IP_FAIL_PROBE_INTERVAL: std::time::Duration = std::time::Duration::from_secs(30);
const DC_FAIL_COOLDOWN: std::time::Duration = std::time::Duration::from_secs(60);
const READ_CHUNK: usize = 65536;
const MAX_FAKE_TLS_HELLO_LEN: usize = 18 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct DirectRoute {
    dc: u16,
    is_media: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RouteCandidate {
    DirectTcp,
    TelegramWebSocket,
    Fallback,
}

impl RouteCandidate {
    fn label(self) -> &'static str {
        match self {
            Self::DirectTcp => "direct-tcp",
            Self::TelegramWebSocket => "telegram-wss",
            Self::Fallback => "fallback",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RouteRaceEvent {
    Unavailable(RouteCandidate),
    TaskFailed(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RelaySessionVerdict {
    Success,
    Failure,
    Neutral,
}

#[derive(Default)]
struct DirectHealth {
    ip_fail_until: HashMap<String, IpFailure>,
    dc_fail_until: HashMap<DirectRoute, Instant>,
}

#[derive(Debug, Clone, Copy)]
struct IpFailure {
    until: Instant,
    next_probe: Instant,
}

impl DirectHealth {
    fn direct_is_suppressed(&mut self, now: Instant, ip: &str) -> bool {
        self.ip_fail_until.retain(|_, failure| failure.until > now);
        self.ip_fail_until
            .get(ip)
            .is_some_and(|failure| now < failure.next_probe)
    }

    fn attempt_timeout(&mut self, now: Instant, ip: &str, route: DirectRoute) -> Option<Duration> {
        self.ip_fail_until.retain(|_, failure| failure.until > now);
        self.dc_fail_until.retain(|_, until| *until > now);

        if let Some(failure) = self.ip_fail_until.get_mut(ip) {
            if now >= failure.next_probe {
                failure.next_probe = now + IP_FAIL_PROBE_INTERVAL;
                return Some(WS_RETRY_TIMEOUT);
            }
            return None;
        }
        if self.dc_fail_until.contains_key(&route) {
            return Some(WS_RETRY_TIMEOUT);
        }
        Some(WS_CONNECT_TIMEOUT)
    }

    fn record_timeout(
        &mut self,
        now: Instant,
        ip: &str,
        route: DirectRoute,
        enable_ip_cooldown: bool,
    ) {
        if enable_ip_cooldown {
            self.ip_fail_until
                .entry(ip.to_string())
                .or_insert(IpFailure {
                    until: now + IP_FAIL_COOLDOWN,
                    next_probe: now + IP_FAIL_PROBE_INTERVAL,
                });
        }
        self.dc_fail_until.insert(route, now + DC_FAIL_COOLDOWN);
    }

    fn record_dc_failure(&mut self, now: Instant, route: DirectRoute) {
        self.dc_fail_until.insert(route, now + DC_FAIL_COOLDOWN);
    }

    fn record_success(&mut self, ip: &str, route: DirectRoute) {
        self.ip_fail_until.remove(ip);
        self.dc_fail_until.remove(&route);
    }
}

static DIRECT_HEALTH: LazyLock<Mutex<DirectHealth>> =
    LazyLock::new(|| Mutex::new(DirectHealth::default()));

fn lock_direct_health() -> std::sync::MutexGuard<'static, DirectHealth> {
    DIRECT_HEALTH
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn direct_timeout_for_route(timeout: Duration, has_fallback: bool) -> Duration {
    if has_fallback {
        timeout.min(DIRECT_RACE_TIMEOUT)
    } else {
        timeout
    }
}

fn fallback_is_available(
    is_test_dc: bool,
    public_relay_enabled: bool,
    worker_configured: bool,
) -> bool {
    !is_test_dc && (public_relay_enabled || (WORKER_ROUTES_ENABLED && worker_configured))
}

/// Общий контекст моста (один на процесс, раздаётся в задачи соединений).
pub struct BridgeContext {
    pub secret: [u8; 16],
    pub connector: TlsConnector,
    pub cf_relay: Option<Arc<CfRelay>>,
    pub worker_domains: Arc<Vec<String>>,
    pub no_direct: bool,
    pub masking: Option<String>,
}

/// Читающая сторона клиента: сырой TCP или обёртка FakeTLS.
pub enum ClientRead {
    Raw(OwnedReadHalf),
    Tls(fake_tls::FakeTlsReader<OwnedReadHalf>),
}

impl ClientRead {
    pub async fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        match self {
            ClientRead::Raw(read) => read.read(out).await,
            ClientRead::Tls(read) => read.read(out).await,
        }
    }
}

/// Пишущая сторона клиента: сырой TCP или обёртка FakeTLS.
pub enum ClientWrite {
    Raw(OwnedWriteHalf),
    Tls(fake_tls::FakeTlsWriter<OwnedWriteHalf>),
}

impl ClientWrite {
    pub async fn write_all(&mut self, data: &[u8]) -> io::Result<()> {
        match self {
            ClientWrite::Raw(write) => write.write_all(data).await,
            ClientWrite::Tls(write) => write.write_all(data).await,
        }
    }
}

pub async fn handle_client(stream: TcpStream, ctx: Arc<BridgeContext>) {
    let peer = match stream.peer_addr() {
        Ok(addr) => addr.to_string(),
        Err(_) => "?".to_string(),
    };
    let _ = stream.set_nodelay(true);
    let _ = ws::configure_tcp_keepalive(&stream);
    let (raw_read, raw_write) = stream.into_split();

    let Some((client_read, client_write, init)) =
        read_client_init(raw_read, raw_write, &ctx, &peer).await
    else {
        return;
    };

    let Some(parsed) = obf2::parse_client_init(&init, &ctx.secret) else {
        logger::warn(format!("[{peer}] bad handshake (wrong secret or proto)"));
        // Отличие от апстрима: дренаж ограничен по времени, а не бесконечен.
        drain_and_close(client_read).await;
        return;
    };

    let mut dc_id = parsed.dc;
    let is_test_dc = dc_id >= 10_000;
    if is_test_dc {
        logger::info(format!("[{peer}] test DC{dc_id} -> DC{}", dc_id - 10_000));
        dc_id -= 10_000;
    }
    let media_tag = if parsed.is_media { " media" } else { "" };

    let proto = match parsed.proto_tag {
        ProtoTag::Abridged => Proto::Abridged,
        ProtoTag::Intermediate => Proto::Intermediate,
        ProtoTag::Padded => Proto::Padded,
    };
    let dc_idx: i16 = if parsed.is_media {
        -(dc_id as i16)
    } else {
        dc_id as i16
    };

    logger::info(format!(
        "[{peer}] handshake ok: DC{dc_id}{media_tag} proto={:?}",
        parsed.proto_tag
    ));

    let relay_init = obf2::make_relay_init(parsed.proto_tag, dc_idx, &mut OsRng);
    let crypto = CryptoCtx::build(&init[8..56], &ctx.secret, &relay_init);
    let splitter = MsgSplitter::new(proto);

    let target_ip = match if is_test_dc {
        dc::dc_test_ip(dc_id)
    } else {
        dc::dc_ws_ip(dc_id)
    } {
        Some(ip) => ip,
        None => {
            logger::warn(format!(
                "[{peer}] DC{dc_id}{media_tag} not in config (no fallback in this build)"
            ));
            return;
        }
    };
    let ws_path = if is_test_dc {
        dc::WS_PATH_TEST
    } else {
        dc::WS_PATH
    };

    let has_fallback = fallback_is_available(
        is_test_dc,
        ctx.cf_relay.is_some(),
        !ctx.worker_domains.is_empty(),
    );
    let upstream = if ctx.no_direct {
        // Отладочный режим --no-direct: сразу в фолбэк.
        connect_fallback(&ctx, dc_id, is_test_dc, parsed.is_media, &peer).await
    } else {
        connect_with_route_race(
            ctx.clone(),
            target_ip,
            dc_id,
            is_test_dc,
            parsed.is_media,
            ws_path,
            peer.clone(),
            has_fallback,
        )
        .await
    };
    let upstream = match upstream {
        Some(upstream) => upstream,
        None => {
            logger::warn(format!(
                "[{peer}] DC{dc_id}{media_tag} no upstream route available"
            ));
            return;
        }
    };

    if let Err(error) = upstream.send_init(&relay_init).await {
        logger::warn(format!("[{peer}] relay init send failed: {error}"));
        if let Some(feedback) = upstream.relay_feedback() {
            feedback.failure();
        }
        return;
    }

    run_session(
        client_read,
        client_write,
        upstream,
        crypto,
        splitter,
        peer,
        dc_id,
        parsed.is_media,
    )
    .await;
}

async fn connect_fallback(
    ctx: &BridgeContext,
    dc_id: u16,
    is_test_dc: bool,
    is_media: bool,
    peer: &str,
) -> Option<Upstream> {
    if WORKER_ROUTES_ENABLED && !ctx.worker_domains.is_empty() {
        if let Some((reader, write, _)) = crate::cfrelay::connect_worker(
            &ctx.worker_domains,
            dc_id,
            is_media,
            &ctx.connector,
            peer,
        )
        .await
        {
            return Some(Upstream::from_websocket(reader, write, "worker-wss"));
        }
    }

    if is_test_dc {
        logger::warn(format!(
            "[{peer}] DC{dc_id}{} no fallback available (test DC)",
            if is_media { " media" } else { "" }
        ));
        return None;
    }

    match ctx.cf_relay.as_ref() {
        Some(relay) => relay
            .connect(dc_id, is_media, &ctx.connector, peer)
            .await
            .map(|(reader, write, domain)| {
                Upstream::from_public_relay(reader, write, relay.clone(), dc_id, domain)
            }),
        None => None,
    }
}

#[allow(clippy::too_many_arguments)]
async fn connect_with_route_race(
    ctx: Arc<BridgeContext>,
    target_ip: &'static str,
    dc_id: u16,
    is_test_dc: bool,
    is_media: bool,
    ws_path: &'static str,
    peer: String,
    has_fallback: bool,
) -> Option<Upstream> {
    let mut attempts = JoinSet::new();

    let tcp_peer = peer.clone();
    attempts.spawn(async move {
        (
            RouteCandidate::DirectTcp,
            dial_tcp_dc(dc_id, is_test_dc, is_media, &tcp_peer).await,
        )
    });

    let direct_suppressed = {
        let mut health = lock_direct_health();
        health.direct_is_suppressed(Instant::now(), target_ip)
    };
    if direct_suppressed {
        logger::info(format!(
            "[{peer}] DC{dc_id}{} direct WSS skipped (cooldown)",
            if is_media { " media" } else { "" }
        ));
    } else {
        let direct_connector = ctx.connector.clone();
        let direct_peer = peer.clone();
        attempts.spawn(async move {
            tokio::time::sleep(WS_STAGGER).await;
            (
                RouteCandidate::TelegramWebSocket,
                dial_dc(
                    target_ip,
                    dc_id,
                    is_media,
                    ws_path,
                    &direct_connector,
                    &direct_peer,
                    has_fallback,
                )
                .await,
            )
        });
    }

    if has_fallback {
        let fallback_ctx = ctx;
        let fallback_peer = peer.clone();
        attempts.spawn(async move {
            tokio::time::sleep(FALLBACK_STAGGER).await;
            (
                RouteCandidate::Fallback,
                connect_fallback(&fallback_ctx, dc_id, is_test_dc, is_media, &fallback_peer).await,
            )
        });
    }

    let media_tag = if is_media { " media" } else { "" };
    let winner = select_route_winner(&mut attempts, |event| match event {
        RouteRaceEvent::Unavailable(candidate) => logger::info(format!(
            "[{peer}] DC{dc_id}{media_tag} route candidate unavailable: {}",
            candidate.label()
        )),
        RouteRaceEvent::TaskFailed(error) => logger::warn(format!(
            "[{peer}] DC{dc_id}{media_tag} route candidate task failed: {error}"
        )),
    })
    .await;

    if let Some((candidate, upstream)) = winner {
        logger::info(format!(
            "[{peer}] DC{dc_id}{media_tag} route race winner: {}",
            candidate.label()
        ));
        Some(upstream)
    } else {
        None
    }
}

async fn select_route_winner<T, F>(
    attempts: &mut JoinSet<(RouteCandidate, Option<T>)>,
    mut on_event: F,
) -> Option<(RouteCandidate, T)>
where
    T: Send + 'static,
    F: FnMut(RouteRaceEvent),
{
    while let Some(result) = attempts.join_next().await {
        match result {
            Ok((candidate, Some(value))) => {
                // `shutdown` both aborts and joins every loser, so no TCP/TLS
                // connection can outlive the route race in the background.
                attempts.shutdown().await;
                return Some((candidate, value));
            }
            Ok((candidate, None)) => on_event(RouteRaceEvent::Unavailable(candidate)),
            Err(error) if error.is_cancelled() => {}
            Err(error) => on_event(RouteRaceEvent::TaskFailed(error.to_string())),
        }
    }
    None
}

/// Чтение клиентского init с учётом опционального FakeTLS-слоя.
/// `None` — соединение уже обработано (редирект, проброс на домен
/// маскировки, обрыв) и задачу закрывать надо без разбора handshake.
#[allow(clippy::too_many_arguments)]
async fn read_client_init(
    mut raw_read: OwnedReadHalf,
    mut raw_write: OwnedWriteHalf,
    ctx: &BridgeContext,
    peer: &str,
) -> Option<(ClientRead, ClientWrite, [u8; obf2::HANDSHAKE_LEN])> {
    let mut first = [0u8; 1];
    match tokio::time::timeout(CLIENT_INIT_TIMEOUT, raw_read.read_exact(&mut first)).await {
        Ok(Ok(_)) => {}
        Ok(Err(_)) => {
            logger::info(format!("[{peer}] client disconnected before handshake"));
            return None;
        }
        Err(_) => {
            logger::warn(format!("[{peer}] handshake timeout"));
            return None;
        }
    }

    if let Some(masking) = ctx.masking.clone() {
        if first[0] == fake_tls::TLS_RECORD_HANDSHAKE {
            let tls_deadline = tokio::time::Instant::now() + CLIENT_INIT_TIMEOUT;
            let mut rest = [0u8; 4];
            match tokio::time::timeout_at(tls_deadline, raw_read.read_exact(&mut rest)).await {
                Ok(Ok(_)) => {}
                Ok(Err(_)) => {
                    logger::info(format!("[{peer}] incomplete TLS record header"));
                    return None;
                }
                Err(_) => {
                    logger::warn(format!("[{peer}] TLS record header timeout"));
                    return None;
                }
            }
            let record_len = u16::from_be_bytes([rest[2], rest[3]]) as usize;
            if record_len > MAX_FAKE_TLS_HELLO_LEN {
                logger::warn(format!(
                    "[{peer}] oversized TLS ClientHello record ({record_len} bytes)"
                ));
                return None;
            }
            let mut body = vec![0u8; record_len];
            match tokio::time::timeout_at(tls_deadline, raw_read.read_exact(&mut body)).await {
                Ok(Ok(_)) => {}
                Ok(Err(_)) => {
                    logger::info(format!("[{peer}] incomplete TLS record body"));
                    return None;
                }
                Err(_) => {
                    logger::warn(format!("[{peer}] TLS record body timeout"));
                    return None;
                }
            }
            let mut hello = Vec::with_capacity(5 + body.len());
            hello.push(first[0]);
            hello.extend_from_slice(&rest);
            hello.extend_from_slice(&body);

            match fake_tls::verify_client_hello(&hello, &ctx.secret) {
                None => {
                    logger::info(format!(
                        "[{peer}] Fake TLS verify failed -> masking {masking}"
                    ));
                    fake_tls::proxy_to_masking_domain(raw_read, raw_write, &hello, &masking, peer)
                        .await;
                    return None;
                }
                Some(verified) => {
                    let response = fake_tls::build_server_hello(
                        &ctx.secret,
                        &verified.client_random,
                        &verified.session_id,
                        &mut OsRng,
                    );
                    if let Err(error) = raw_write.write_all(&response).await {
                        logger::warn(format!("[{peer}] server hello write failed: {error}"));
                        return None;
                    }

                    let mut tls_reader = fake_tls::FakeTlsReader::new(raw_read);
                    let mut init = [0u8; obf2::HANDSHAKE_LEN];
                    match tokio::time::timeout(
                        CLIENT_INIT_TIMEOUT,
                        tls_reader.read_exact(&mut init),
                    )
                    .await
                    {
                        Ok(Ok(())) => {
                            return Some((
                                ClientRead::Tls(tls_reader),
                                ClientWrite::Tls(fake_tls::FakeTlsWriter::new(raw_write)),
                                init,
                            ));
                        }
                        Ok(Err(_)) => {
                            logger::info(format!("[{peer}] incomplete obfs2 init inside TLS"));
                            return None;
                        }
                        Err(_) => {
                            logger::warn(format!("[{peer}] obfs2 init timeout inside TLS"));
                            return None;
                        }
                    }
                }
            }
        }

        // Не-TLS первый байт при включённой маскировке — HTTP-редирект
        // на домен маскировки (как в апстриме).
        let redirect = format!(
            "HTTP/1.1 301 Moved Permanently\r\n\
             Location: https://{masking}/\r\n\
             Content-Length: 0\r\n\
             Connection: close\r\n\
             \r\n"
        );
        let _ = raw_write.write_all(redirect.as_bytes()).await;
        return None;
    }

    // Обычный путь: 64-байтный init без обёртки.
    let mut init = [0u8; obf2::HANDSHAKE_LEN];
    init[0] = first[0];
    match tokio::time::timeout(CLIENT_INIT_TIMEOUT, raw_read.read_exact(&mut init[1..])).await {
        Ok(Ok(_)) => Some((ClientRead::Raw(raw_read), ClientWrite::Raw(raw_write), init)),
        Ok(Err(_)) => {
            logger::info(format!("[{peer}] client disconnected before handshake"));
            None
        }
        Err(_) => {
            logger::warn(format!("[{peer}] handshake timeout"));
            None
        }
    }
}

async fn dial_tcp_dc(dc: u16, is_test_dc: bool, is_media: bool, peer: &str) -> Option<Upstream> {
    let media_tag = if is_media { " media" } else { "" };
    let endpoints = dc::dc_tcp_endpoints(dc, is_test_dc);
    if endpoints.is_empty() {
        logger::warn(format!(
            "[{peer}] DC{dc}{media_tag} has no raw TCP bootstrap endpoint"
        ));
        return None;
    }

    for &(ip, port) in endpoints {
        logger::info(format!(
            "[{peer}] DC{dc}{media_tag} -> tcp://{ip}:{port} (direct)"
        ));
        match tokio::time::timeout(TCP_CONNECT_TIMEOUT, TcpStream::connect((ip, port))).await {
            Ok(Ok(stream)) => return Some(Upstream::from_tcp(stream, "direct-tcp")),
            Ok(Err(error)) => logger::warn(format!(
                "[{peer}] DC{dc}{media_tag} direct TCP {ip}:{port} failed: {error}"
            )),
            Err(_) => logger::warn(format!(
                "[{peer}] DC{dc}{media_tag} direct TCP {ip}:{port} timed out"
            )),
        }
    }

    None
}

async fn dial_dc(
    ip: &str,
    dc: u16,
    is_media: bool,
    ws_path: &str,
    connector: &TlsConnector,
    peer: &str,
    enable_ip_cooldown: bool,
) -> Option<Upstream> {
    let route = DirectRoute { dc, is_media };
    let media_tag = if is_media { "m" } else { "" };
    let timeout = {
        let mut health = lock_direct_health();
        health.attempt_timeout(Instant::now(), ip, route)
    };
    let Some(timeout) = timeout else {
        logger::info(format!(
            "[{peer}] DC{dc}{media_tag} direct WS skipped (IP cooldown for {ip})"
        ));
        return None;
    };
    let timeout = direct_timeout_for_route(timeout, enable_ip_cooldown);
    if timeout == WS_RETRY_TIMEOUT {
        logger::info(format!(
            "[{peer}] DC{dc}{media_tag} direct WS retry with {:.0}s timeout",
            timeout.as_secs_f64()
        ));
    }

    let mut timed_out = false;
    for domain in dc::ws_domains(dc, is_media) {
        logger::info(format!(
            "[{peer}] DC{dc}{media_tag} -> wss://{domain}{ws_path} via {ip}"
        ));
        match ws::connect(ip, &domain, ws_path, timeout, connector).await {
            Ok(pair) => {
                lock_direct_health().record_success(ip, route);
                return Some(Upstream::from_websocket(pair.0, pair.1, "telegram-wss"));
            }
            Err(error) => {
                let is_timeout = matches!(&error, ws::ConnectError::Timeout);
                logger::warn(format!(
                    "[{peer}] DC{dc}{media_tag} WS connect failed via {domain}: {error}"
                ));
                if is_timeout {
                    timed_out = true;
                    // Both Telegram WS hostnames use the same destination IP.
                    // Retrying the second name after a TCP/TLS timeout only
                    // doubles the cold-start penalty and matches no useful
                    // route distinction.
                    break;
                }
            }
        }
    }
    if timed_out {
        lock_direct_health().record_timeout(Instant::now(), ip, route, enable_ip_cooldown);
        if enable_ip_cooldown {
            logger::info(format!(
                "[{peer}] DC{dc}{media_tag} direct WS timed out; {ip} enters cooldown"
            ));
        }
    } else {
        lock_direct_health().record_dc_failure(Instant::now(), route);
    }
    logger::warn(format!("[{peer}] DC{dc}{media_tag} direct WS unavailable"));
    None
}

/// Два насоса: client→DC и DC→client. Первый завершившийся гасит второй
/// и закрывает обе стороны (порт `bridge_ws_reencrypt`).
#[derive(Default)]
struct SessionStats {
    bytes: AtomicUsize,
    units: AtomicUsize,
}

impl SessionStats {
    fn record(&self, bytes: usize, units: usize) {
        self.bytes.fetch_add(bytes, AtomicOrdering::Relaxed);
        self.units.fetch_add(units, AtomicOrdering::Relaxed);
    }

    fn snapshot(&self) -> (usize, usize) {
        (
            self.bytes.load(AtomicOrdering::Relaxed),
            self.units.load(AtomicOrdering::Relaxed),
        )
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_session(
    client_read: ClientRead,
    client_write: ClientWrite,
    upstream: Upstream,
    ctx: CryptoCtx,
    splitter: MsgSplitter,
    peer: String,
    dc: u16,
    is_media: bool,
) {
    let CryptoCtx {
        clt_dec,
        clt_enc,
        tg_enc,
        tg_dec,
    } = ctx;
    let route = upstream.route();
    let relay_feedback = upstream.relay_feedback();
    let (upstream_read, upstream_write) = upstream.into_parts();

    let up_stats = Arc::new(SessionStats::default());
    let down_stats = Arc::new(SessionStats::default());
    let close_reason = Arc::new(std::sync::Mutex::new("normal".to_string()));
    let started = Instant::now();

    let mut up = tokio::spawn(pump_up(
        client_read,
        upstream_write.clone(),
        clt_dec,
        tg_enc,
        splitter,
        up_stats.clone(),
        close_reason.clone(),
    ));
    let mut down = tokio::spawn(pump_down(
        upstream_read,
        client_write,
        tg_dec,
        clt_enc,
        down_stats.clone(),
        close_reason.clone(),
    ));

    tokio::select! {
        _ = &mut up => {
            down.abort();
            let _ = down.await;
        }
        _ = &mut down => {
            up.abort();
            let _ = up.await;
        }
    }

    let (up_bytes, up_packets) = up_stats.snapshot();
    let (down_bytes, down_packets) = down_stats.snapshot();
    let reason = close_reason.lock().unwrap().clone();
    let media_tag = if is_media { "m" } else { "" };
    let elapsed = started.elapsed();
    logger::info(format!(
        "[{peer}] DC{dc}{media_tag} {route} session closed ({reason}): ^{up_bytes} B ({up_packets} units) v{down_bytes} B ({down_packets} units) in {:.1}s",
        elapsed.as_secs_f64()
    ));

    if let Some(feedback) = relay_feedback {
        match relay_session_verdict(elapsed, down_bytes, &reason) {
            RelaySessionVerdict::Success => feedback.success(),
            RelaySessionVerdict::Failure => {
                logger::warn(format!(
                    "[{peer}] DC{dc}{media_tag} public relay demoted after unstable session"
                ));
                feedback.failure();
            }
            RelaySessionVerdict::Neutral => {}
        }
    }

    upstream_write.close().await;
}

fn relay_session_verdict(
    elapsed: Duration,
    down_bytes: usize,
    close_reason: &str,
) -> RelaySessionVerdict {
    if elapsed >= RELAY_CONFIDENCE_TIME && down_bytes > 0 {
        RelaySessionVerdict::Success
    } else if close_reason.starts_with("upstream:") || down_bytes == 0 {
        RelaySessionVerdict::Failure
    } else {
        RelaySessionVerdict::Neutral
    }
}

async fn pump_up(
    mut read: ClientRead,
    upstream: UpstreamWriter,
    mut clt_dec: CtrCipher,
    mut tg_enc: CtrCipher,
    mut splitter: MsgSplitter,
    stats: Arc<SessionStats>,
    close_reason: Arc<std::sync::Mutex<String>>,
) {
    let framing = upstream.framing();
    let mut buf = vec![0u8; READ_CHUNK];
    let mut cipher_scratch = Vec::new();
    loop {
        match read.read(&mut buf).await {
            Ok(0) => {
                if let Some(tail) = splitter.flush() {
                    let _ = upstream.send_batch(vec![tail]).await;
                }
                return;
            }
            Ok(n) => {
                let chunk = &mut buf[..n];
                let result = match framing {
                    Framing::ByteStream => {
                        reencrypt_direct_chunk(&mut clt_dec, &mut tg_enc, chunk);
                        stats.record(n, 1);
                        Some(upstream.send(chunk).await)
                    }
                    Framing::TelegramMessages => {
                        clt_dec.apply(chunk);
                        cipher_scratch.clear();
                        cipher_scratch.extend_from_slice(chunk);
                        tg_enc.apply(&mut cipher_scratch);

                        let parts = splitter.split(chunk, &cipher_scratch);
                        stats.record(n, parts.len());
                        if parts.is_empty() {
                            None
                        } else {
                            Some(upstream.send_batch(parts).await)
                        }
                    }
                };
                if let Some(Err(error)) = result {
                    *close_reason.lock().unwrap() = format!("upstream: {error}");
                    return;
                }
            }
            Err(error) => {
                *close_reason.lock().unwrap() = format!("client: {error}");
                return;
            }
        }
    }
}

fn reencrypt_direct_chunk(clt_dec: &mut CtrCipher, tg_enc: &mut CtrCipher, chunk: &mut [u8]) {
    clt_dec.apply(chunk);
    tg_enc.apply(chunk);
}

async fn pump_down(
    mut upstream_read: UpstreamReader,
    mut client_write: ClientWrite,
    mut tg_dec: CtrCipher,
    mut clt_enc: CtrCipher,
    stats: Arc<SessionStats>,
    close_reason: Arc<std::sync::Mutex<String>>,
) {
    let mut data = Vec::new();
    loop {
        match upstream_read.recv_into(&mut data).await {
            Ok(Some(count)) => {
                let chunk = &mut data[..count];
                tg_dec.apply(chunk);
                clt_enc.apply(chunk);
                stats.record(count, 1);
                match tokio::time::timeout(SESSION_WRITE_TIMEOUT, client_write.write_all(chunk))
                    .await
                {
                    Ok(Ok(())) => {}
                    Ok(Err(error)) => {
                        *close_reason.lock().unwrap() = format!("client: {error}");
                        return;
                    }
                    Err(_) => {
                        *close_reason.lock().unwrap() = "client: write timed out".to_string();
                        return;
                    }
                }
            }
            Ok(None) => {
                let mut reason = close_reason.lock().unwrap();
                if *reason == "normal" {
                    *reason = "upstream: closed".to_string();
                }
                return;
            }
            Err(error) => {
                *close_reason.lock().unwrap() = format!("upstream: {error}");
                return;
            }
        }
    }
}

/// Вычитать остаток до закрытия клиента, но не дольше `DRAIN_TIMEOUT`.
async fn drain_and_close(mut read: ClientRead) {
    let _ = tokio::time::timeout(DRAIN_TIMEOUT, async {
        let mut sink = [0u8; 4096];
        loop {
            match read.read(&mut sink).await {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
        }
    })
    .await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    const NORMAL_DC2: DirectRoute = DirectRoute {
        dc: 2,
        is_media: false,
    };
    const MEDIA_DC2: DirectRoute = DirectRoute {
        dc: 2,
        is_media: true,
    };

    #[test]
    fn session_stats_accumulate_without_a_mutex() {
        let stats = SessionStats::default();
        stats.record(64, 1);
        stats.record(128, 3);
        assert_eq!(stats.snapshot(), (192, 4));
    }

    #[test]
    fn fresh_direct_route_uses_full_timeout() {
        let now = Instant::now();
        let mut health = DirectHealth::default();

        assert_eq!(
            health.attempt_timeout(now, "149.154.167.220", NORMAL_DC2),
            Some(WS_CONNECT_TIMEOUT)
        );
    }

    #[test]
    fn fallback_route_caps_direct_timeout_without_changing_direct_only() {
        assert_eq!(
            direct_timeout_for_route(WS_CONNECT_TIMEOUT, true),
            DIRECT_RACE_TIMEOUT
        );
        assert_eq!(
            direct_timeout_for_route(WS_RETRY_TIMEOUT, true),
            DIRECT_RACE_TIMEOUT
        );
        assert_eq!(
            direct_timeout_for_route(WS_CONNECT_TIMEOUT, false),
            WS_CONNECT_TIMEOUT
        );
    }

    #[test]
    fn worker_configuration_never_enables_a_fallback_route() {
        assert!(!fallback_is_available(false, false, true));
        assert!(fallback_is_available(false, true, false));
        assert!(fallback_is_available(false, true, true));
        assert!(!fallback_is_available(true, true, true));
    }

    #[tokio::test]
    async fn route_race_uses_first_success_after_an_unavailable_candidate() {
        let (release_tx, release_rx) = tokio::sync::oneshot::channel();
        let mut attempts = JoinSet::new();
        attempts.spawn(async { (RouteCandidate::DirectTcp, None) });
        attempts.spawn(async move {
            release_rx.await.expect("direct result must release WSS");
            (RouteCandidate::TelegramWebSocket, Some("telegram-wss"))
        });
        attempts.spawn(async {
            std::future::pending::<()>().await;
            (RouteCandidate::Fallback, Some("fallback"))
        });

        let mut events = Vec::new();
        let mut release_tx = Some(release_tx);
        let winner = select_route_winner(&mut attempts, |event| {
            if event == RouteRaceEvent::Unavailable(RouteCandidate::DirectTcp) {
                let _ = release_tx.take().expect("release only once").send(());
            }
            events.push(event);
        })
        .await;

        assert_eq!(
            winner,
            Some((RouteCandidate::TelegramWebSocket, "telegram-wss"))
        );
        assert_eq!(
            events,
            vec![RouteRaceEvent::Unavailable(RouteCandidate::DirectTcp)]
        );
        assert!(attempts.is_empty());
    }

    #[tokio::test]
    async fn route_race_joins_cancelled_losers_before_returning() {
        struct CancellationFlag(Arc<AtomicBool>);

        impl Drop for CancellationFlag {
            fn drop(&mut self) {
                self.0.store(true, Ordering::SeqCst);
            }
        }

        let cancelled = Arc::new(AtomicBool::new(false));
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let mut attempts = JoinSet::new();
        let loser_cancelled = cancelled.clone();
        attempts.spawn(async move {
            let _flag = CancellationFlag(loser_cancelled);
            let _ = started_tx.send(());
            std::future::pending::<()>().await;
            (RouteCandidate::Fallback, Some("loser"))
        });
        started_rx.await.expect("loser task must start");
        attempts.spawn(async { (RouteCandidate::DirectTcp, Some("winner")) });

        let winner = select_route_winner(&mut attempts, |_| {}).await;

        assert_eq!(winner, Some((RouteCandidate::DirectTcp, "winner")));
        assert!(cancelled.load(Ordering::SeqCst));
        assert!(attempts.is_empty());
    }

    #[tokio::test]
    async fn route_race_returns_none_when_every_candidate_is_unavailable() {
        let mut attempts = JoinSet::new();
        attempts.spawn(async { (RouteCandidate::DirectTcp, None::<()>) });
        attempts.spawn(async { (RouteCandidate::TelegramWebSocket, None::<()>) });

        let mut events = Vec::new();
        let winner = select_route_winner(&mut attempts, |event| events.push(event)).await;

        assert_eq!(winner, None);
        assert_eq!(events.len(), 2);
        assert!(events.iter().all(|event| matches!(
            event,
            RouteRaceEvent::Unavailable(RouteCandidate::DirectTcp)
                | RouteRaceEvent::Unavailable(RouteCandidate::TelegramWebSocket)
        )));
    }

    #[test]
    fn timeout_opens_global_ip_and_route_cooldowns() {
        let now = Instant::now();
        let mut health = DirectHealth::default();
        health.record_timeout(now, "149.154.167.220", NORMAL_DC2, true);

        assert_eq!(
            health.attempt_timeout(now, "149.154.167.220", NORMAL_DC2),
            None
        );
        assert_eq!(
            health.attempt_timeout(now, "149.154.167.220", MEDIA_DC2),
            None
        );
        assert_eq!(
            health.attempt_timeout(now, "149.154.171.5", NORMAL_DC2),
            Some(WS_RETRY_TIMEOUT)
        );
    }

    #[test]
    fn fallback_preference_skips_direct_until_probe_window() {
        let now = Instant::now();
        let mut health = DirectHealth::default();
        health.record_timeout(now, "149.154.167.220", NORMAL_DC2, true);

        assert!(health.direct_is_suppressed(now, "149.154.167.220"));
        assert!(!health.direct_is_suppressed(now + IP_FAIL_PROBE_INTERVAL, "149.154.167.220"));
    }

    #[test]
    fn cooldown_allows_one_periodic_probe() {
        let now = Instant::now();
        let mut health = DirectHealth::default();
        health.record_timeout(now, "149.154.167.220", NORMAL_DC2, true);

        let probe_at = now + IP_FAIL_PROBE_INTERVAL;
        assert_eq!(
            health.attempt_timeout(probe_at, "149.154.167.220", NORMAL_DC2),
            Some(WS_RETRY_TIMEOUT)
        );
        assert_eq!(
            health.attempt_timeout(probe_at, "149.154.167.220", MEDIA_DC2),
            None
        );
    }

    #[test]
    fn timeout_without_fallback_does_not_disable_the_ip() {
        let now = Instant::now();
        let mut health = DirectHealth::default();
        health.record_timeout(now, "149.154.167.220", NORMAL_DC2, false);

        assert_eq!(
            health.attempt_timeout(now, "149.154.167.220", NORMAL_DC2),
            Some(WS_RETRY_TIMEOUT)
        );
        assert_eq!(
            health.attempt_timeout(now, "149.154.167.220", MEDIA_DC2),
            Some(WS_CONNECT_TIMEOUT)
        );
    }

    #[test]
    fn dc_failure_shortens_only_matching_route_timeout() {
        let now = Instant::now();
        let mut health = DirectHealth::default();
        health.record_dc_failure(now, NORMAL_DC2);

        assert_eq!(
            health.attempt_timeout(now, "149.154.167.220", NORMAL_DC2),
            Some(WS_RETRY_TIMEOUT)
        );
        assert_eq!(
            health.attempt_timeout(now, "149.154.167.220", MEDIA_DC2),
            Some(WS_CONNECT_TIMEOUT)
        );
    }

    #[test]
    fn cooldowns_expire_without_wall_clock_waits() {
        let now = Instant::now();
        let mut health = DirectHealth::default();
        health.record_timeout(now, "149.154.167.220", NORMAL_DC2, true);

        assert_eq!(
            health.attempt_timeout(
                now + DC_FAIL_COOLDOWN + Duration::from_secs(1),
                "149.154.171.5",
                NORMAL_DC2,
            ),
            Some(WS_CONNECT_TIMEOUT)
        );
        assert_eq!(
            health.attempt_timeout(
                now + IP_FAIL_COOLDOWN + Duration::from_secs(1),
                "149.154.167.220",
                NORMAL_DC2,
            ),
            Some(WS_CONNECT_TIMEOUT)
        );
    }

    #[test]
    fn success_clears_ip_and_matching_route_health() {
        let now = Instant::now();
        let mut health = DirectHealth::default();
        health.record_timeout(now, "149.154.167.220", NORMAL_DC2, true);
        health.record_success("149.154.167.220", NORMAL_DC2);

        assert_eq!(
            health.attempt_timeout(now, "149.154.167.220", NORMAL_DC2),
            Some(WS_CONNECT_TIMEOUT)
        );
    }

    #[test]
    fn websocket_waits_for_a_complete_mtproto_packet() {
        let partial_plain = [0x02u8, 0xaa, 0xbb];
        let partial_cipher = [0x10, 0x20, 0x30];
        let mut websocket_splitter = MsgSplitter::new(Proto::Abridged);
        let websocket_parts = websocket_splitter.split(&partial_plain, &partial_cipher);
        assert!(websocket_parts.is_empty());
    }

    #[test]
    fn direct_reencryption_matches_the_copying_path_across_chunk_boundaries() {
        let client_key = [0x11; 32];
        let client_iv = [0x22; 16];
        let telegram_key = [0x33; 32];
        let telegram_iv = [0x44; 16];
        let input: Vec<u8> = (0..137)
            .map(|value| (value as u8).wrapping_mul(37))
            .collect();
        let boundaries = [1usize, 18, 65, input.len()];

        let mut old_client = CtrCipher::new(&client_key, &client_iv);
        let mut old_telegram = CtrCipher::new(&telegram_key, &telegram_iv);
        let mut expected = Vec::with_capacity(input.len());
        let mut start = 0;
        for end in boundaries {
            let mut plain = input[start..end].to_vec();
            old_client.apply(&mut plain);
            let mut cipher = plain.clone();
            old_telegram.apply(&mut cipher);
            expected.extend_from_slice(&cipher);
            start = end;
        }

        let mut actual = input;
        let mut new_client = CtrCipher::new(&client_key, &client_iv);
        let mut new_telegram = CtrCipher::new(&telegram_key, &telegram_iv);
        let mut start = 0;
        for end in boundaries {
            reencrypt_direct_chunk(&mut new_client, &mut new_telegram, &mut actual[start..end]);
            start = end;
        }

        assert_eq!(actual, expected);
    }

    #[test]
    fn relay_health_requires_a_stable_downstream_session() {
        assert_eq!(
            relay_session_verdict(Duration::from_secs(3), 0, "upstream: closed"),
            RelaySessionVerdict::Failure
        );
        assert_eq!(
            relay_session_verdict(Duration::from_secs(3), 100, "normal"),
            RelaySessionVerdict::Neutral
        );
        assert_eq!(
            relay_session_verdict(RELAY_CONFIDENCE_TIME, 100, "upstream: closed"),
            RelaySessionVerdict::Success
        );
    }
}
