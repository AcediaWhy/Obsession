//! A bounded real-player trial, separate from the routing-verified tournament.
//! CDP supplies fresh network bytes; decoded frames and continuous media time
//! supply playback evidence. Neither establishes which winws2 block acted.
use super::zapret2_measure::{FunctionEvidence, Outcome, RoundMeasurement};
use super::zapret2_scenario::{unix_ms, Progress};
use crate::util::LockExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

pub const VERSION: &str = "youtube-real-player-v1";
pub const TIMEOUT: Duration = Duration::from_secs(60);
const WATCH: &str = "https://www.youtube.com/watch?v=jNQXAC9IVRw";
const VIDEO_ID: &str = "jNQXAC9IVRw";
const NETWORK_BUDGET: u64 = 16 * 1024 * 1024;
const MIN_MEDIA_BYTES: u64 = 32 * 1024;
const PLAY_SECONDS: f64 = 10.0;
const STALL_LIMIT: f64 = 4.0;
const SCRIPT: &str = include_str!("youtube_playback.js");

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Metrics {
    pub video_id: &'static str,
    pub network_bytes: u64,
    pub media_bytes: u64,
    pub media_responses: usize,
    pub cached_responses: usize,
    pub first_media_ms: Option<u64>,
    pub first_playback_ms: Option<u64>,
    pub playback_seconds: f64,
    pub stall_seconds: f64,
    pub decoded_frames: u64,
    pub dropped_frames: u64,
    pub width: u32,
    pub height: u32,
    pub media_protocols: Vec<String>,
    pub seek_confirmed: bool,
    pub stop_reason: String,
}

#[derive(Default)]
struct Media {
    fresh: bool,
    accepted: bool,
    bytes: u64,
    protocol: String,
}

#[derive(Default)]
struct Network {
    media: BTreeMap<String, Media>,
    cached: BTreeSet<String>,
    bytes: u64,
    first_media_ms: Option<u64>,
    unavailable: bool,
    overflow: bool,
}

fn media_url(value: &str) -> bool {
    reqwest::Url::parse(value).ok().is_some_and(|url| {
        url.scheme() == "https"
            && url.port_or_known_default() == Some(443)
            && url.username().is_empty()
            && url.password().is_none()
            && url
                .host_str()
                .is_some_and(|host| host.ends_with(".googlevideo.com"))
            && url.path() == "/videoplayback"
    })
}

fn byte_count(value: &Value) -> u64 {
    value
        .as_f64()
        .filter(|v| v.is_finite() && *v >= 0.0)
        .unwrap_or_default()
        .min(u64::MAX as f64) as u64
}

impl Network {
    // Only bounded metadata is retained. Signed media URLs, cookies and response
    // bodies never cross into progress, logs or the local history archive.
    fn event(&mut self, kind: &str, value: &Value, elapsed_ms: u64) {
        let Some(id) = value["requestId"].as_str() else {
            return;
        };
        match kind {
            "Network.requestServedFromCache" => {
                if self.cached.len() < 256 {
                    self.cached.insert(id.into());
                } else {
                    self.overflow = true;
                }
                if let Some(media) = self.media.get_mut(id) {
                    media.fresh = false;
                }
            }
            "Network.responseReceived" => {
                let response = &value["response"];
                if !media_url(response["url"].as_str().unwrap_or_default()) {
                    return;
                }
                if self.media.len() >= 256 {
                    self.overflow = true;
                    return;
                }
                let status = byte_count(&response["status"]);
                let mime = response["mimeType"].as_str().unwrap_or_default();
                let binary = mime.starts_with("video/")
                    || mime.starts_with("audio/")
                    || matches!(mime, "application/octet-stream" | "application/vnd.yt-ump");
                let fresh = !self.cached.contains(id)
                    && !["fromDiskCache", "fromPrefetchCache", "fromServiceWorker"]
                        .iter()
                        .any(|key| response[*key].as_bool() == Some(true));
                self.unavailable |= matches!(status, 403 | 410 | 429 | 500..=599);
                self.media.insert(
                    id.into(),
                    Media {
                        fresh,
                        accepted: matches!(status, 200 | 206) && binary,
                        protocol: response["protocol"]
                            .as_str()
                            .unwrap_or("unknown")
                            .chars()
                            .take(16)
                            .collect(),
                        ..Media::default()
                    },
                );
            }
            "Network.dataReceived" => {
                let bytes = byte_count(&value["encodedDataLength"]);
                self.bytes = self.bytes.saturating_add(bytes);
                if let Some(media) = self.media.get_mut(id) {
                    media.bytes = media.bytes.saturating_add(bytes);
                    if media.fresh && media.accepted && bytes > 0 {
                        self.first_media_ms.get_or_insert(elapsed_ms);
                    }
                }
            }
            "Network.loadingFinished" => {
                // Some Chromium builds deliver all encoded bytes at completion.
                if let Some(media) = self.media.get_mut(id) {
                    let completed = byte_count(&value["encodedDataLength"]);
                    self.bytes = self
                        .bytes
                        .saturating_add(completed.saturating_sub(media.bytes));
                    media.bytes = media.bytes.max(completed);
                    if media.fresh && media.accepted && media.bytes > 0 {
                        self.first_media_ms.get_or_insert(elapsed_ms);
                    }
                }
            }
            _ => {}
        }
    }
    fn snapshot(&self, metrics: &mut Metrics) {
        metrics.network_bytes = self.bytes;
        metrics.media_bytes = self
            .media
            .values()
            .filter(|m| m.fresh && m.accepted)
            .map(|m| m.bytes)
            .sum();
        metrics.media_responses = self
            .media
            .values()
            .filter(|m| m.fresh && m.accepted && m.bytes > 0)
            .count();
        metrics.cached_responses = self.media.values().filter(|m| !m.fresh).count();
        metrics.first_media_ms = self.first_media_ms;
        metrics.media_protocols = self
            .media
            .values()
            .filter(|m| m.fresh && m.accepted && m.bytes > 0)
            .map(|m| m.protocol.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
    }
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct Sample {
    identity: String,
    ad: bool,
    error: bool,
    present: bool,
    time: f64,
    duration: f64,
    ready: u8,
    paused: bool,
    ended: bool,
    width: u32,
    height: u32,
    frames: u64,
    dropped: u64,
    source: String,
}
#[derive(Default)]
struct Playback {
    last: Option<(Sample, f64)>,
    seconds: f64,
    stall: f64,
    frames: u64,
    seek_started: Option<(f64, f64)>,
    seek_seconds: f64,
    seek_frames: u64,
    seek_confirmed: bool,
}
impl Playback {
    fn observe(&mut self, sample: Sample, wall: f64, metrics: &mut Metrics) {
        if !sample.present || sample.identity != VIDEO_ID || sample.ad || sample.error {
            // Discontinuities cannot borrow playback time from ads or another video.
            self.last = None;
            return;
        }
        metrics.width = sample.width;
        metrics.height = sample.height;
        metrics.dropped_frames = metrics.dropped_frames.max(sample.dropped);
        if let Some((previous, at)) = self.last.take() {
            let elapsed = wall - at;
            let delta = sample.time - previous.time;
            let frames = sample.frames.saturating_sub(previous.frames);
            let continuous = elapsed > 0.0
                && elapsed <= 2.5
                && delta > 0.0
                && delta <= elapsed * 1.3 + 0.15
                && previous.source == sample.source;
            if continuous
                && frames > 0
                && sample.ready >= 2
                && sample.width > 0
                && sample.height > 0
            {
                self.seconds += delta.min(elapsed);
                self.frames += frames;
                metrics
                    .first_playback_ms
                    .get_or_insert((wall * 1000.0) as u64);
                if let Some((target, started)) = self.seek_started {
                    if previous.time >= target - 0.5 && wall > started {
                        self.seek_seconds += delta.min(elapsed);
                        self.seek_frames += frames;
                        self.seek_confirmed = self.seek_seconds >= 2.0 && self.seek_frames >= 10;
                    }
                }
            } else if self.seconds > 0.0
                && elapsed > 0.0
                && elapsed <= 2.5
                && !sample.ended
                && !sample.paused
                && delta.abs() < 0.05
            {
                self.stall += elapsed;
            }
        }
        metrics.playback_seconds = self.seconds;
        metrics.stall_seconds = self.stall;
        metrics.decoded_frames = self.frames;
        metrics.seek_confirmed = self.seek_confirmed;
        self.last = Some((sample, wall));
    }
    fn seek_target(&self) -> Option<f64> {
        let (sample, _) = self.last.as_ref()?;
        (self.seconds >= PLAY_SECONDS
            && self.frames >= 30
            && self.seek_started.is_none()
            && sample.duration.is_finite()
            && sample.duration >= 16.0)
            .then(|| sample.duration * 0.7)
    }
    fn passed(&self, metrics: &Metrics) -> bool {
        self.seconds >= PLAY_SECONDS
            && self.frames >= 30
            && self.stall <= STALL_LIMIT
            && self.seek_confirmed
            && metrics.media_bytes >= MIN_MEDIA_BYTES
            && metrics.media_responses > 0
    }
}

fn functions(
    metrics: &Metrics,
    outcome: Outcome,
    reason: Option<&str>,
    elapsed: u64,
) -> Vec<FunctionEvidence> {
    [
        ("media_download", "Загрузка видеоданных"),
        ("media_playback", "Воспроизведение и перемотка"),
    ]
    .into_iter()
    .map(|(id, label)| {
        let download_passed = id == "media_download"
            && metrics.media_bytes >= MIN_MEDIA_BYTES
            && metrics.media_responses > 0
            && metrics.decoded_frames > 0
            && matches!(outcome, Outcome::Failed | Outcome::Inconclusive);
        FunctionEvidence {
            function: id.into(),
            label: label.into(),
            required: true,
            outcome: if download_passed {
                Outcome::Passed
            } else {
                outcome
            },
            proof: if outcome == Outcome::Passed {
                "browser_media_and_decoded_frames"
            } else if download_passed {
                "browser_media_network"
            } else {
                "none"
            },
            stage: "player_trial",
            reason: if download_passed {
                None
            } else {
                reason.map(str::to_owned)
            },
            target: WATCH.into(),
            remote_address: None,
            ip_family: None,
            elapsed_ms: elapsed,
            bytes: if id == "media_download" {
                metrics.media_bytes as usize
            } else {
                0
            },
            transfer_bytes: if id == "media_download" {
                metrics.media_bytes as usize
            } else {
                0
            },
            transfer_ms: None,
            tls_handshakes: vec![],
            http_status: None,
            connections: vec![],
        }
    })
    .collect()
}

pub(super) fn pending(round: &mut RoundMeasurement) {
    round.functions = functions(
        &Metrics::default(),
        Outcome::NotTested,
        Some("Плеер ещё не проверен."),
        0,
    );
}

#[cfg(windows)]
mod native {
    use super::*;
    use std::cell::RefCell;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, Ordering};
    use tokio::sync::oneshot;
    use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2DevToolsProtocolEventReceiver;
    use webview2_com::{
        CallDevToolsProtocolMethodCompletedHandler, DevToolsProtocolEventReceivedEventHandler,
    };
    use windows_core::PCWSTR;

    // COM receivers belong to the UI apartment. Retain them until this window
    // is destroyed instead of relying on temporary subscription objects.
    thread_local! {
        static RECEIVERS: RefCell<BTreeMap<String, Vec<ICoreWebView2DevToolsProtocolEventReceiver>>> = RefCell::new(BTreeMap::new());
    }

    pub(super) struct Window {
        app: AppHandle,
        pub(super) label: String,
        directory: PathBuf,
        alive: Arc<AtomicBool>,
    }
    impl Drop for Window {
        fn drop(&mut self) {
            self.alive.store(false, Ordering::SeqCst);
            if let Some(window) = self.app.get_webview_window(&self.label) {
                let _ = window.destroy();
            }
            let directory = self.directory.clone();
            // Only this freshly generated profile under our private temp root.
            // The browser may release files asynchronously after Close.
            tauri::async_runtime::spawn(async move {
                for _ in 0..3 {
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    let root = std::env::temp_dir().join("obsession-youtube-probe");
                    if directory.parent() != Some(root.as_path())
                        || !directory
                            .file_name()
                            .and_then(|n| n.to_str())
                            .is_some_and(|n| {
                                n.len() == 32 && n.bytes().all(|b| b.is_ascii_hexdigit())
                            })
                    {
                        return;
                    }
                    if let Ok(metadata) = std::fs::symlink_metadata(&directory) {
                        if metadata.file_type().is_symlink() {
                            return;
                        }
                    }
                    let Ok(resolved_root) = std::fs::canonicalize(&root) else {
                        return;
                    };
                    let Ok(resolved_target) = std::fs::canonicalize(&directory) else {
                        return;
                    };
                    if resolved_target.parent() != Some(resolved_root.as_path()) {
                        return;
                    }
                    if std::fs::remove_dir_all(&directory).is_ok() {
                        break;
                    }
                }
            });
        }
    }
    impl Window {
        pub(super) async fn open(
            app: &AppHandle,
            network: Arc<Mutex<Network>>,
            started: Instant,
        ) -> Result<Self, String> {
            let id = format!("{:032x}", rand::random::<u128>());
            let label = format!("youtube-probe-{id}");
            let directory = std::env::temp_dir()
                .join("obsession-youtube-probe")
                .join(&id);
            std::fs::create_dir_all(&directory)
                .map_err(|_| "Не удалось создать изолированный профиль плеера.".to_owned())?;
            let alive = Arc::new(AtomicBool::new(true));
            let guard = Self {
                app: app.clone(),
                label: label.clone(),
                directory: directory.clone(),
                alive: alive.clone(),
            };
            let app_ui = app.clone();
            let (send, receive) = oneshot::channel();
            app.run_on_main_thread(move || {
                if !alive.load(Ordering::SeqCst) {
                    return;
                }
                let result = tauri::WebviewWindowBuilder::new(
                    &app_ui,
                    &label,
                    tauri::WebviewUrl::External("about:blank".parse().unwrap()),
                )
                .title("Проверка видео YouTube · Obsession")
                .inner_size(720.0, 520.0)
                .incognito(true)
                .data_directory(directory)
                .visible(false)
                // A fresh environment also isolates pools and service workers.
                .additional_browser_args(
                    "--no-proxy-server --autoplay-policy=no-user-gesture-required",
                )
                .on_new_window(|_, _| tauri::webview::NewWindowResponse::Deny)
                .on_navigation(|url| {
                    url.as_str() == "about:blank"
                        || (url.scheme() == "https"
                            && url.host_str() == Some("www.youtube.com")
                            && url.path() == "/watch"
                            && url.query_pairs().any(|(k, v)| k == "v" && v == VIDEO_ID))
                })
                .build()
                .map(|window| {
                    let release_label = label.clone();
                    window.on_window_event(move |event| {
                        if matches!(event, tauri::WindowEvent::Destroyed) {
                            RECEIVERS.with(|receivers| {
                                receivers.borrow_mut().remove(&release_label);
                            });
                        }
                    });
                    if !alive.load(Ordering::SeqCst) {
                        let _ = window.destroy();
                    }
                })
                .map_err(|_| "Не удалось открыть тестовый плеер.".to_owned());
                let _ = send.send(result);
            })
            .map_err(|_| "Поток интерфейса недоступен.".to_owned())?;
            tokio::time::timeout(Duration::from_secs(8), receive)
                .await
                .map_err(|_| "Тестовый плеер не открылся вовремя.".to_owned())?
                .map_err(|_| "Тестовый плеер не создан.".to_owned())??;
            guard.subscribe(network, started).await?;
            guard
                .call(
                    "Network.enable",
                    json!({"maxTotalBufferSize": 1048576, "maxResourceBufferSize": 65536}),
                )
                .await?;
            guard
                .call("Network.setCacheDisabled", json!({"cacheDisabled": true}))
                .await?;
            guard
                .call("Network.setBypassServiceWorker", json!({"bypass": true}))
                .await?;
            guard.call("Page.navigate", json!({"url": WATCH})).await?;
            if let Some(window) = app.get_webview_window(&guard.label) {
                let _ = window.show();
            }
            Ok(guard)
        }
        fn window(&self) -> Result<tauri::WebviewWindow, String> {
            self.app
                .get_webview_window(&self.label)
                .ok_or("Тестовое окно закрыто.".into())
        }
        pub(super) async fn stop(&self) -> Result<(), String> {
            if let Ok(window) = self.window() {
                window
                    .destroy()
                    .map_err(|_| "Не удалось закрыть тестовый плеер.".to_owned())?;
            }
            for _ in 0..30 {
                if self.window().is_err() {
                    return Ok(());
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            Err("Закрытие тестового плеера не подтверждено.".into())
        }
        pub(super) async fn call(
            &self,
            method: &'static str,
            parameters: Value,
        ) -> Result<Value, String> {
            let window = self.window()?;
            let (send, receive) = oneshot::channel::<Result<Value, String>>();
            let reply = Arc::new(Mutex::new(Some(send)));
            window
                .with_webview(move |webview| unsafe {
                    let operation = || -> windows_core::Result<()> {
                        let core = webview.controller().CoreWebView2()?;
                        let output = reply.clone();
                        let callback = CallDevToolsProtocolMethodCompletedHandler::create(
                            Box::new(move |status, response| {
                                let value = status
                                    .map_err(|_| "Инструменты плеера отклонили запрос.".to_owned())
                                    .and_then(|_| {
                                        serde_json::from_str(&response).map_err(|_| {
                                            "Некорректный ответ инструментов плеера.".to_owned()
                                        })
                                    });
                                if let Some(send) = output.lock_recover().take() {
                                    let _ = send.send(value);
                                }
                                Ok(())
                            }),
                        );
                        let name: Vec<u16> = method.encode_utf16().chain(Some(0)).collect();
                        let arguments: Vec<u16> = parameters
                            .to_string()
                            .encode_utf16()
                            .chain(Some(0))
                            .collect();
                        core.CallDevToolsProtocolMethod(
                            PCWSTR(name.as_ptr()),
                            PCWSTR(arguments.as_ptr()),
                            &callback,
                        )
                    };
                    if operation().is_err() {
                        if let Some(send) = reply.lock_recover().take() {
                            let _ = send.send(Err("Инструменты плеера недоступны.".into()));
                        }
                    }
                })
                .map_err(|_| "Тестовое окно недоступно.".to_owned())?;
            let value = tokio::time::timeout(Duration::from_secs(3), receive)
                .await
                .map_err(|_| "Плеер не ответил на запрос измерения.".to_owned())?
                .map_err(|_| "Измерение плеера прервано.".to_owned())??;
            if value.get("error").is_some() {
                return Err("Метод измерения плеера недоступен.".into());
            }
            Ok(value)
        }
        async fn subscribe(
            &self,
            network: Arc<Mutex<Network>>,
            started: Instant,
        ) -> Result<(), String> {
            let window = self.window()?;
            let (send, receive) = oneshot::channel();
            window
                .clone()
                .with_webview(move |webview| unsafe {
                    let operation = || -> windows_core::Result<()> {
                        let core = webview.controller().CoreWebView2()?;
                        let mut retained = Vec::new();
                        for event in [
                            "Network.responseReceived",
                            "Network.dataReceived",
                            "Network.loadingFinished",
                            "Network.requestServedFromCache",
                        ] {
                            let name: Vec<u16> = event.encode_utf16().chain(Some(0)).collect();
                            let receiver =
                                core.GetDevToolsProtocolEventReceiver(PCWSTR(name.as_ptr()))?;
                            let observed = network.clone();
                            let closer = window.clone();
                            let handler = DevToolsProtocolEventReceivedEventHandler::create(
                                Box::new(move |_, args| {
                                    if let Some(args) = args {
                                        let mut pointer = windows_core::PWSTR::null();
                                        args.ParameterObjectAsJson(&mut pointer)?;
                                        let text = webview2_com::take_pwstr(pointer);
                                        if let Ok(value) = serde_json::from_str::<Value>(&text) {
                                            let stop = {
                                                let mut network = observed.lock_recover();
                                                network.event(
                                                    event,
                                                    &value,
                                                    started.elapsed().as_millis() as u64,
                                                );
                                                network.bytes >= NETWORK_BUDGET || network.overflow
                                            };
                                            if stop {
                                                let _ = closer.destroy();
                                            }
                                        }
                                    }
                                    Ok(())
                                }),
                            );
                            let mut token = 0;
                            receiver.add_DevToolsProtocolEventReceived(&handler, &mut token)?;
                            retained.push(receiver);
                        }
                        RECEIVERS.with(|receivers| {
                            receivers
                                .borrow_mut()
                                .insert(window.label().into(), retained);
                        });
                        Ok(())
                    };
                    let _ = send.send(
                        operation()
                            .map_err(|_| "Сетевой наблюдатель плеера недоступен.".to_owned()),
                    );
                })
                .map_err(|_| "Тестовое окно недоступно.".to_owned())?;
            tokio::time::timeout(Duration::from_secs(3), receive)
                .await
                .map_err(|_| "Сетевой наблюдатель не запустился вовремя.".to_owned())?
                .map_err(|_| "Запуск наблюдателя прерван.".to_owned())?
        }
    }
}

pub(super) async fn measure(
    app: &AppHandle,
    epoch: u64,
    mut round: RoundMeasurement,
    mut progress: Progress,
    initial_network: &str,
) -> Result<(super::legacy_test::TestReport, Metrics), String> {
    let started = Instant::now();
    let mut metrics = Metrics {
        video_id: VIDEO_ID,
        ..Metrics::default()
    };
    let network = Arc::new(Mutex::new(Network::default()));
    round.network_before = super::zapret2_measure::network_key().await;
    let (outcome, reason) = if round.network_before.as_deref() == Some(initial_network) {
        progress.phase = "automatic";
        progress.step_id = None;
        progress.step_index = 1;
        progress.total = 2;
        progress.label = "Проверяем загрузку и воспроизведение";
        progress.instruction = "Откроется отдельный тестовый плеер без твоего аккаунта. Проверка завершится автоматически. Закрытие окна отменяет испытание.";
        progress.expires_unix_ms = unix_ms() + TIMEOUT.as_millis() as u64;
        let _ = app.emit("zapret2-scenario-progress", &progress);
        #[cfg(windows)]
        {
            let cancel = async {
                while !super::legacy_test::cancelled(app, epoch) {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
            };
            let result = {
                let collect = async {
                    let window = match native::Window::open(app, network.clone(), started).await {
                        Ok(window) => window,
                        Err(error) => {
                            return (Outcome::Inconclusive, "instrumentation_unavailable", error)
                        }
                    };
                    let mut playback = Playback::default();
                    let result = loop {
                        {
                            let observed = network.lock_recover();
                            observed.snapshot(&mut metrics);
                            if observed.bytes >= NETWORK_BUDGET || observed.overflow {
                                break (
                                    Outcome::Inconclusive,
                                    "budget",
                                    "Достигнут лимит загрузки тестового плеера.".into(),
                                );
                            }
                            if observed.unavailable {
                                break (Outcome::Inconclusive, "media_access_unavailable", "YouTube отклонил доступ к потоку; этот ответ не доказывает отказ обхода.".into());
                            }
                        }
                        let response = match window
                            .call(
                                "Runtime.evaluate",
                                json!({"expression": SCRIPT, "returnByValue": true}),
                            )
                            .await
                        {
                            Ok(value) => value,
                            Err(error) => {
                                if app.get_webview_window(&window.label).is_none() {
                                    // A close from a budget callback is not a user cancellation.
                                    let observed = network.lock_recover();
                                    if observed.bytes >= NETWORK_BUDGET || observed.overflow {
                                        break (
                                            Outcome::Inconclusive,
                                            "budget",
                                            "Достигнут лимит загрузки тестового плеера.".into(),
                                        );
                                    }
                                    break (
                                        Outcome::Cancelled,
                                        "window_closed",
                                        "Тестовое окно закрыто.".into(),
                                    );
                                }
                                break (
                                    Outcome::Inconclusive,
                                    "instrumentation_unavailable",
                                    error,
                                );
                            }
                        };
                        let sample = match serde_json::from_value::<Sample>(
                            response["result"]["value"].clone(),
                        ) {
                            Ok(sample) => sample,
                            Err(_) => {
                                break (
                                    Outcome::Inconclusive,
                                    "instrumentation_unavailable",
                                    "Не удалось получить наблюдения плеера.".into(),
                                )
                            }
                        };
                        if sample.error {
                            break (
                                Outcome::Inconclusive,
                                "player_unavailable",
                                "YouTube не предоставил тестовое видео.".into(),
                            );
                        }
                        playback.observe(sample, started.elapsed().as_secs_f64(), &mut metrics);
                        if playback.stall > STALL_LIMIT {
                            break (
                                Outcome::Failed,
                                "playback_stalled",
                                "Видео начало воспроизводиться, затем остановилось на буферизации."
                                    .into(),
                            );
                        }
                        if let Some(target) = playback.seek_target() {
                            progress.step_index = 2;
                            progress.label = "Проверяем продолжение после перемотки";
                            let _ = app.emit("zapret2-scenario-progress", &progress);
                            let expression = format!("(() => {{ const v=document.querySelector('video'); if(v) v.currentTime={target}; }})()");
                            if window
                                .call("Runtime.evaluate", json!({"expression": expression}))
                                .await
                                .is_err()
                            {
                                break (
                                    Outcome::Inconclusive,
                                    "seek_unavailable",
                                    "Не удалось провести перемотку тестового видео.".into(),
                                );
                            }
                            playback.seek_started = Some((target, started.elapsed().as_secs_f64()));
                            playback.last = None;
                        }
                        if playback.passed(&metrics) {
                            break (Outcome::Passed, "completed", String::new());
                        }
                        tokio::time::sleep(Duration::from_millis(500)).await;
                    };
                    if let Err(error) = window.stop().await {
                        return (Outcome::Inconclusive, "window_close_unconfirmed", error);
                    }
                    result
                };
                tokio::pin!(cancel, collect);
                tokio::select! {
                    biased;
                    _ = cancel => (Outcome::Cancelled, "cancelled", "Испытание отменено.".into()),
                    result = &mut collect => result,
                    _ = tokio::time::sleep(TIMEOUT) => (Outcome::Inconclusive, "timeout", "Проверка не завершилась за отведённое время.".into()),
                }
            };
            metrics.stop_reason = result.1.into();
            // Dropping collect closes its window before run_attempt restores DPI.
            (result.0, result.2)
        }
        #[cfg(not(windows))]
        {
            (
                Outcome::Inconclusive,
                "Тестовый плеер доступен только в Windows.".into(),
            )
        }
    } else {
        metrics.stop_reason = "network_changed".into();
        (
            Outcome::Inconclusive,
            "Сетевой контекст изменился перед проверкой.".into(),
        )
    };
    network.lock_recover().snapshot(&mut metrics);
    round.network_after = super::zapret2_measure::network_key().await;
    super::zapret2_measure::set_context(&mut round, Some(initial_network));
    round.functions = functions(
        &metrics,
        outcome,
        (!reason.is_empty()).then_some(reason.as_str()),
        started.elapsed().as_millis() as u64,
    );
    let checks = round
        .functions
        .iter()
        .map(|evidence| super::legacy_test::ProbeResult {
            url: evidence.target.clone(),
            passed: evidence.outcome == Outcome::Passed,
            elapsed_ms: evidence.elapsed_ms,
            bytes: evidence.bytes,
            attempts: 1,
            http_status: None,
            error: evidence.reason.clone(),
        })
        .collect();
    let mut report = super::legacy_test::report(checks, outcome == Outcome::Cancelled);
    report.measurement = Some(round);
    Ok((report, metrics))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn response(url: &str, mime: &str, cache: bool) -> Value {
        json!({"requestId":"a","response":{"url":url,"status":206,"mimeType":mime,"protocol":"h2","fromDiskCache":cache}})
    }
    fn sample(time: f64, frames: u64) -> Sample {
        Sample {
            identity: VIDEO_ID.into(),
            present: true,
            time,
            duration: 19.0,
            ready: 4,
            width: 480,
            height: 360,
            frames,
            source: "blob:test".into(),
            ..Sample::default()
        }
    }
    #[test]
    fn network_requires_fresh_media_bytes_and_never_archives_signed_urls() {
        let url = "https://rr1.googlevideo.com/videoplayback?expire=private&sig=secret";
        for (url, mime, cache, expected) in [
            (url, "video/mp4", false, MIN_MEDIA_BYTES),
            (url, "application/vnd.yt-ump", false, MIN_MEDIA_BYTES),
            (url, "video/mp4", true, 0),
            (url, "text/html", false, 0),
            (
                "https://googlevideo.com.evil.test/videoplayback",
                "video/mp4",
                false,
                0,
            ),
        ] {
            let mut network = Network::default();
            network.event("Network.responseReceived", &response(url, mime, cache), 0);
            network.event(
                "Network.dataReceived",
                &json!({"requestId":"a","encodedDataLength":MIN_MEDIA_BYTES}),
                100,
            );
            network.event(
                "Network.loadingFinished",
                &json!({"requestId":"a","encodedDataLength":MIN_MEDIA_BYTES}),
                110,
            );
            let mut metrics = Metrics::default();
            network.snapshot(&mut metrics);
            assert_eq!(metrics.media_bytes, expected);
            assert_eq!(metrics.network_bytes, MIN_MEDIA_BYTES);
            let archived = serde_json::to_string(&metrics).unwrap();
            assert!(!archived.contains("secret") && !archived.contains("private"));
        }
        let mut network = Network::default();
        network.event(
            "Network.requestServedFromCache",
            &json!({"requestId":"a"}),
            0,
        );
        network.event(
            "Network.responseReceived",
            &response(url, "video/mp4", false),
            10,
        );
        network.event(
            "Network.loadingFinished",
            &json!({"requestId":"a","encodedDataLength":99999}),
            20,
        );
        let mut metrics = Metrics::default();
        network.snapshot(&mut metrics);
        assert_eq!(metrics.media_bytes, 0);
    }
    #[test]
    fn access_errors_are_inconclusive_and_byte_events_support_cdp_floating_numbers() {
        for status in [403, 410, 429, 503] {
            let mut network = Network::default();
            let mut value = response(
                "https://rr1.googlevideo.com/videoplayback",
                "text/html",
                false,
            );
            value["response"]["status"] = json!(status);
            network.event("Network.responseReceived", &value, 0);
            network.event(
                "Network.loadingFinished",
                &json!({"requestId":"a","encodedDataLength":99999.0}),
                1,
            );
            let mut metrics = Metrics::default();
            network.snapshot(&mut metrics);
            assert!(network.unavailable);
            assert_eq!(metrics.media_bytes, 0);
            assert!(!Playback::default().passed(&metrics));
        }
        let mut network = Network::default();
        network.event(
            "Network.responseReceived",
            &response(
                "https://rr1.googlevideo.com/videoplayback",
                "video/mp4",
                false,
            ),
            0,
        );
        network.event(
            "Network.dataReceived",
            &json!({"requestId":"a","encodedDataLength":32768.0}),
            10,
        );
        network.event(
            "Network.loadingFinished",
            &json!({"requestId":"a","encodedDataLength":32768.0}),
            20,
        );
        let mut metrics = Metrics::default();
        network.snapshot(&mut metrics);
        assert_eq!(metrics.media_bytes, 32768);
        assert_eq!(metrics.network_bytes, 32768);
    }
    #[test]
    fn player_requires_decoded_frames_continuous_time_and_seek_with_network_evidence() {
        let mut playback = Playback::default();
        let mut metrics = Metrics::default();
        for second in 0..=10 {
            playback.observe(
                sample(second as f64, second * 30),
                second as f64,
                &mut metrics,
            );
        }
        assert_eq!(playback.seconds, 10.0);
        assert!(!playback.passed(&metrics));
        let target = playback.seek_target().unwrap();
        playback.seek_started = Some((target, 10.0));
        playback.last = None;
        for step in 0..=2 {
            playback.observe(
                sample(target + step as f64, 330 + step * 30),
                11.0 + step as f64,
                &mut metrics,
            );
        }
        assert!(playback.seek_confirmed);
        assert!(!playback.passed(&metrics));
        metrics.media_bytes = MIN_MEDIA_BYTES;
        metrics.media_responses = 1;
        assert!(playback.passed(&metrics));
        let evidence = functions(&metrics, Outcome::Passed, None, 14000);
        assert!(evidence
            .iter()
            .all(|f| f.connections.is_empty() && f.proof != "user_report"));
    }
    #[test]
    fn ads_clock_jumps_and_static_frames_never_count_as_playback() {
        for mode in ["ad", "other_video", "jump", "no_frames", "no_pixels"] {
            let mut playback = Playback::default();
            let mut metrics = Metrics::default();
            for second in 0..=15 {
                let mut value = sample(second as f64, second * 30);
                match mode {
                    "ad" => value.ad = true,
                    "other_video" => value.identity = "other".into(),
                    "jump" => value.time = second as f64 * 5.0,
                    "no_frames" => value.frames = 0,
                    "no_pixels" => value.width = 0,
                    _ => {}
                }
                playback.observe(value, second as f64, &mut metrics);
            }
            assert_eq!(playback.seconds, 0.0, "{mode}");
        }
        let mut playback = Playback::default();
        let mut metrics = Metrics::default();
        playback.observe(sample(0.0, 0), 0.0, &mut metrics);
        playback.observe(sample(1.0, 30), 1.0, &mut metrics);
        for wall in 2..=7 {
            playback.observe(sample(1.0, 30), wall as f64, &mut metrics);
        }
        assert!(playback.stall > STALL_LIMIT);
    }
    #[test]
    fn successful_download_is_retained_when_playback_stalls_without_promoting_the_trial() {
        let metrics = Metrics {
            media_bytes: MIN_MEDIA_BYTES,
            media_responses: 1,
            decoded_frames: 30,
            ..Metrics::default()
        };
        let evidence = functions(&metrics, Outcome::Failed, Some("stalled"), 10000);
        assert_eq!(evidence[0].outcome, Outcome::Passed);
        assert_eq!(evidence[1].outcome, Outcome::Failed);
        assert_eq!(evidence[0].proof, "browser_media_network");
        let mut round = super::super::zapret2_tournament::tests_round("profile", true);
        round.phase = "player_scenario".into();
        round.tester_version = VERSION;
        round.functions = evidence;
        assert!(!round.complete());
        assert_eq!(
            super::super::zapret2_scenario::status(&round, false, true, false),
            "failed"
        );
    }
}
