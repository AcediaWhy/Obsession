//! v1: захват трафика через WinDivert в режиме sniff (только Windows).
//!
//! Грузим ТУ ЖЕ `WinDivert.dll` (2.2), что и winws — через `libloading` в
//! рантайме. Это устраняет главный риск: версия драйвера у наблюдателя и у
//! winws не может разойтись, потому что файл буквально один. Никакого `.lib`,
//! `WINDIVERT_PATH` или статической LGPL-линковки на билде не требуется.
//!
//! Хендл открывается с `SNIFF | RECV_ONLY | NO_INSTALL`:
//! - SNIFF     — получаем КОПИЮ пакета, не вмешиваясь в доставку (боевой winws не тронут);
//! - RECV_ONLY — физически не можем инжектить (наблюдатель не мешает трафику);
//! - NO_INSTALL — не переустанавливаем драйвер; открытие упадёт, если WinDivert
//!   ещё не поднят. Значит запускать глаза имеет смысл, только когда winws уже
//!   активен — что совпадает с задачей «следить за текущей стратегией».
//!
//! Модель — актор: блокирующий `recv` в своём потоке шлёт `ParsedPacket` в
//! канал; поток-трекер единолично владеет `FlowTable` (никаких мьютексов на
//! таблице); тикер шлёт метки времени для детекта blackhole по таймауту.

#![cfg(windows)]

use std::ffi::c_void;
use std::path::Path;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use libloading::{Library, Symbol};

use crate::dpi_supervisor::{join_workers_bounded, WorkerStopOutcome};
use crate::eyes::flow::{Config, FlowTable, WorkingSignalMode};
use crate::eyes::parse::{decode_ip_tcp, ParsedPacket};
use crate::eyes::signal::Observation;
use crate::legacy_reliability::health::AtomicHealthCounters;
use crate::legacy_reliability::target_registry::PortPlan;

// --- значения из windivert.h (2.2), подтверждены по официальному заголовку ---
const LAYER_NETWORK: u8 = 0;
const FLAG_SNIFF: u64 = 0x0001;
const FLAG_RECV_ONLY: u64 = 0x0004;
const FLAG_NO_INSTALL: u64 = 0x0010;

/// Максимальный размер сетевого пакета (с запасом на джамбо-кадры не идём —
/// TLS ClientHello укладывается с большим запасом).
const PACKET_BUF: usize = 65535;
/// Жёсткая граница backlog capture -> tracker. При переполнении лучше потерять
/// отдельное наблюдение, чем бесконечно наращивать RAM.
const PACKET_QUEUE_CAP: usize = 4096;
const TRACKER_TICK: Duration = Duration::from_millis(250);
const DROP_LOG_INTERVAL: Duration = Duration::from_secs(5);
const PARTIAL_START_CLEANUP_TIMEOUT: Duration = Duration::from_secs(1);

struct TickDeadline {
    period: Duration,
    next: Instant,
}

impl TickDeadline {
    fn new(now: Instant, period: Duration) -> Self {
        Self {
            period,
            next: now + period,
        }
    }

    fn wait(&self, now: Instant) -> Duration {
        self.next.saturating_duration_since(now)
    }

    /// Возвращает true не чаще одного раза за period. Пропущенные интервалы
    /// скипаются, поэтому после задержки нет burst-догона.
    fn take_due(&mut self, now: Instant) -> bool {
        if now < self.next {
            return false;
        }
        self.next = now + self.period;
        true
    }
}

/// Marks an unexpected worker unwind/return as a terminal sensor failure. An
/// intentional stop sets the shared flag before workers leave, disarming this
/// guard without needing a second control channel.
struct SensorWorkerGuard {
    stop: Arc<std::sync::atomic::AtomicBool>,
    health: Option<Arc<AtomicHealthCounters>>,
    started: Instant,
}

impl SensorWorkerGuard {
    fn new(
        stop: Arc<std::sync::atomic::AtomicBool>,
        health: Option<Arc<AtomicHealthCounters>>,
        started: Instant,
    ) -> Self {
        Self {
            stop,
            health,
            started,
        }
    }
}

impl Drop for SensorWorkerGuard {
    fn drop(&mut self) {
        if !self.stop.load(std::sync::atomic::Ordering::SeqCst) {
            if let Some(health) = self.health.as_ref() {
                health.record_sensor_failure(self.started.elapsed().as_millis() as u64);
            }
        }
    }
}

/// Сигнатуры экспортов WinDivert.dll (см. windivert.h 2.2).
type FnOpen = unsafe extern "system" fn(*const u8, u8, i16, u64) -> *mut c_void;
type FnRecv =
    unsafe extern "system" fn(*mut c_void, *mut u8, u32, *mut u32, *mut WinDivertAddress) -> i32;
type FnClose = unsafe extern "system" fn(*mut c_void) -> i32;
type FnShutdown = unsafe extern "system" fn(*mut c_void, u32) -> i32;

const SHUTDOWN_BOTH: u32 = 3;
const INVALID_HANDLE: isize = -1;

/// Раскладка `WINDIVERT_ADDRESS` для network-слоя.
///
/// C-структура: `INT64 Timestamp` + упакованное `UINT64`-битполе
/// (Layer:8, Event:8, Sniffed:1, Outbound:1, Loopback:1, Impostor:1, IPv6:1,
/// IPChecksum:1, TCPChecksum:1, UDPChecksum:1, Reserved) + union данных слоя.
/// Нам из битполя нужны только Outbound (бит 17) и IPv6 (бит 20).
/// Держим union как непрозрачный хвост фиксированного размера.
#[repr(C)]
struct WinDivertAddress {
    timestamp: i64,
    bitfield: u64,
    // WINDIVERT_DATA_NETWORK: { UINT32 IfIdx; UINT32 SubIfIdx; } + выравнивание
    // до размера самого большого варианта union. 64 байта — заведомо достаточно
    // (реальный размер аддреса в 2.2 — 64 байта суммарно, но берём хвост с запасом).
    _data: [u8; 64],
}

impl WinDivertAddress {
    fn zeroed() -> Self {
        Self {
            timestamp: 0,
            bitfield: 0,
            _data: [0u8; 64],
        }
    }
    /// Направление: бит 17 битполя.
    fn outbound(&self) -> bool {
        (self.bitfield >> 17) & 1 == 1
    }
}

/// Загруженная библиотека + разрешённые символы. Держим `Library` живой всё
/// время работы хендла — иначе указатели на функции инвалидируются.
struct WinDivert {
    _lib: Library,
    handle: *mut c_void,
    recv: FnRecv,
    close: FnClose,
    shutdown: FnShutdown,
}

// Хендл WinDivert потокобезопасен для recv/close из разных потоков в нашей
// модели (recv в одном потоке, shutdown из управляющего). Оборачиваем вручную.
unsafe impl Send for WinDivert {}
unsafe impl Sync for WinDivert {}

impl WinDivert {
    /// Открывает sniff-хендл, грузя dll по указанному пути (наш bin/WinDivert.dll).
    unsafe fn open(dll_path: &Path, filter: &str) -> Result<Self, String> {
        let lib = Library::new(dll_path)
            .map_err(|e| format!("не удалось загрузить WinDivert.dll: {e}"))?;

        let open: Symbol<FnOpen> = lib
            .get(b"WinDivertOpen\0")
            .map_err(|e| format!("нет символа WinDivertOpen: {e}"))?;
        let recv: Symbol<FnRecv> = lib
            .get(b"WinDivertRecv\0")
            .map_err(|e| format!("нет символа WinDivertRecv: {e}"))?;
        let close: Symbol<FnClose> = lib
            .get(b"WinDivertClose\0")
            .map_err(|e| format!("нет символа WinDivertClose: {e}"))?;
        let shutdown: Symbol<FnShutdown> = lib
            .get(b"WinDivertShutdown\0")
            .map_err(|e| format!("нет символа WinDivertShutdown: {e}"))?;

        // Разыменовываем символы в 'static-указатели (lib держим живой в структуре).
        let recv = *recv;
        let close = *close;
        let shutdown = *shutdown;

        let mut filter_z = filter.as_bytes().to_vec();
        filter_z.push(0);
        let flags = FLAG_SNIFF | FLAG_RECV_ONLY | FLAG_NO_INSTALL;
        let handle = open(filter_z.as_ptr(), LAYER_NETWORK, 0, flags);

        if handle as isize == INVALID_HANDLE || handle.is_null() {
            let err = std::io::Error::last_os_error();
            return Err(format!(
                "WinDivertOpen не удался (драйвер не запущен? запущен ли winws?): {err}"
            ));
        }

        Ok(Self {
            _lib: lib,
            handle,
            recv,
            close,
            shutdown,
        })
    }

    /// Блокирующий приём одного пакета. Возвращает (len, outbound) либо None при ошибке.
    unsafe fn recv_into(&self, buf: &mut [u8]) -> Option<(usize, bool)> {
        let mut addr = WinDivertAddress::zeroed();
        let mut recv_len: u32 = 0;
        let ok = (self.recv)(
            self.handle,
            buf.as_mut_ptr(),
            buf.len() as u32,
            &mut recv_len,
            &mut addr,
        );
        if ok == 0 {
            return None;
        }
        Some((recv_len as usize, addr.outbound()))
    }
}

impl Drop for WinDivert {
    fn drop(&mut self) {
        unsafe {
            // Сначала будим заблокированный recv, затем закрываем хендл.
            (self.shutdown)(self.handle, SHUTDOWN_BOTH);
            (self.close)(self.handle);
        }
    }
}

/// Хендл управления работающими «глазами». Дроп/`stop` гасит поток захвата.
pub struct EyesHandle {
    stop: Arc<std::sync::atomic::AtomicBool>,
    capture: Option<JoinHandle<()>>,
    tracker: Option<JoinHandle<()>>,
    divert: Arc<WinDivert>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EyesStartError {
    message: String,
    safe_to_retry: bool,
}

impl EyesStartError {
    fn clean(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            safe_to_retry: true,
        }
    }

    fn partial(message: impl Into<String>, safe_to_retry: bool) -> Self {
        Self {
            message: message.into(),
            safe_to_retry,
        }
    }

    pub fn safe_to_retry(&self) -> bool {
        self.safe_to_retry
    }
}

impl std::fmt::Display for EyesStartError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for EyesStartError {}

impl EyesHandle {
    pub fn stop_bounded(mut self, timeout: Duration) -> Vec<WorkerStopOutcome> {
        self.stop.store(true, std::sync::atomic::Ordering::SeqCst);
        unsafe {
            (self.divert.shutdown)(self.divert.handle, SHUTDOWN_BOTH);
        }
        let mut workers = Vec::with_capacity(2);
        if let Some(capture) = self.capture.take() {
            workers.push(("eyes-capture", capture));
        }
        if let Some(tracker) = self.tracker.take() {
            workers.push(("eyes-tracker", tracker));
        }
        join_workers_bounded(workers, timeout)
    }

    /// Останавливает наблюдение: будит recv через shutdown, ждёт завершения потоков.
    pub fn stop(mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::SeqCst);
        // Разбудить заблокированный WinDivertRecv.
        unsafe {
            (self.divert.shutdown)(self.divert.handle, SHUTDOWN_BOTH);
        }
        if let Some(h) = self.capture.take() {
            let _ = h.join();
        }
        if let Some(h) = self.tracker.take() {
            let _ = h.join();
        }
    }
}

impl Drop for EyesHandle {
    /// Страховка на случай, если хэндл дропнули без явного `stop()` (перезапись
    /// `AppState.eyes`, паника и т.п.): без неё потоки capture/tracker держали бы
    /// `Arc<WinDivert>` и жили до конца процесса — утечка двух потоков и хэндла
    /// драйвера. `stop()` забирает JoinHandle'ы через `.take()`, поэтому после
    /// него этот Drop — no-op (двойного shutdown/join не будет).
    fn drop(&mut self) {
        if self.capture.is_none() && self.tracker.is_none() {
            return;
        }
        self.stop.store(true, std::sync::atomic::Ordering::SeqCst);
        unsafe {
            (self.divert.shutdown)(self.divert.handle, SHUTDOWN_BOTH);
        }
        if let Some(h) = self.capture.take() {
            let _ = h.join();
        }
        if let Some(h) = self.tracker.take() {
            let _ = h.join();
        }
    }
}

/// Фильтр WinDivert: TCP на 443 порт в обе стороны. Грубый и дешёвый —
/// точный матч по домену делаем в userspace (парсер SNI + хостлист в `flow`).
const FILTER: &str = "tcp and (tcp.SrcPort == 443 or tcp.DstPort == 443)";

/// Запускает «глаза»: открывает sniff-хендл и поднимает потоки захвата/трекинга.
///
/// `dll_path` — путь к нашей `WinDivert.dll` (обычно `bin_dir()/WinDivert.dll`).
/// `on_observation` вызывается для каждого готового per-flow вердикта.
pub fn start<F>(dll_path: &Path, cfg: Config, on_observation: F) -> Result<EyesHandle, String>
where
    F: Fn(Observation) + Send + 'static,
{
    start_inner(dll_path, cfg, FILTER, None, on_observation).map_err(|error| error.to_string())
}

/// Legacy-only entry point with a registry-derived TCP capture plan and
/// out-of-band health counters. The compatibility [`start`] path used by
/// Zapret2 remains fixed to TCP/443.
pub fn start_legacy<F>(
    dll_path: &Path,
    mut cfg: Config,
    port_plan: &PortPlan,
    health: Arc<AtomicHealthCounters>,
    on_observation: F,
) -> Result<EyesHandle, EyesStartError>
where
    F: Fn(Observation) + Send + 'static,
{
    // The Legacy entry point owns the strict policy boundary. Keep this
    // invariant here as well as at the current dpi caller so future callers
    // cannot accidentally re-enable compatibility semantics.
    cfg.working_signal_mode = WorkingSignalMode::StrictTls;
    let filter = port_plan
        .to_windivert_filter()
        .ok_or_else(|| EyesStartError::clean("Legacy Eyes capture plan is empty"))?;
    start_inner(dll_path, cfg, &filter, Some(health), on_observation)
}

fn start_inner<F>(
    dll_path: &Path,
    cfg: Config,
    filter: &str,
    health: Option<Arc<AtomicHealthCounters>>,
    on_observation: F,
) -> Result<EyesHandle, EyesStartError>
where
    F: Fn(Observation) + Send + 'static,
{
    let divert =
        Arc::new(unsafe { WinDivert::open(dll_path, filter) }.map_err(EyesStartError::clean)?);
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));

    // Bounded канал: поток захвата -> поток трекинга. Capture не блокируется
    // на полном канале, чтобы backlog не переехал в WinDivert/kernel buffers.
    let (tx, rx): (SyncSender<ParsedPacket>, Receiver<ParsedPacket>) =
        mpsc::sync_channel(PACKET_QUEUE_CAP);

    // Поток захвата: блокирующий recv, декод, отправка в трекер.
    let capture = {
        let divert = Arc::clone(&divert);
        let capture_stop = Arc::clone(&stop);
        let health = health.clone();
        std::thread::Builder::new()
            .name("eyes-capture".into())
            .spawn(move || {
                let mut buf = vec![0u8; PACKET_BUF];
                let mut dropped = 0u64;
                let mut last_drop_log: Option<Instant> = None;
                let started = Instant::now();
                let _worker_guard = SensorWorkerGuard::new(
                    Arc::clone(&capture_stop),
                    health.clone(),
                    started,
                );
                while !capture_stop.load(std::sync::atomic::Ordering::SeqCst) {
                    match unsafe { divert.recv_into(&mut buf) } {
                        Some((len, outbound)) => {
                            let now_ms = started.elapsed().as_millis() as u64;
                            if let Some(health) = health.as_ref() {
                                health.record_packet(now_ms);
                            }
                            match decode_ip_tcp(&buf[..len], outbound) {
                                Some(pkt) => match tx.try_send(pkt) {
                                    Ok(()) => {}
                                    Err(TrySendError::Full(_)) => {
                                        if let Some(health) = health.as_ref() {
                                            health.record_queue_drop(now_ms);
                                            continue;
                                        }
                                        dropped = dropped.saturating_add(1);
                                        let now = Instant::now();
                                        let should_log = last_drop_log
                                            .map(|last| now.duration_since(last) >= DROP_LOG_INTERVAL)
                                            .unwrap_or(true);
                                        if should_log {
                                            eprintln!(
                                                "[eyes] очередь capture переполнена: отброшено {dropped} пакетов"
                                            );
                                            dropped = 0;
                                            last_drop_log = Some(now);
                                        }
                                    }
                                    Err(TrySendError::Disconnected(_)) => {
                                        if let Some(health) = health.as_ref() {
                                            health.record_sensor_failure(now_ms);
                                        }
                                        break;
                                    }
                                },
                                None => {
                                    if let Some(health) = health.as_ref() {
                                        health.record_parse_error(now_ms);
                                    }
                                }
                            }
                        }
                        None => {
                            // recv вернул ошибку: shutdown при остановке — это норма.
                            if capture_stop.load(std::sync::atomic::Ordering::SeqCst) {
                                break;
                            }
                            // Не превращаем ошибку WinDivert в автоматический
                            // verdict. Это терминальный сбой текущего sensor
                            // generation; Manager переведёт его в Blind.
                            if let Some(health) = health.as_ref() {
                                health.record_sensor_failure(started.elapsed().as_millis() as u64);
                            }
                            // Иначе краткая пауза, чтобы не крутить busy-loop на сбое.
                            std::thread::sleep(Duration::from_millis(50));
                        }
                    }
                }
            })
            .map_err(|e| EyesStartError::clean(format!("не удалось создать поток захвата: {e}")))?
    };

    // Поток трекинга: единоличный владелец FlowTable. Пакеты + тики времени.
    let tracker = {
        let tracker_stop = Arc::clone(&stop);
        let divert_for_tracker = Arc::clone(&divert);
        let health = health.clone();
        let spawned = std::thread::Builder::new()
            .name("eyes-tracker".into())
            .spawn(move || {
                let start = Instant::now();
                let _worker_guard =
                    SensorWorkerGuard::new(Arc::clone(&tracker_stop), health.clone(), start);
                let emit_observation = |observation: Observation| {
                    let callback_result =
                        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            on_observation(observation)
                        }));
                    if callback_result.is_ok() {
                        true
                    } else {
                        if let Some(health) = health.as_ref() {
                            health.record_sensor_failure(start.elapsed().as_millis() as u64);
                        }
                        tracker_stop.store(true, std::sync::atomic::Ordering::SeqCst);
                        unsafe {
                            (divert_for_tracker.shutdown)(divert_for_tracker.handle, SHUTDOWN_BOTH);
                        }
                        false
                    }
                };
                let mut table = FlowTable::new(cfg);
                let mut tick = TickDeadline::new(Instant::now(), TRACKER_TICK);
                loop {
                    let wait = tick.wait(Instant::now());
                    match rx.recv_timeout(wait) {
                        Ok(pkt) => {
                            let now = start.elapsed().as_millis() as u64;
                            if let Some(obs) = table.on_packet(&pkt, now) {
                                if !emit_observation(obs) {
                                    break;
                                }
                            }
                        }
                        Err(RecvTimeoutError::Timeout) => {}
                        Err(RecvTimeoutError::Disconnected) => {
                            if !tracker_stop.load(std::sync::atomic::Ordering::SeqCst) {
                                if let Some(health) = health.as_ref() {
                                    health
                                        .record_sensor_failure(start.elapsed().as_millis() as u64);
                                }
                            }
                            break;
                        }
                    }
                    // Пакеты могут будить tracker тысячами раз в секунду, но полный
                    // O(flows) tick выполняется только по временному дедлайну.
                    let wall_now = Instant::now();
                    if tick.take_due(wall_now) {
                        let now = start.elapsed().as_millis() as u64;
                        for obs in table.on_tick(now) {
                            if !emit_observation(obs) {
                                break;
                            }
                        }
                    }
                    if tracker_stop.load(std::sync::atomic::Ordering::SeqCst) {
                        break;
                    }
                }
            });
        match spawned {
            Ok(tracker) => tracker,
            Err(error) => {
                stop.store(true, std::sync::atomic::Ordering::SeqCst);
                unsafe {
                    (divert.shutdown)(divert.handle, SHUTDOWN_BOTH);
                }
                let cleanup = join_workers_bounded(
                    vec![("eyes-capture-partial-start", capture)],
                    PARTIAL_START_CLEANUP_TIMEOUT,
                );
                let safe_to_retry = cleanup.iter().all(|outcome| outcome.is_clean());
                return Err(EyesStartError::partial(
                    format!(
                        "не удалось создать поток трекинга: {error}; capture cleanup: {cleanup:?}"
                    ),
                    safe_to_retry,
                ));
            }
        }
    };

    Ok(EyesHandle {
        stop,
        capture: Some(capture),
        tracker: Some(tracker),
        divert,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_start_error_preserves_cleanup_safety() {
        assert!(EyesStartError::clean("before open").safe_to_retry());
        assert!(!EyesStartError::partial("worker leaked", false).safe_to_retry());
    }

    #[test]
    fn tick_deadline_is_time_driven_and_skips_missed_intervals() {
        let start = Instant::now();
        let period = Duration::from_millis(250);
        let mut tick = TickDeadline::new(start, period);

        assert!(!tick.take_due(start + Duration::from_millis(1)));
        assert!(tick.take_due(start + period));
        // Тысячи packet wakeups внутри следующего периода не создают ticks.
        for millis in 251..500 {
            assert!(!tick.take_due(start + Duration::from_millis(millis)));
        }
        assert!(tick.take_due(start + Duration::from_millis(500)));
        // Большой лаг даёт один tick, а не пачку накопленных.
        assert!(tick.take_due(start + Duration::from_secs(5)));
        assert!(!tick.take_due(start + Duration::from_secs(5)));
    }
}
