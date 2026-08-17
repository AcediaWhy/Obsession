//! Обработка одного клиентского соединения: опциональный FakeTLS-слой,
//! obfuscated2-handshake, подключение к DC и двунаправленное
//! перешифрование (порт `_handle_client` + `bridge_ws_reencrypt`).

use std::collections::HashMap;
use std::io;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

use rand::rngs::OsRng;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;

use crate::cfrelay::CfRelay;
use crate::crypto::CtrCipher;
use crate::dc;
use crate::fake_tls;
use crate::logger;
use crate::obf2::{self, CryptoCtx, ProtoTag};
use crate::splitter::{MsgSplitter, Proto};
use crate::ws;

const CLIENT_INIT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
const DRAIN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
const WS_CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
const WS_RETRY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);
const IP_FAIL_COOLDOWN: std::time::Duration = std::time::Duration::from_secs(60 * 60);
const DC_FAIL_COOLDOWN: std::time::Duration = std::time::Duration::from_secs(60);
const READ_CHUNK: usize = 65536;
const MAX_FAKE_TLS_HELLO_LEN: usize = 18 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct DirectRoute {
    dc: u16,
    is_media: bool,
}

#[derive(Default)]
struct DirectHealth {
    ip_fail_until: HashMap<String, Instant>,
    dc_fail_until: HashMap<DirectRoute, Instant>,
}

impl DirectHealth {
    fn attempt_timeout(&mut self, now: Instant, ip: &str, route: DirectRoute) -> Option<Duration> {
        self.ip_fail_until.retain(|_, until| *until > now);
        self.dc_fail_until.retain(|_, until| *until > now);

        if self.ip_fail_until.contains_key(ip) {
            return None;
        }
        if self.dc_fail_until.contains_key(&route) {
            return Some(WS_RETRY_TIMEOUT);
        }
        Some(WS_CONNECT_TIMEOUT)
    }

    fn record_timeout(&mut self, now: Instant, ip: &str, route: DirectRoute) {
        self.ip_fail_until
            .insert(ip.to_string(), now + IP_FAIL_COOLDOWN);
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

    let direct = if ctx.no_direct {
        // Отладочный режим --no-direct: сразу в фолбэк.
        None
    } else {
        dial_dc(
            target_ip,
            dc_id,
            parsed.is_media,
            ws_path,
            &ctx.connector,
            &peer,
            media_tag,
        )
        .await
    };
    let (ws_read, ws_write) = match direct {
        Some(pair) => pair,
        None => {
            // Прямой путь недоступен. Порядок фолбэка как в апстриме:
            // свой CF worker, затем публичные relay (для тестовых DC —
            // только worker).
            let worker = if ctx.worker_domains.is_empty() {
                None
            } else {
                crate::cfrelay::connect_worker(
                    &ctx.worker_domains,
                    dc_id,
                    parsed.is_media,
                    &ctx.connector,
                    &peer,
                )
                .await
            };
            match worker {
                Some((reader, write, _)) => (reader, write),
                None => {
                    if is_test_dc {
                        logger::warn(format!(
                            "[{peer}] DC{dc_id}{media_tag} no fallback available (test DC)"
                        ));
                        return;
                    }
                    let fallback = match ctx.cf_relay.as_ref() {
                        Some(relay) => {
                            relay
                                .connect(dc_id, parsed.is_media, &ctx.connector, &peer)
                                .await
                        }
                        None => None,
                    };
                    match fallback {
                        Some((reader, write, _)) => (reader, write),
                        None => {
                            logger::warn(format!(
                                "[{peer}] DC{dc_id}{media_tag} no fallback available"
                            ));
                            return;
                        }
                    }
                }
            }
        }
    };

    if let Err(error) = ws_write.send(&relay_init).await {
        logger::warn(format!("[{peer}] relay init send failed: {error}"));
        return;
    }

    run_session(
        client_read,
        client_write,
        ws_read,
        ws_write,
        crypto,
        splitter,
        peer,
        dc_id,
        parsed.is_media,
    )
    .await;
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

async fn dial_dc(
    ip: &str,
    dc: u16,
    is_media: bool,
    ws_path: &str,
    connector: &TlsConnector,
    peer: &str,
    media_tag: &str,
) -> Option<(ws::WsReader, Arc<ws::WsWriter>)> {
    let route = DirectRoute { dc, is_media };
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
                return Some(pair);
            }
            Err(error) => {
                let is_timeout = matches!(&error, ws::ConnectError::Timeout);
                logger::warn(format!(
                    "[{peer}] DC{dc}{media_tag} WS connect failed via {domain}: {error}"
                ));
                if is_timeout {
                    lock_direct_health().record_timeout(Instant::now(), ip, route);
                    logger::info(format!(
                        "[{peer}] DC{dc}{media_tag} direct WS timed out; {ip} enters cooldown"
                    ));
                    timed_out = true;
                    break;
                }
            }
        }
    }
    if !timed_out {
        lock_direct_health().record_dc_failure(Instant::now(), route);
    }
    logger::warn(format!("[{peer}] DC{dc}{media_tag} direct WS unavailable"));
    None
}

/// Два насоса: client→DC и DC→client. Первый завершившийся гасит второй
/// и закрывает обе стороны (порт `bridge_ws_reencrypt`).
#[allow(clippy::too_many_arguments)]
async fn run_session(
    client_read: ClientRead,
    client_write: ClientWrite,
    ws_read: ws::WsReader,
    ws_write: Arc<ws::WsWriter>,
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

    let up_stats = Arc::new(std::sync::Mutex::new((0usize, 0usize)));
    let down_stats = Arc::new(std::sync::Mutex::new((0usize, 0usize)));
    let close_reason = Arc::new(std::sync::Mutex::new("normal".to_string()));
    let started = Instant::now();

    let mut up = tokio::spawn(pump_up(
        client_read,
        ws_write.clone(),
        clt_dec,
        tg_enc,
        splitter,
        up_stats.clone(),
        close_reason.clone(),
    ));
    let mut down = tokio::spawn(pump_down(
        ws_read,
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

    let (up_bytes, up_packets) = *up_stats.lock().unwrap();
    let (down_bytes, down_packets) = *down_stats.lock().unwrap();
    let reason = close_reason.lock().unwrap().clone();
    let media_tag = if is_media { "m" } else { "" };
    logger::info(format!(
        "[{peer}] DC{dc}{media_tag} WS session closed ({reason}): ^{up_bytes} B ({up_packets} pkts) v{down_bytes} B ({down_packets} pkts) in {:.1}s",
        started.elapsed().as_secs_f64()
    ));

    ws_write.close().await;
}

async fn pump_up(
    mut read: ClientRead,
    ws: Arc<ws::WsWriter>,
    mut clt_dec: CtrCipher,
    mut tg_enc: CtrCipher,
    mut splitter: MsgSplitter,
    stats: Arc<std::sync::Mutex<(usize, usize)>>,
    close_reason: Arc<std::sync::Mutex<String>>,
) {
    let mut buf = vec![0u8; READ_CHUNK];
    loop {
        match read.read(&mut buf).await {
            Ok(0) => {
                if let Some(tail) = splitter.flush() {
                    let _ = ws.send(&tail).await;
                }
                return;
            }
            Ok(n) => {
                let chunk = &mut buf[..n];
                clt_dec.apply(chunk);
                let mut cipher = chunk.to_vec();
                tg_enc.apply(&mut cipher);
                let parts = splitter.split(chunk, &cipher);
                if parts.is_empty() {
                    continue;
                }
                {
                    let mut stats = stats.lock().unwrap();
                    stats.0 += n;
                    stats.1 += parts.len();
                }
                let result = if parts.len() > 1 {
                    ws.send_batch(&parts).await
                } else {
                    ws.send(&parts[0]).await
                };
                if let Err(error) = result {
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

async fn pump_down(
    mut ws_read: ws::WsReader,
    mut client_write: ClientWrite,
    mut tg_dec: CtrCipher,
    mut clt_enc: CtrCipher,
    stats: Arc<std::sync::Mutex<(usize, usize)>>,
    close_reason: Arc<std::sync::Mutex<String>>,
) {
    loop {
        match ws_read.recv().await {
            Some(mut data) => {
                tg_dec.apply(&mut data);
                clt_enc.apply(&mut data);
                {
                    let mut stats = stats.lock().unwrap();
                    stats.0 += data.len();
                    stats.1 += 1;
                }
                if let Err(error) = client_write.write_all(&data).await {
                    *close_reason.lock().unwrap() = format!("client: {error}");
                    return;
                }
            }
            None => {
                let mut reason = close_reason.lock().unwrap();
                if *reason == "normal" {
                    *reason = "upstream: ws_close".to_string();
                }
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

    const NORMAL_DC2: DirectRoute = DirectRoute {
        dc: 2,
        is_media: false,
    };
    const MEDIA_DC2: DirectRoute = DirectRoute {
        dc: 2,
        is_media: true,
    };

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
    fn timeout_opens_global_ip_and_route_cooldowns() {
        let now = Instant::now();
        let mut health = DirectHealth::default();
        health.record_timeout(now, "149.154.167.220", NORMAL_DC2);

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
        health.record_timeout(now, "149.154.167.220", NORMAL_DC2);

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
        health.record_timeout(now, "149.154.167.220", NORMAL_DC2);
        health.record_success("149.154.167.220", NORMAL_DC2);

        assert_eq!(
            health.attempt_timeout(now, "149.154.167.220", NORMAL_DC2),
            Some(WS_CONNECT_TIMEOUT)
        );
    }
}
