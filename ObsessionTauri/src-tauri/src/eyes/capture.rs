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
use std::sync::atomic::Ordering;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TryRecvError, TrySendError};
use std::sync::{Arc, Mutex, TryLockError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use libloading::{Library, Symbol};
use windows::Win32::System::Performance::QueryPerformanceCounter;

use crate::dpi_supervisor::{WorkerStopOutcome, WorkerTeardown};
use crate::eyes::flow::{
    Config, FlowTable, SocketAttributionHint, WorkingSignalMode, BLACKHOLE_DELIVERY_GRACE_MS,
};
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
const ATTRIBUTION_QUEUE_CAP: usize = 256;
const TRACKER_TICK: Duration = Duration::from_millis(250);
const TRACKER_DRAIN_BUDGET: usize = 256;
const CAPTURE_HANDOFF_TIMEOUT: Duration = Duration::from_millis(250);
const CAPTURE_HANDOFF_RETRY: Duration = Duration::from_millis(1);
const DROP_LOG_INTERVAL: Duration = Duration::from_secs(5);
const PARTIAL_START_CLEANUP_TIMEOUT: Duration = Duration::from_secs(1);

struct CapturedPacket {
    packet: ParsedPacket,
    captured_at_ms: u64,
    capture_timestamp: i64,
}

#[derive(Debug)]
enum StableEmpty<T, O> {
    Busy,
    Packet(T),
    Tick(O),
    Disconnected,
    Poisoned,
}

fn try_stable_empty<T, O>(
    publication_gate: &Mutex<()>,
    rx: &Receiver<T>,
    on_empty: impl FnOnce() -> O,
) -> StableEmpty<T, O> {
    let _publication = match publication_gate.try_lock() {
        Ok(guard) => guard,
        Err(TryLockError::WouldBlock) => return StableEmpty::Busy,
        Err(TryLockError::Poisoned(_)) => return StableEmpty::Poisoned,
    };

    match rx.try_recv() {
        Ok(packet) => StableEmpty::Packet(packet),
        Err(TryRecvError::Empty) => StableEmpty::Tick(on_empty()),
        Err(TryRecvError::Disconnected) => StableEmpty::Disconnected,
    }
}

fn backlog_tick_watermark(next_capture_at_ms: u64) -> u64 {
    next_capture_at_ms.saturating_sub(1)
}

#[derive(Debug, PartialEq, Eq)]
enum QueueDrain<T> {
    Empty,
    Backlogged(T),
    Disconnected,
    Halted,
}

fn drain_available<T>(
    rx: &Receiver<T>,
    budget: usize,
    mut process: impl FnMut(T) -> bool,
) -> QueueDrain<T> {
    for _ in 0..budget {
        match rx.try_recv() {
            Ok(packet) => {
                if !process(packet) {
                    return QueueDrain::Halted;
                }
            }
            Err(TryRecvError::Empty) => return QueueDrain::Empty,
            Err(TryRecvError::Disconnected) => return QueueDrain::Disconnected,
        }
    }

    match rx.try_recv() {
        Ok(packet) => QueueDrain::Backlogged(packet),
        Err(TryRecvError::Empty) => QueueDrain::Empty,
        Err(TryRecvError::Disconnected) => QueueDrain::Disconnected,
    }
}

fn process_captured_packet(
    table: &mut FlowTable,
    captured: CapturedPacket,
    mut emit_observation: impl FnMut(Observation) -> bool,
) -> bool {
    table
        .on_captured_packet(
            &captured.packet,
            captured.captured_at_ms,
            captured.capture_timestamp,
        )
        .is_none_or(&mut emit_observation)
}

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

    fn is_due(&self, now: Instant) -> bool {
        now >= self.next
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

    /// Блокирующий приём одного пакета. Timestamp WinDivert использует тот же
    /// QueryPerformanceCounter clock, что и confirmation-arm.
    unsafe fn recv_into(&self, buf: &mut [u8]) -> Option<(usize, bool, i64)> {
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
        Some((recv_len as usize, addr.outbound(), addr.timestamp))
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
    sensor_started: Instant,
    attribution_tx: SyncSender<SocketAttributionHint>,
}

pub struct EyesStartError {
    message: String,
    safe_to_retry: bool,
    pending_teardown: Option<WorkerTeardown>,
}

impl std::fmt::Debug for EyesStartError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("EyesStartError")
            .field("message", &self.message)
            .field("safe_to_retry", &self.safe_to_retry)
            .field("pending_teardown", &self.pending_teardown.is_some())
            .finish()
    }
}

impl EyesStartError {
    fn clean(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            safe_to_retry: true,
            pending_teardown: None,
        }
    }

    pub(crate) fn partial(
        message: impl Into<String>,
        safe_to_retry: bool,
        pending_teardown: Option<WorkerTeardown>,
    ) -> Self {
        Self {
            message: message.into(),
            safe_to_retry,
            pending_teardown,
        }
    }

    pub fn safe_to_retry(&self) -> bool {
        self.safe_to_retry
    }

    pub(crate) fn into_parts(self) -> (String, bool, Option<WorkerTeardown>) {
        (self.message, self.safe_to_retry, self.pending_teardown)
    }
}

impl std::fmt::Display for EyesStartError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for EyesStartError {}

struct PartialStartCleanup {
    outcomes: Vec<WorkerStopOutcome>,
    safe_to_retry: bool,
    pending_teardown: Option<WorkerTeardown>,
}

fn cleanup_partial_capture(capture: JoinHandle<()>, timeout: Duration) -> PartialStartCleanup {
    let mut teardown = WorkerTeardown::new(vec![("eyes-capture-partial-start", capture)]);
    let outcomes = teardown.wait_bounded(timeout);
    let safe_to_retry = outcomes.iter().all(|outcome| outcome.is_clean());
    let pending_teardown = (!teardown.is_resolved()).then_some(teardown);
    PartialStartCleanup {
        outcomes,
        safe_to_retry,
        pending_teardown,
    }
}

impl EyesHandle {
    /// Supplies one exact process/socket-owner correlation to the tracker.
    /// This is a bounded best-effort side channel; failure never widens scope
    /// or turns a process launch alone into recovery evidence.
    pub fn try_attribute_socket(&self, hint: SocketAttributionHint) -> bool {
        self.attribution_tx.try_send(hint).is_ok()
    }

    pub(crate) fn capture_timestamp(&self) -> Option<i64> {
        let mut timestamp = 0i64;
        unsafe { QueryPerformanceCounter(&mut timestamp) }
            .ok()
            .map(|()| timestamp)
    }

    pub(crate) fn sensor_monotonic_ms(&self) -> u64 {
        self.sensor_started
            .elapsed()
            .as_millis()
            .min(u128::from(u64::MAX)) as u64
    }

    pub(crate) fn begin_stop(mut self) -> WorkerTeardown {
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
        WorkerTeardown::new(workers)
    }

    pub fn stop_bounded(self, timeout: Duration) -> Vec<WorkerStopOutcome> {
        self.begin_stop().wait_bounded(timeout)
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
pub fn start<F>(
    dll_path: &Path,
    cfg: Config,
    on_observation: F,
) -> Result<EyesHandle, EyesStartError>
where
    F: Fn(Observation) + Send + 'static,
{
    start_inner(dll_path, cfg, FILTER, None, on_observation)
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
    cfg.blackhole_delivery_grace_ms = BLACKHOLE_DELIVERY_GRACE_MS;
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
    let sensor_started = Instant::now();

    // Bounded канал: поток захвата -> поток трекинга. Capture не блокируется
    // на полном канале, чтобы backlog не переехал в WinDivert/kernel buffers.
    let (tx, rx): (SyncSender<CapturedPacket>, Receiver<CapturedPacket>) =
        mpsc::sync_channel(PACKET_QUEUE_CAP);
    let (attribution_tx, attribution_rx): (
        SyncSender<SocketAttributionHint>,
        Receiver<SocketAttributionHint>,
    ) = mpsc::sync_channel(ATTRIBUTION_QUEUE_CAP);
    let publication_gate = Arc::new(Mutex::new(()));

    // Поток захвата: блокирующий recv, декод, отправка в трекер.
    let capture = {
        let divert = Arc::clone(&divert);
        let capture_stop = Arc::clone(&stop);
        let publication_gate = Arc::clone(&publication_gate);
        let health = health.clone();
        std::thread::Builder::new()
            .name("eyes-capture".into())
            .spawn(move || {
                let mut buf = vec![0u8; PACKET_BUF];
                let mut dropped = 0u64;
                let mut last_drop_log: Option<Instant> = None;
                let started = sensor_started;
                let _worker_guard = SensorWorkerGuard::new(
                    Arc::clone(&capture_stop),
                    health.clone(),
                    started,
                );
                while !capture_stop.load(std::sync::atomic::Ordering::SeqCst) {
                    match unsafe { divert.recv_into(&mut buf) } {
                        Some((len, outbound, capture_timestamp)) => {
                            let _publication = match publication_gate.lock() {
                                Ok(guard) => guard,
                                Err(_) => {
                                    if let Some(health) = health.as_ref() {
                                        health.record_sensor_failure(
                                            started.elapsed().as_millis() as u64,
                                        );
                                    }
                                    break;
                                }
                            };
                            let now_ms = started.elapsed().as_millis() as u64;
                            if let Some(health) = health.as_ref() {
                                health.record_packet(now_ms);
                            }
                            match decode_ip_tcp(&buf[..len], outbound) {
                                Some(packet) => match tx.try_send(CapturedPacket {
                                    packet,
                                    captured_at_ms: now_ms,
                                    capture_timestamp,
                                }) {
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
        let publication_gate = Arc::clone(&publication_gate);
        let health = health.clone();
        let spawned = std::thread::Builder::new()
            .name("eyes-tracker".into())
            .spawn(move || {
                let start = sensor_started;
                let _worker_guard =
                    SensorWorkerGuard::new(Arc::clone(&tracker_stop), health.clone(), start);
                let mut emit_observation = |observation: Observation| {
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
                let mut pending_packet = None;
                let mut publication_wait_started = None;
                'tracker: loop {
                    let now_ms = start.elapsed().as_millis() as u64;
                    for _ in 0..ATTRIBUTION_QUEUE_CAP {
                        match attribution_rx.try_recv() {
                            Ok(hint) => {
                                table.remember_socket_domain(hint, now_ms);
                            }
                            Err(TryRecvError::Empty) => break,
                            Err(TryRecvError::Disconnected) => break,
                        }
                    }
                    let wait = tick.wait(Instant::now());
                    let captured = match pending_packet.take() {
                        Some(captured) => Some(captured),
                        None => match rx.recv_timeout(wait) {
                            Ok(captured) => Some(captured),
                            Err(RecvTimeoutError::Timeout) => None,
                            Err(RecvTimeoutError::Disconnected) => {
                                if !tracker_stop.load(std::sync::atomic::Ordering::SeqCst) {
                                    if let Some(health) = health.as_ref() {
                                        health.record_sensor_failure(
                                            start.elapsed().as_millis() as u64
                                        );
                                    }
                                }
                                break;
                            }
                        },
                    };
                    if let Some(captured) = captured {
                        if !process_captured_packet(&mut table, captured, &mut emit_observation) {
                            break;
                        }
                    }
                    if tracker_stop.load(std::sync::atomic::Ordering::SeqCst) {
                        break;
                    }

                    // A due timeout may never overtake packets which are already
                    // waiting in the capture queue. Process a bounded batch first;
                    // if the producer still has backlog, retain the look-ahead
                    // packet and defer the tick until the tracker catches up.
                    let wall_now = Instant::now();
                    if tick.is_due(wall_now) {
                        let drain = drain_available(&rx, TRACKER_DRAIN_BUDGET, |captured| {
                            process_captured_packet(&mut table, captured, &mut emit_observation)
                        });
                        match drain {
                            QueueDrain::Empty => {
                                let now_ms = start.elapsed().as_millis() as u64;
                                match try_stable_empty(&publication_gate, &rx, || {
                                    table.on_tick(now_ms)
                                }) {
                                    StableEmpty::Busy => {
                                        let started =
                                            *publication_wait_started.get_or_insert(wall_now);
                                        if wall_now.saturating_duration_since(started)
                                            >= CAPTURE_HANDOFF_TIMEOUT
                                        {
                                            if let Some(health) = health.as_ref() {
                                                health.record_sensor_failure(now_ms);
                                            }
                                            tracker_stop.store(true, Ordering::SeqCst);
                                            unsafe {
                                                (divert_for_tracker.shutdown)(
                                                    divert_for_tracker.handle,
                                                    SHUTDOWN_BOTH,
                                                );
                                            }
                                            break;
                                        }
                                        std::thread::sleep(CAPTURE_HANDOFF_RETRY);
                                        continue;
                                    }
                                    StableEmpty::Packet(captured) => {
                                        let tick_watermark =
                                            backlog_tick_watermark(captured.captured_at_ms);
                                        pending_packet = Some(captured);
                                        publication_wait_started = None;
                                        if tick.take_due(Instant::now()) {
                                            for obs in table.on_tick(tick_watermark) {
                                                if !emit_observation(obs) {
                                                    break 'tracker;
                                                }
                                            }
                                        }
                                        if tracker_stop.load(std::sync::atomic::Ordering::SeqCst) {
                                            break;
                                        }
                                        std::thread::yield_now();
                                        continue;
                                    }
                                    StableEmpty::Tick(observations) => {
                                        publication_wait_started = None;
                                        let advanced = tick.take_due(Instant::now());
                                        debug_assert!(advanced, "stable empty tick must be due");
                                        if tracker_stop.load(std::sync::atomic::Ordering::SeqCst) {
                                            break;
                                        }
                                        for observation in observations {
                                            if !emit_observation(observation) {
                                                break 'tracker;
                                            }
                                        }
                                        continue;
                                    }
                                    StableEmpty::Disconnected => {
                                        if !tracker_stop.load(std::sync::atomic::Ordering::SeqCst) {
                                            if let Some(health) = health.as_ref() {
                                                health.record_sensor_failure(now_ms);
                                            }
                                        }
                                        break;
                                    }
                                    StableEmpty::Poisoned => {
                                        if let Some(health) = health.as_ref() {
                                            health.record_sensor_failure(now_ms);
                                        }
                                        tracker_stop.store(true, Ordering::SeqCst);
                                        unsafe {
                                            (divert_for_tracker.shutdown)(
                                                divert_for_tracker.handle,
                                                SHUTDOWN_BOTH,
                                            );
                                        }
                                        break;
                                    }
                                }
                            }
                            QueueDrain::Backlogged(captured) => {
                                let tick_watermark =
                                    backlog_tick_watermark(captured.captured_at_ms);
                                pending_packet = Some(captured);
                                publication_wait_started = None;
                                if tick.take_due(wall_now) {
                                    for obs in table.on_tick(tick_watermark) {
                                        if !emit_observation(obs) {
                                            break 'tracker;
                                        }
                                    }
                                }
                                if tracker_stop.load(std::sync::atomic::Ordering::SeqCst) {
                                    break;
                                }
                                std::thread::yield_now();
                                continue;
                            }
                            QueueDrain::Disconnected => {
                                if !tracker_stop.load(std::sync::atomic::Ordering::SeqCst) {
                                    if let Some(health) = health.as_ref() {
                                        health.record_sensor_failure(
                                            start.elapsed().as_millis() as u64
                                        );
                                    }
                                }
                                break;
                            }
                            QueueDrain::Halted => break,
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
                let cleanup = cleanup_partial_capture(capture, PARTIAL_START_CLEANUP_TIMEOUT);
                return Err(EyesStartError::partial(
                    format!(
                        "не удалось создать поток трекинга: {error}; capture cleanup: {:?}",
                        cleanup.outcomes
                    ),
                    cleanup.safe_to_retry,
                    cleanup.pending_teardown,
                ));
            }
        }
    };

    Ok(EyesHandle {
        stop,
        capture: Some(capture),
        tracker: Some(tracker),
        divert,
        sensor_started,
        attribution_tx,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eyes::parse::{FlowKey, TcpFlags};
    use std::net::{IpAddr, Ipv4Addr};

    #[test]
    fn captured_packet_keeps_the_capture_time_while_queued() {
        let captured = CapturedPacket {
            packet: ParsedPacket {
                outbound: true,
                key: FlowKey {
                    local_port: 50_000,
                    remote_ip: IpAddr::V4(Ipv4Addr::new(203, 0, 113, 10)),
                    remote_port: 443,
                },
                ttl: 64,
                seq: 1,
                flags: TcpFlags::default(),
                payload: Vec::new(),
            },
            captured_at_ms: 41,
            capture_timestamp: 9_001,
        };

        assert_eq!(captured.captured_at_ms, 41);
        assert_eq!(captured.capture_timestamp, 9_001);
        assert_eq!(captured.packet.key.local_port, 50_000);
    }

    #[test]
    fn due_tick_drain_processes_every_ready_packet_before_timeout_evaluation() {
        let (tx, rx) = std::sync::mpsc::sync_channel(4);
        tx.send(1).unwrap();
        tx.send(2).unwrap();
        tx.send(3).unwrap();
        let mut processed = Vec::new();

        let outcome = drain_available(&rx, 4, |packet| {
            processed.push(packet);
            true
        });

        assert_eq!(outcome, QueueDrain::Empty);
        assert_eq!(processed, vec![1, 2, 3]);
    }

    #[test]
    fn due_tick_is_deferred_when_drain_budget_leaves_backlog() {
        let (tx, rx) = std::sync::mpsc::sync_channel(4);
        tx.send(1).unwrap();
        tx.send(2).unwrap();
        tx.send(3).unwrap();
        let mut processed = Vec::new();

        let outcome = drain_available(&rx, 2, |packet| {
            processed.push(packet);
            true
        });

        assert_eq!(outcome, QueueDrain::Backlogged(3));
        assert_eq!(processed, vec![1, 2]);
    }

    #[test]
    fn stable_empty_recheck_observes_producer_that_won_publication_gate() {
        let gate = std::sync::Mutex::new(());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let producer = gate.lock().unwrap();

        assert!(matches!(
            try_stable_empty(&gate, &rx, || panic!("busy gate must not tick")),
            StableEmpty::Busy
        ));
        tx.send(7).unwrap();
        drop(producer);

        assert!(matches!(
            try_stable_empty(&gate, &rx, || panic!("queued packet must win")),
            StableEmpty::Packet(7)
        ));
    }

    #[test]
    fn stable_empty_tick_runs_while_publication_gate_is_held() {
        let gate = std::sync::Mutex::new(());
        let (_tx, rx) = std::sync::mpsc::sync_channel::<u8>(1);

        let result = try_stable_empty(&gate, &rx, || {
            assert!(matches!(
                gate.try_lock(),
                Err(std::sync::TryLockError::WouldBlock)
            ));
            42
        });

        assert!(matches!(result, StableEmpty::Tick(42)));
    }

    #[test]
    fn parse_failure_under_gate_is_followed_by_a_tick_not_queue_changed() {
        let gate = std::sync::Mutex::new(());
        let (_tx, rx) = std::sync::mpsc::sync_channel::<u8>(1);
        let parse_failure = gate.lock().unwrap();

        assert!(matches!(
            try_stable_empty(&gate, &rx, || ()),
            StableEmpty::Busy
        ));
        drop(parse_failure);
        assert!(matches!(
            try_stable_empty(&gate, &rx, || 9),
            StableEmpty::Tick(9)
        ));
    }

    #[test]
    fn backlog_tick_stops_before_the_next_unprocessed_capture() {
        assert_eq!(backlog_tick_watermark(500), 499);
        assert_eq!(backlog_tick_watermark(0), 0);
    }

    #[test]
    fn partial_start_error_preserves_cleanup_safety() {
        assert!(EyesStartError::clean("before open").safe_to_retry());
        assert!(!EyesStartError::partial("worker leaked", false, None).safe_to_retry());
    }

    #[test]
    fn partial_start_timeout_returns_a_persistent_teardown_ticket() {
        let (release, blocked) = std::sync::mpsc::channel();
        let capture = std::thread::spawn(move || {
            let _ = blocked.recv();
        });

        let cleanup = cleanup_partial_capture(capture, Duration::ZERO);

        assert!(!cleanup.safe_to_retry);
        assert!(cleanup
            .outcomes
            .iter()
            .any(|outcome| { outcome.state == crate::dpi_supervisor::WorkerStopState::TimedOut }));
        let mut teardown = cleanup.pending_teardown.unwrap();
        assert!(!teardown.is_resolved());

        release.send(()).unwrap();
        teardown.wait_bounded(Duration::from_secs(1));
        assert!(teardown.is_resolved());
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
