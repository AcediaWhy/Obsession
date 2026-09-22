//! Legacy tests use the authenticated service, never an AppData executable.
//! The caller holds dpi_gate for the entire transaction.
use super::*;

const DISCORD_UPDATER: &str = "https://updates.discord.com/distributions/app/manifests/latest?channel=stable&platform=win&arch=x64";
const DISCORD_IMAGE: &str = "https://cdn.discordapp.com/embed/avatars/0.png";
const YOUTUBE_IMAGE: &str = "https://i.ytimg.com/vi/jNQXAC9IVRw/hqdefault.jpg";

#[derive(Debug, Serialize)]
pub struct ProbeResult {
    url: String,
    passed: bool,
    elapsed_ms: u64,
    bytes: usize,
    attempts: u8,
    http_status: Option<u16>,
    error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct TestReport {
    pub passed: bool,
    pub status: &'static str,
    checks: Vec<ProbeResult>,
}

fn report(checks: Vec<ProbeResult>, cancelled: bool) -> TestReport {
    let passed = !cancelled && !checks.is_empty() && checks.iter().all(|c| c.passed);
    let status = if cancelled { "cancelled" } else if passed { "passed" }
        else if checks.iter().any(|c| c.passed) { "partial" } else { "failed" };
    TestReport { passed, status, checks }
}

fn targets(category: &str) -> Result<&'static [&'static str], String> {
    match category {
        "discord" => Ok(&["https://discord.com/api/v10/gateway", DISCORD_UPDATER, "https://discord.com/", DISCORD_IMAGE]),
        "youtube_twitch" => Ok(&["https://www.youtube.com/", "https://i.ytimg.com/vi/jNQXAC9IVRw/hqdefault.jpg", "https://www.twitch.tv/"]),
        "gaming" => Ok(&["https://www.epicgames.com/", "https://signin.ea.com/"]),
        "universal" | "atrisk" => Ok(&["https://www.google.com/", "https://www.youtube.com/", "https://discord.com/"]),
        _ => Err("Неизвестная категория проверки DPI.".into()),
    }
}

fn cancelled(app: &AppHandle, epoch: u64) -> bool {
    let state = app.state::<AppState>();
    state.shutting_down.load(Ordering::SeqCst)
        || state.test_generation.load(Ordering::SeqCst) != epoch
        || state.test_cancel.load(Ordering::SeqCst)
}

async fn probe(url: &str) -> ProbeResult {
    let started = std::time::Instant::now();
    let mut result = ProbeResult { url: url.into(), passed: false, elapsed_ms: 0,
        bytes: 0, attempts: 0, http_status: None, error: None };
    for attempt in 1..=2 {
        result.attempts = attempt;
        result.http_status = None;
        let check = async {
        let client = reqwest::Client::builder()
        .no_proxy()
        .user_agent("Obsession-Diagnostics/1.1")
        .connect_timeout(Duration::from_secs(4))
        .timeout(Duration::from_secs(7))
        .redirect(reqwest::redirect::Policy::limited(3))
        .build().map_err(|e| format!("HTTP client: {e}"))?;
        let response = client.get(url).send().await.map_err(|e| format!("TLS/HTTP: {e:?}"))?;
        result.http_status = Some(response.status().as_u16());
        validate_response(url, response).await
        }.await;
        match check {
            Ok(bytes) => { result.bytes = bytes; result.passed = true; result.error = None; break; }
            Err(error) => result.error = Some(error),
        }
        // One retry for a transport/body failure or a server error, not a 403/404.
        if attempt == 2 || result.http_status.is_some_and(|code| (400..500).contains(&code)) { break; }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    result.elapsed_ms = started.elapsed().as_millis() as u64;
    result
}

async fn validate_response(url: &str, mut response: reqwest::Response) -> Result<usize, String> {
    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status().as_u16()));
    }
    if url == "https://discord.com/api/v10/gateway" || url == DISCORD_UPDATER {
        // Do not let a redirect to an unrelated HTML page evade strict JSON
        // validation, which dispatches by the final response host.
        let expected_host = reqwest::Url::parse(url).map_err(|e| e.to_string())?;
        if response.url().scheme() != "https" || response.url().host_str() != expected_host.host_str() {
            return Err("Discord API redirected outside its expected HTTPS host".into());
        }
        return crate::service_health::read_response(response, Duration::from_secs(7))
            .await;
    }
    let image = url == DISCORD_IMAGE || url == YOUTUBE_IMAGE;
    if image {
        let expected = reqwest::Url::parse(url).map_err(|e| e.to_string())?;
        if response.url().scheme() != "https" || response.url().host_str() != expected.host_str() {
            return Err("Image redirected outside its expected HTTPS host".into());
        }
    }
    // Read actual content, not just HTTP headers. Bound work and memory.
    let limit = if image { 256 * 1024 } else { 16 * 1024 };
    let mut body = Vec::new();
    loop {
        match response.chunk().await {
            Ok(Some(chunk)) => {
                if image && chunk.len() > limit - body.len() { return Err("Image exceeds probe size limit".into()); }
                body.extend_from_slice(&chunk[..chunk.len().min(limit - body.len())]);
                if !image && body.len() == limit { break; }
            }
            Ok(None) => break,
            Err(error) => return Err(format!("response body after {} bytes: {error:?}", body.len())),
        }
    }
    if image { validate_image(url, &body)?; }
    if !body.is_empty() { Ok(body.len()) } else { Err("empty response body".into()) }
}

fn validate_image(url: &str, body: &[u8]) -> Result<(), String> {
    let valid = if url == DISCORD_IMAGE {
        body.starts_with(b"\x89PNG\r\n\x1a\n") && body.ends_with(b"IEND\xaeB`\x82")
    } else { body.starts_with(&[0xff, 0xd8, 0xff]) && body.ends_with(&[0xff, 0xd9]) };
    if valid { Ok(()) } else { Err("Response is not a complete expected PNG/JPEG image".into()) }
}

pub(crate) async fn run(app: &AppHandle, category: &str, config: &str) -> Result<TestReport, String> {
    let urls = targets(category)?;
    #[cfg(windows)]
    {
        let state = app.state::<AppState>();
        state.test_cancel.store(false, Ordering::SeqCst);
        let epoch = state.test_generation.fetch_add(1, Ordering::SeqCst) + 1;
        let request = start_request(DpiEngine::Legacy,
            &[(category.to_owned(), config.to_owned())], 0, false, &BTreeMap::new())?;
        // Check the authoritative runtime; never stop an existing user session.
        let response = tauri::async_runtime::spawn_blocking(move || {
            if runtime_snapshot_blocking(OPERATION_TIMEOUT)?.dpi.is_some() {
                return Err("Перед тестом выключите защиту: служба сообщает об активной сессии.".into());
            }
            call_service("dpi-start", request, OPERATION_TIMEOUT)
        }).await.map_err(|e| e.to_string())??;
        let generation = match response {
            Response::Started(started) => started.generation,
            _ => return Err("Неожиданный ответ службы при запуске теста.".into()),
        };
        let check = async {
            tokio::time::sleep(Duration::from_millis(800)).await;
            let mut checks = Vec::new();
            for url in urls {
                let result = probe(url).await;
                util::emit_log(app, if result.passed { "info" } else { "warn" }, "dpi",
                    &format!("{config}: {url}: {}; {} мс; {} байт; попыток {}; {}",
                        if result.passed { "OK" } else { "не подтверждено" }, result.elapsed_ms,
                        result.bytes, result.attempts, result.error.as_deref().unwrap_or("")));
                checks.push(result);
            }
            checks
        };
        let cancel = async {
            loop {
                if cancelled(app, epoch) { break; }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        };
        let (checks, was_cancelled) = tokio::select! {
            result = check => (result, false),
            _ = cancel => (Vec::new(), true),
        };
        // Stop exactly the generation returned by this start, not a fresh
        // snapshot which could belong to another client. Cleanup errors abort
        // the entire picker; it must not continue against an unknown runtime.
        tauri::async_runtime::spawn_blocking(move || {
            match call_service("dpi-stop", Request::DpiStop(DpiStopRequest { generation }), OPERATION_TIMEOUT)? {
                Response::Stopped => Ok(()),
                _ => Err("Служба не подтвердила завершение тестовой сессии.".to_string()),
            }
        }).await.map_err(|e| e.to_string())??;
        Ok(report(checks, was_cancelled || cancelled(app, epoch)))
    }
    #[cfg(not(windows))]
    { let _ = (app, config, urls); Err(unavailable()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Explicit opt-in smoke check; never starts/stops DPI or changes settings.
    #[tokio::test]
    #[ignore = "live public endpoints; requires an explicitly requested network diagnostic"]
    async fn live_media_probe_without_runtime_changes() {
        for url in [DISCORD_IMAGE, YOUTUBE_IMAGE] {
            println!("{}", serde_json::to_string(&probe(url).await).unwrap());
        }
    }
    fn evidence(passed: bool) -> ProbeResult {
        ProbeResult { url: "https://example.com".into(), passed, elapsed_ms: 20,
            bytes: 0, attempts: 1, http_status: None, error: None }
    }

    #[test]
    fn partial_and_cancelled_reports_never_confirm_a_config() {
        assert_eq!(report(vec![evidence(true), evidence(false)], false).status, "partial");
        assert!(!report(vec![evidence(true), evidence(false)], false).passed);
        assert_eq!(report(vec![evidence(false)], false).status, "failed");
        assert!(!report(vec![], false).passed);
        assert_eq!(report(vec![evidence(true)], true).status, "cancelled");
        assert!(!report(vec![evidence(true)], true).passed);
        assert!(report(vec![evidence(true), evidence(true)], false).passed);
    }

    #[test]
    fn cdn_probe_uses_public_image_and_rejects_html_and_truncation() {
        assert!(targets("discord").unwrap().contains(&DISCORD_IMAGE));
        assert!(!targets("discord").unwrap().contains(&"https://cdn.discordapp.com/"));
        assert!(validate_image(DISCORD_IMAGE, b"<html>Access denied</html>").is_err());
        assert!(validate_image(DISCORD_IMAGE, b"\x89PNG\r\n\x1a\ntruncated").is_err());
        assert!(validate_image(YOUTUBE_IMAGE, &[0xff, 0xd8, 0xff, 0]).is_err());
    }

    #[tokio::test]
    async fn probe_retries_server_failure_and_retains_measurements() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let worker = std::thread::spawn(move || {
            for status in [503, 200] {
                let (mut socket, _) = listener.accept().unwrap();
                socket.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
                socket.read(&mut [0; 2048]).unwrap();
                write!(socket, "HTTP/1.1 {status} Test\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok").unwrap();
            }
        });
        let result = probe(&format!("http://{address}/")).await;
        worker.join().unwrap();
        assert!(result.passed);
        assert_eq!(result.attempts, 2);
        assert_eq!(result.bytes, 2);
        assert_eq!(result.http_status, Some(200));
        assert!(result.elapsed_ms >= 250);
        assert!(result.error.is_none());
    }
    #[test]
    fn discord_picker_requires_the_platform_specific_updater_manifest() {
        let urls = targets("discord").unwrap();
        assert!(urls.contains(&DISCORD_UPDATER));
        let updater = reqwest::Url::parse(DISCORD_UPDATER).unwrap();
        assert_eq!(updater.host_str(), Some("updates.discord.com"));
        assert_eq!(format!("{}?{}", updater.path(), updater.query().unwrap()), crate::service_health::DISCORD_UPDATE_PATH);
        assert!(urls.contains(&"https://discord.com/"));
        assert!(urls.contains(&"https://discord.com/api/v10/gateway"));
    }

    #[tokio::test]
    async fn discord_api_cannot_pass_via_an_unrelated_response_host() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let worker = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
            let mut request = [0; 2048];
            socket.read(&mut request).unwrap();
            socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}").unwrap();
        });
        let response = reqwest::Client::builder().no_proxy().build().unwrap()
            .get(format!("http://{address}/")).timeout(Duration::from_secs(2)).send().await.unwrap();
        assert!(validate_response(DISCORD_UPDATER, response).await.unwrap_err().contains("expected HTTPS host"));
        worker.join().unwrap();
    }
    #[tokio::test]
    async fn empty_cdn_404_is_valid_but_other_errors_and_empty_pages_are_not() {
        use std::io::{Read, Write};
        for (url, status, expected) in [
            (DISCORD_IMAGE, 404, false),
            ("https://cdn.discordapp.com/", 503, false),
            ("https://www.youtube.com/", 404, false),
            ("https://www.youtube.com/", 200, false),
        ] {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let address = listener.local_addr().unwrap();
            let worker = std::thread::spawn(move || {
                let (mut socket, _) = listener.accept().unwrap();
                socket.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
                let mut request = [0; 2048];
                socket.read(&mut request).unwrap();
                write!(socket, "HTTP/1.1 {status} Test\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
            });
            let response = reqwest::Client::builder().no_proxy().build().unwrap()
                .get(format!("http://{address}/")).timeout(Duration::from_secs(2))
                .send().await.unwrap();
            assert_eq!(validate_response(url, response).await.is_ok(), expected);
            worker.join().unwrap();
        }
    }
    #[test]
    fn youtube_cannot_be_replaced_by_twitch_success() {
        let urls = targets("youtube_twitch").unwrap();
        assert!(urls.contains(&"https://www.youtube.com/"));
        assert!(urls.iter().any(|url| url.contains("ytimg.com")));
        assert!(urls.contains(&"https://www.twitch.tv/"));
    }
    #[test]
    fn discord_requires_api_and_cdn_and_unknown_categories_fail_closed() {
        assert_eq!(targets("discord").unwrap().len(), 4);
        assert!(targets("unknown").is_err());
    }
    #[cfg(windows)]
    #[test]
    fn test_start_disables_recovery_without_mutating_user_settings() {
        let request = start_request(DpiEngine::Legacy,
            &[("discord".into(), "discord_1.conf".into())], 0, false, &BTreeMap::new()).unwrap();
        let Request::DpiStart(request) = request else { panic!("expected start"); };
        assert!(!request.options.legacy_reliability);
        assert!(request.options.zapret2_overrides.is_empty());
    }
}
