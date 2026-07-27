//! Рантайм Мозга — тонкий tokio-интерпретатор чистой машины [`super::model`].
//! Единственное место сайд-эффектов: спавн/стоп winws (`dpi`), пробы, запись
//! L1-кэша, эмиссия статуса во фронт. Повторяет паттерн Глаз (чистое ядро +
//! грязный рантайм).
//!
//! **Время.** Model работает на логическом времени и сравнивает `ts` между
//! событиями (окно, дедлайны, backoff). Часы Глаз (`Observation.ts_ms`) —
//! `Instant::elapsed` от старта их потока трекинга, который **сбрасывается в 0 на
//! каждом респавне** (Глаза рестартуют на каждом `Switch`). Доверять им нельзя:
//! логическое время прыгнуло бы назад. Поэтому рантайм ведёт СВОЮ монотонику и
//! перештамповывает `now`/`ts` во ВСЕХ входящих событиях единым источником.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tauri::async_runtime::{self, JoinHandle};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::{mpsc, watch};

use crate::brain::model::{
    Action, Brain, BrainCfg, BrainEvent, BrainStatus, Candidate, NetSnapshot, Source,
};
use crate::netcache::NetCache;
use crate::netid;
use crate::ranking::Ranking;
use crate::state::AppState;
use crate::util::VersionedSection;

const OBSERVATION_QUEUE_CAP: usize = 1024;
const CONTROL_QUEUE_CAP: usize = 64;
const DROP_LOG_INTERVAL: Duration = Duration::from_secs(5);

struct DropLogState {
    dropped: u64,
    last_report: Option<Instant>,
}

/// Клонируемый вход рантайма. Высокочастотные observations отделены от
/// управляющих событий, поэтому сетевой flood не вытесняет SessionStop.
#[derive(Clone)]
pub struct BrainInput {
    observation_tx: mpsc::Sender<BrainEvent>,
    control_tx: mpsc::Sender<BrainEvent>,
    drop_log: Arc<Mutex<DropLogState>>,
}

impl BrainInput {
    fn new(observation_tx: mpsc::Sender<BrainEvent>, control_tx: mpsc::Sender<BrainEvent>) -> Self {
        Self {
            observation_tx,
            control_tx,
            drop_log: Arc::new(Mutex::new(DropLogState {
                dropped: 0,
                last_report: None,
            })),
        }
    }

    /// Неблокирующая отправка packet-driven события. `Some(n)` означает, что
    /// очередь переполнена и пора залогировать n drops за последний интервал.
    pub fn try_observation(
        &self,
        domain: String,
        verdict: crate::eyes::Verdict,
        ts: u64,
    ) -> Option<u64> {
        let event = BrainEvent::Observation {
            domain,
            verdict,
            ts,
        };
        match self.observation_tx.try_send(event) {
            Ok(()) | Err(mpsc::error::TrySendError::Closed(_)) => None,
            Err(mpsc::error::TrySendError::Full(_)) => {
                let now = Instant::now();
                let mut log = self.drop_log.lock().unwrap_or_else(|e| e.into_inner());
                log.dropped = log.dropped.saturating_add(1);
                let should_report = log
                    .last_report
                    .map(|last| now.duration_since(last) >= DROP_LOG_INTERVAL)
                    .unwrap_or(true);
                if should_report {
                    let dropped = std::mem::take(&mut log.dropped);
                    log.last_report = Some(now);
                    Some(dropped)
                } else {
                    None
                }
            }
        }
    }

    /// Управляющие события не дропаются: producer ждёт место в малом bounded
    /// канале. Вызывать без удержания std::sync guard.
    pub async fn send_control(&self, event: BrainEvent) -> Result<(), ()> {
        debug_assert!(!matches!(
            event,
            BrainEvent::Observation { .. } | BrainEvent::Tick(_)
        ));
        self.control_tx.send(event).await.map_err(|_| ())
    }
}

/// Хендл на работающую задачу Мозга. Живёт в `AppState.brain`.
pub struct BrainHandle {
    pub input: BrainInput,
    /// Последний агрегированный статус (для `brain_get_status`).
    pub status: watch::Receiver<BrainStatus>,
    shutdown_tx: watch::Sender<bool>,
    join: JoinHandle<()>,
}

impl BrainHandle {
    /// Сигнал shutdown не зависит от заполненности control queue. Abort остаётся
    /// последней страховкой для зависшего внешнего I/O внутри action.
    pub fn shutdown(self) {
        let _ = self.shutdown_tx.send(true);
        self.join.abort();
    }
}

/// Начальный «пустой» статус для watch-канала до первого события.
fn initial_status() -> BrainStatus {
    BrainStatus {
        enabled: false,
        phase: "idle",
        category: None,
        current_conf: None,
        ladder_level: "none",
        frozen_until_ms: None,
        backoff_secs: None,
        asn_region: None,
        gateway_mac_masked: None,
    }
}

/// Запускает задачу Мозга. Все packet-driven очереди имеют фиксированную ёмкость.
pub fn start(app: AppHandle) -> BrainHandle {
    let (observation_tx, mut observation_rx) = mpsc::channel::<BrainEvent>(OBSERVATION_QUEUE_CAP);
    let (control_tx, mut control_rx) = mpsc::channel::<BrainEvent>(CONTROL_QUEUE_CAP);
    let (shutdown_tx, mut shutdown_rx) = watch::channel(false);
    let (status_tx, status_rx) = watch::channel(initial_status());
    let input = BrainInput::new(observation_tx, control_tx);

    let cfg = BrainCfg::default();
    let tick_ms = cfg.tick_ms.max(50);
    let join = async_runtime::spawn(async move {
        let mut brain = Brain::new(cfg);
        let start = Instant::now();
        let mut net = NetSnapshot::default();
        let mut pending: VecDeque<BrainEvent> = VecDeque::new();
        let mut interval = tokio::time::interval(Duration::from_millis(tick_ms));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            let ev = if let Some(ev) = pending.pop_front() {
                ev
            } else {
                tokio::select! {
                    biased;
                    changed = shutdown_rx.changed() => {
                        if changed.is_err() || *shutdown_rx.borrow() {
                            BrainEvent::Shutdown
                        } else {
                            continue;
                        }
                    }
                    control = control_rx.recv() => {
                        control.unwrap_or(BrainEvent::Shutdown)
                    }
                    _ = interval.tick() => BrainEvent::Tick(0),
                    observation = observation_rx.recv() => {
                        observation.unwrap_or(BrainEvent::Shutdown)
                    }
                }
            };

            let shutdown = matches!(ev, BrainEvent::Shutdown);
            let now = start.elapsed().as_millis() as u64;
            let ev = restamp(ev, now);
            if let BrainEvent::SessionStart { net: n, .. } = &ev {
                net = n.clone();
            }
            for action in brain.step(ev) {
                if let Some(follow_up) = exec(&app, &net, action, &status_tx).await {
                    // Follow-up рождается строго из одного action; локальная очередь
                    // не принимает packet input и не может расти от сетевого flood.
                    pending.push_back(follow_up);
                }
            }
            if shutdown {
                break;
            }
        }
    });

    BrainHandle {
        input,
        status: status_rx,
        shutdown_tx,
        join,
    }
}

/// Перештамповывает временные поля события единой монотоникой рантайма.
fn restamp(ev: BrainEvent, now: u64) -> BrainEvent {
    match ev {
        BrainEvent::Observation {
            domain, verdict, ..
        } => BrainEvent::Observation {
            domain,
            verdict,
            ts: now,
        },
        BrainEvent::RespawnResult { ok, .. } => BrainEvent::RespawnResult { ok, now },
        BrainEvent::ProbeResult {
            category, conf, ok, ..
        } => BrainEvent::ProbeResult {
            category,
            conf,
            ok,
            now,
        },
        BrainEvent::Tick(_) => BrainEvent::Tick(now),
        BrainEvent::SessionStart {
            selections,
            candidates,
            domain_to_cat,
            net,
            ..
        } => BrainEvent::SessionStart {
            selections,
            candidates,
            domain_to_cat,
            net,
            now,
        },
        other => other, // SessionStop / Shutdown — без времени
    }
}

/// Исполняет одно действие модели. Follow-up события (Respawn/Probe) шлёт обратно
/// в `tx` с плейсхолдер-временем — цикл перештампует при приёме.
/// ПРАВИЛО: не держим State-гард `AppState` через `.await`.
async fn exec(
    app: &AppHandle,
    net: &NetSnapshot,
    action: Action,
    status_tx: &watch::Sender<BrainStatus>,
) -> Option<BrainEvent> {
    match action {
        Action::Switch { selections } => {
            // Ворота: сериализуем с кликами юзера/трея (dpi_start/stop/test).
            let state = app.state::<AppState>();
            let _gate = state.dpi_gate.lock().await;
            let ok = crate::dpi::start_many(app, &selections).await.is_ok();
            drop(_gate);
            if ok {
                // Уведомляем: авто-восстановление подобрало другую стратегию.
                let cats = selections
                    .iter()
                    .map(|(c, _)| c.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                crate::util::notify_throttled(
                    app,
                    "recover",
                    "Obsession — обход восстановлен",
                    &format!("Авто-восстановление переключило стратегию ({cats})."),
                );
            }
            return Some(BrainEvent::RespawnResult { ok, now: 0 });
        }
        Action::StopBypass => {
            let state = app.state::<AppState>();
            let _gate = state.dpi_gate.lock().await;
            crate::dpi::stop_all(app).await;
        }
        Action::Probe { category, conf } => {
            let ok = {
                let state = app.state::<AppState>();
                let _gate = state.dpi_gate.lock().await;
                crate::dpi::test(app, &category, &conf).await
            };
            return Some(BrainEvent::ProbeResult {
                category,
                conf,
                ok,
                now: 0,
            });
        }
        Action::WriteCache { category, conf } => {
            write_cache(app, net, &category, &conf);
        }
        Action::EmitStatus(s) => {
            let _ = status_tx.send_replace(s.clone());
            let revision = app.state::<AppState>().brain_revision.bump();
            let _ = app.emit("brain://status", VersionedSection::new(revision, s));
        }
    }
    None
}

/// Записывает подтверждённо-рабочий конфиг в L1-кэш. Синхронная FS-операция без
/// `.await` — гард `AppState` не пересекает точку ожидания.
fn write_cache(app: &AppHandle, net: &NetSnapshot, category: &str, conf: &str) {
    let Some(mac) = net.gateway_mac.as_deref() else {
        return; // нет стабильного ключа сети — L1 не пишем
    };
    let paths = app.state::<AppState>().paths.clone();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let mut cache = NetCache::load(&paths);
    cache.put(mac, net.asn_region.as_deref(), category, conf, now);
    cache.save(&paths);
}

/// Собирает событие `SessionStart`: резолвит сеть, строит per-cat лестницу
/// кандидатов с метками [`Source`] (L1 кэш → L2 рейтинг → L3 полный список) и мост
/// `домен → категория`. Вызывается командным слоем (`dpi_start`) при включённом
/// Мозге. Здесь материализуется решение развилки §6: источник помечается меткой,
/// model про полосы L1/L2/L3 не знает.
pub async fn build_session_start(app: &AppHandle, selections: Vec<(String, String)>) -> BrainEvent {
    // Клонируем paths ДО await — гард AppState не должен пересекать точку ожидания.
    let paths = app.state::<AppState>().paths.clone();
    // Single-flight через netid_gate (тот же паттерн, что commands::current_netid):
    // без него Brain и UI-запрос get_network_identity могли бы одновременно
    // дернуть ipinfo дважды и разъехаться в записи netid_cache.json.
    let net_id = {
        let cached = app
            .state::<AppState>()
            .netid
            .lock()
            .ok()
            .and_then(|g| g.clone());
        match cached {
            Some(id) => id,
            None => {
                let state = app.state::<AppState>();
                let _gate = state.netid_gate.lock().await;
                // Повторная проверка под gate: конкурентный резолв мог уже
                // заполнить кэш, пока мы ждали на этом же gate.
                if let Some(id) = app
                    .state::<AppState>()
                    .netid
                    .lock()
                    .ok()
                    .and_then(|g| g.clone())
                {
                    id
                } else {
                    let id = netid::resolve(&paths).await;
                    if let Ok(mut slot) = app.state::<AppState>().netid.lock() {
                        *slot = Some(id.clone());
                    }
                    id
                }
            }
        }
    };

    // Сохраняем снимок идентичности в состояние (для лога/диагностики) + лог org.
    if let Some(org) = net_id.org.as_deref() {
        crate::util::emit_log(app, "info", "brain", &format!("Сеть: {org}"));
    }
    if let Ok(mut slot) = app.state::<AppState>().netid.lock() {
        *slot = Some(net_id.clone());
    }

    let ranking = Ranking::load(&paths);
    let netcache = NetCache::load(&paths);

    let mut candidates: HashMap<String, Vec<Candidate>> = HashMap::new();
    for (cat, _active) in &selections {
        let mut list: Vec<Candidate> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();

        // L1 — что работало в этой сети.
        if let Some(mac) = net_id.gateway_mac.as_deref() {
            if let Some(c) = netcache.get(mac, cat) {
                if seen.insert(c.clone()) {
                    list.push(Candidate {
                        conf: c,
                        source: Source::L1,
                    });
                }
            }
        }
        // L2 — курируемый рейтинг под ASN_region.
        for c in ranking.ranked_for(cat, net_id.asn_region.as_deref()) {
            if seen.insert(c.clone()) {
                list.push(Candidate {
                    conf: c,
                    source: Source::L2,
                });
            }
        }
        // L3 — полный список категории (последний рубеж).
        for c in paths.get_configs_for_category(cat) {
            if seen.insert(c.clone()) {
                list.push(Candidate {
                    conf: c,
                    source: Source::L3,
                });
            }
        }
        candidates.insert(cat.clone(), list);
    }

    // Мост домен→категория: только под windows (парсинг активных конфигов).
    #[cfg(windows)]
    let domain_to_cat = crate::dpi::collect_hostlist_by_category(app, &selections);
    #[cfg(not(windows))]
    let domain_to_cat: HashMap<String, String> = HashMap::new();

    let net = NetSnapshot {
        gateway_mac: net_id.gateway_mac.clone(),
        asn_region: net_id.asn_region.clone(),
    };

    BrainEvent::SessionStart {
        selections,
        candidates,
        domain_to_cat,
        net,
        now: 0, // перештампуется в цикле рантайма
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn observation_channel_is_bounded_but_control_remains_available() {
        let (observation_tx, mut observation_rx) = mpsc::channel(1);
        let (control_tx, mut control_rx) = mpsc::channel(1);
        let input = BrainInput::new(observation_tx, control_tx);

        assert_eq!(
            input.try_observation("one.example".into(), crate::eyes::Verdict::Working, 1),
            None
        );
        // Второе observation не помещается и дропается вместо роста памяти.
        assert_eq!(
            input.try_observation("two.example".into(), crate::eyes::Verdict::Reset, 2),
            Some(1)
        );
        assert!(matches!(
            observation_rx.recv().await,
            Some(BrainEvent::Observation { domain, .. }) if domain == "one.example"
        ));

        input.send_control(BrainEvent::SessionStop).await.unwrap();
        assert!(matches!(
            control_rx.recv().await,
            Some(BrainEvent::SessionStop)
        ));
    }
}
