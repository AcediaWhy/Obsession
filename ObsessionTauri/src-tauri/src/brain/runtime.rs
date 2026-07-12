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

use std::collections::{HashMap, HashSet};
use std::time::Instant;

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

/// Хендл на работающую задачу Мозга. Живёт в `AppState.brain`.
pub struct BrainHandle {
    /// Вход событий (Observation из Глаз, RespawnResult/ProbeResult из рантайма,
    /// SessionStart/Stop из командного слоя).
    pub tx: mpsc::UnboundedSender<BrainEvent>,
    /// Последний агрегированный статус (для `brain_get_status`).
    pub status: watch::Receiver<BrainStatus>,
    join: JoinHandle<()>,
}

impl BrainHandle {
    /// Останавливает задачу Мозга (после `SessionStop`, если он был отправлен).
    pub fn shutdown(self) {
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

/// Запускает задачу Мозга. Задача крутится, пока хендл жив (или `abort`).
pub fn start(app: AppHandle) -> BrainHandle {
    let (tx, mut rx) = mpsc::unbounded_channel::<BrainEvent>();
    let (status_tx, status_rx) = watch::channel(initial_status());

    let cfg = BrainCfg::default();
    let tick_ms = cfg.tick_ms.max(50);

    // Тикер-сосед: гонит таймауты/предохранитель без новых пакетов. Значение
    // Tick перештампуется в цикле, поэтому шлём плейсхолдер.
    let tick_tx = tx.clone();
    let ticker: JoinHandle<()> = async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_millis(tick_ms));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            if tick_tx.send(BrainEvent::Tick(0)).is_err() {
                break; // приёмник закрыт — задача Мозга завершилась
            }
        }
    });

    let loop_tx = tx.clone();
    let join = async_runtime::spawn(async move {
        let mut brain = Brain::new(cfg);
        let start = Instant::now();
        // Последний снимок сети — источник mac/asn для WriteCache.
        let mut net = NetSnapshot::default();

        while let Some(ev) = rx.recv().await {
            let now = start.elapsed().as_millis() as u64;
            let ev = restamp(ev, now);
            if let BrainEvent::SessionStart { net: n, .. } = &ev {
                net = n.clone();
            }
            for action in brain.step(ev) {
                exec(&app, &net, action, &loop_tx, &status_tx).await;
            }
        }
        ticker.abort();
    });

    BrainHandle {
        tx,
        status: status_rx,
        join,
    }
}

/// Перештамповывает временные поля события единой монотоникой рантайма.
fn restamp(ev: BrainEvent, now: u64) -> BrainEvent {
    match ev {
        BrainEvent::Observation { domain, verdict, .. } => BrainEvent::Observation {
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
    tx: &mpsc::UnboundedSender<BrainEvent>,
    status_tx: &watch::Sender<BrainStatus>,
) {
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
            let _ = tx.send(BrainEvent::RespawnResult { ok, now: 0 });
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
            let _ = tx.send(BrainEvent::ProbeResult {
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
            let _ = app.emit("brain://status", &s);
            let _ = status_tx.send_replace(s);
        }
    }
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
pub async fn build_session_start(
    app: &AppHandle,
    selections: Vec<(String, String)>,
) -> BrainEvent {
    // Клонируем paths ДО await — гард AppState не должен пересекать точку ожидания.
    let paths = app.state::<AppState>().paths.clone();
    let net_id = netid::resolve(&paths).await;

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
