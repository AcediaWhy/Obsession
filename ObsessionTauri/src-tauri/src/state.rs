//! Глобальное состояние приложения, управляемое Tauri (`app.state`).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;

use crate::paths::Paths;
use crate::settings::Settings;

/// Независимые subsystem revisions для listener-first hydration.
#[derive(Default)]
pub struct RevisionClock {
    value: AtomicU64,
}

impl RevisionClock {
    pub fn current(&self) -> u64 {
        self.value.load(Ordering::Acquire)
    }

    pub fn bump(&self) -> u64 {
        let mut current = self.value.load(Ordering::Relaxed);
        loop {
            let mut next = current.wrapping_add(1);
            if next == 0 {
                next = 1;
            }
            match self.value.compare_exchange_weak(
                current,
                next,
                Ordering::AcqRel,
                Ordering::Relaxed,
            ) {
                Ok(_) => return next,
                Err(actual) => current = actual,
            }
        }
    }
}

/// Точное описание реально запущенного DPI runtime. Оно не сериализуется и не
/// заменяет settings: это in-memory snapshot для generation-safe rollback.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DpiLaunchSpec {
    Legacy {
        selections: Vec<(String, String)>,
    },
    Zapret2 {
        selections: Vec<(String, String)>,
        adaptive_overrides: BTreeMap<String, crate::adaptive_strategy::dsl::StrategyCandidate>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DpiRuntimeSnapshot {
    pub generation: u64,
    pub launch: Option<DpiLaunchSpec>,
}

/// Отслеживаемый DPI-процесс winws.
pub struct DpiProc {
    pub pid: u32,
    pub category: String,
    pub config_file: String,
    pub generation: u64,
    pub engine: String,
    pub process_identity: Option<crate::dpi_supervisor::ProcessIdentity>,
}

#[derive(Default)]
pub struct DpiState {
    /// Revision последнего опубликованного dpi-status.
    pub revision: u64,
    /// Любой start/stop инвалидирует callbacks старого поколения.
    pub generation: u64,
    /// Активные (свои) процессы winws по PID.
    pub procs: HashMap<u32, DpiProc>,
    /// PID, которые останавливаем намеренно — чтобы монитор не считал это крахом.
    pub stopping: HashSet<u32>,
    /// Последний реально запущенный Legacy-набор для аварийного возврата из Beta.
    pub last_legacy_selection: Vec<(String, String)>,
    /// Точный активный запуск текущего generation. `None` означает stopped или
    /// промежуточное окно respawn.
    pub active_launch: Option<DpiLaunchSpec>,
    /// Unix-время (сек) появления ПЕРВОГО процесса текущей сессии обхода. Держится,
    /// пока `procs` непуст; сбрасывается при опустошении. Источник аптайма для UI —
    /// переживает смену вкладок и resume из трея (в отличие от клиентского Date.now).
    pub started_at_unix: Option<u64>,
}

impl DpiState {
    pub fn bump_revision(&mut self) -> u64 {
        self.revision = self.revision.wrapping_add(1);
        if self.revision == 0 {
            self.revision = 1;
        }
        self.revision
    }

    pub fn advance_generation(&mut self) -> u64 {
        self.generation = self.generation.wrapping_add(1);
        if self.generation == 0 {
            self.generation = 1;
        }
        self.generation
    }

    pub fn is_current_generation(&self, generation: u64) -> bool {
        generation != 0 && self.generation == generation
    }

    pub fn runtime_snapshot(&self) -> DpiRuntimeSnapshot {
        DpiRuntimeSnapshot {
            generation: self.generation,
            launch: self.active_launch.clone(),
        }
    }

    /// Поддерживает инвариант `started_at_unix = Some ⟺ procs непуст`. Возвращает
    /// текущее значение для payload. `now` — Unix-секунды (передаём снаружи, т.к.
    /// state не тянет системное время). Первый процесс сессии фиксирует старт;
    /// полное опустошение — сбрасывает. Промежуточный respawn (procs временно
    /// пуст, затем снова полон) НЕ сохраняет старое время — это новый запуск.
    pub fn sync_started_at(&mut self, now: u64) -> Option<u64> {
        if self.procs.is_empty() {
            self.started_at_unix = None;
        } else if self.started_at_unix.is_none() {
            self.started_at_unix = Some(now);
        }
        self.started_at_unix
    }

    /// Отделяет неожиданно завершившийся Zapret2 только если PID и generation
    /// всё ещё принадлежат текущему runtime. Intentional/stale exit → None.
    pub fn detach_unexpected_zapret2(
        &mut self,
        pid: u32,
        generation: u64,
    ) -> Option<Vec<(String, String)>> {
        let intentional = self.stopping.remove(&pid);
        let owned = self
            .procs
            .get(&pid)
            .is_some_and(|proc| proc.generation == generation && proc.engine == "zapret2");
        if owned {
            self.procs.remove(&pid);
            self.active_launch = None;
        }
        if intentional || !owned || !self.is_current_generation(generation) {
            return None;
        }
        Some(self.last_legacy_selection.clone())
    }
}

pub struct ProxyForwarder {
    pub generation: u64,
    pub handle: tokio::task::JoinHandle<()>,
}

pub struct ProxyFirewall {
    pub generation: u64,
    pub name: String,
}

#[derive(Default)]
pub struct ProxyState {
    /// Revision последнего опубликованного proxy-status.
    pub revision: u64,
    /// Монотонное поколение lifecycle. Любой start/stop инвалидирует callbacks
    /// предыдущего поколения, чтобы старый monitor не очищал новый запуск.
    pub generation: u64,
    pub pid: Option<u32>,
    pub link: String,
    pub lan_link: Option<String>,
    pub forwarder: Option<ProxyForwarder>,
    pub firewall: Option<ProxyFirewall>,
    /// Активна ли LAN-публикация (0.0.0.0 forwarder + firewall). Снимается по
    /// таймауту или вручную; локальный 127.0.0.1 прокси при этом продолжает жить.
    pub lan_published: bool,
    /// Unix-время (сек) авто-закрытия LAN-публикации; None = без авто-закрытия.
    pub lan_expiry_unix: Option<u64>,
}

impl ProxyState {
    pub fn bump_revision(&mut self) -> u64 {
        self.revision = self.revision.wrapping_add(1);
        if self.revision == 0 {
            self.revision = 1;
        }
        self.revision
    }
}

pub struct AppState {
    pub paths: Paths,
    /// Выставляется до teardown. Все новые start/test операции после этого
    /// отклоняются, пока background shutdown ждёт operation gates.
    pub shutting_down: AtomicBool,
    pub dpi: Mutex<DpiState>,
    /// Сериализует DPI-операции (start_many/stop_all/test) между собой. Без него
    /// частые клики по «Старт» запускают параллельные start/stop, которые топчут
    /// общий `procs` и поднимают Глаза на уже убитый winws → обход не детектится.
    /// Async-мьютекс (держится через `.await`), НЕ std — операции асинхронные.
    pub dpi_gate: tokio::sync::Mutex<()>,
    /// Флаг отмены текущего теста конфигов. Ставится командой `dpi_test_cancel`
    /// (без ворот, чтобы сработать, пока тест их держит); `dpi::test` проверяет
    /// его между этапами и обрывается досрочно, освобождая обход.
    pub test_cancel: AtomicBool,
    pub proxy: Mutex<ProxyState>,
    /// Сериализует proxy start/stop между UI, треем и shutdown.
    pub proxy_gate: tokio::sync::Mutex<()>,
    pub settings: Mutex<Settings>,
    pub settings_revision: RevisionClock,
    pub hosts_revision: RevisionClock,
    /// Активный наблюдатель трафика («Глаза»), пока запущен winws.
    #[cfg(windows)]
    pub eyes: Mutex<Option<crate::eyes::EyesHandle>>,
    /// Задача «Мозга» (L3), пока включено авто-восстановление.
    pub brain: Mutex<Option<crate::brain::runtime::BrainHandle>>,
    pub brain_revision: RevisionClock,
    /// Отдельный coordinator Safe Strategy DSL для Zapret2. Не разделяет
    /// очереди или state machine с Legacy Brain.
    pub adaptive: Mutex<Option<crate::adaptive_strategy::runtime::AdaptiveHandle>>,
    pub adaptive_revision: RevisionClock,
    /// Последний снимок сетевой идентичности (Менеджер сети, L2).
    pub netid: Mutex<Option<crate::netid::NetIdentity>>,
    /// Сериализует резолв сетевой идентичности: без него два конкурентных
    /// вызова оба видят пустой кэш и дважды дёргают ipinfo (лишний внешний
    /// round-trip + дребезг значения). Держится через `.await` резолва.
    pub netid_gate: tokio::sync::Mutex<()>,
}

impl AppState {
    pub fn new(paths: Paths, settings: Settings) -> Self {
        Self {
            paths,
            shutting_down: AtomicBool::new(false),
            dpi: Mutex::new(DpiState::default()),
            dpi_gate: tokio::sync::Mutex::new(()),
            test_cancel: AtomicBool::new(false),
            proxy: Mutex::new(ProxyState::default()),
            proxy_gate: tokio::sync::Mutex::new(()),
            settings: Mutex::new(settings),
            settings_revision: RevisionClock::default(),
            hosts_revision: RevisionClock::default(),
            #[cfg(windows)]
            eyes: Mutex::new(None),
            brain: Mutex::new(None),
            brain_revision: RevisionClock::default(),
            adaptive: Mutex::new(None),
            adaptive_revision: RevisionClock::default(),
            netid: Mutex::new(None),
            netid_gate: tokio::sync::Mutex::new(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revision_clock_is_monotonic() {
        let clock = RevisionClock::default();
        assert_eq!(clock.current(), 0);
        assert_eq!(clock.bump(), 1);
        assert_eq!(clock.bump(), 2);
        assert_eq!(clock.current(), 2);
    }

    #[test]
    fn revision_clocks_advance_independently() {
        let settings = RevisionClock::default();
        let hosts = RevisionClock::default();
        let brain = RevisionClock::default();
        let adaptive = RevisionClock::default();

        assert_eq!(brain.bump(), 1);
        assert_eq!(adaptive.bump(), 1);
        assert_eq!(adaptive.bump(), 2);
        assert_eq!(settings.current(), 0);
        assert_eq!(hosts.current(), 0);
        assert_eq!(brain.current(), 1);
        assert_eq!(adaptive.current(), 2);
    }

    #[test]
    fn dpi_revision_is_monotonic_and_skips_zero() {
        let mut state = DpiState {
            revision: u64::MAX,
            ..DpiState::default()
        };
        assert_eq!(state.bump_revision(), 1);
        assert_eq!(state.bump_revision(), 2);
    }

    #[test]
    fn proxy_revision_is_monotonic_and_skips_zero() {
        let mut state = ProxyState {
            revision: u64::MAX,
            ..ProxyState::default()
        };
        assert_eq!(state.bump_revision(), 1);
        assert_eq!(state.bump_revision(), 2);
    }

    #[test]
    fn dpi_generation_never_uses_zero_after_wrap() {
        let mut state = DpiState {
            generation: u64::MAX,
            ..DpiState::default()
        };
        assert_eq!(state.advance_generation(), 1);
        assert!(state.is_current_generation(1));
        assert!(!state.is_current_generation(u64::MAX));
    }

    #[test]
    fn legacy_selection_survives_generation_changes() {
        let mut state = DpiState {
            last_legacy_selection: vec![("discord".into(), "discord_1.conf".into())],
            ..Default::default()
        };
        state.advance_generation();
        assert_eq!(state.last_legacy_selection[0].0, "discord");
    }

    #[test]
    fn runtime_snapshot_preserves_exact_launch_spec() {
        let mut state = DpiState::default();
        let generation = state.advance_generation();
        state.active_launch = Some(DpiLaunchSpec::Legacy {
            selections: vec![("discord".into(), "discord_1.conf".into())],
        });
        let snapshot = state.runtime_snapshot();
        assert_eq!(snapshot.generation, generation);
        assert_eq!(snapshot.launch, state.active_launch);
    }

    #[test]
    fn only_current_unexpected_zapret2_exit_gets_fallback() {
        let mut state = DpiState::default();
        let generation = state.advance_generation();
        state.last_legacy_selection = vec![("discord".into(), "discord_1.conf".into())];
        state.procs.insert(
            10,
            DpiProc {
                pid: 10,
                category: "zapret2".into(),
                config_file: "beta".into(),
                generation,
                engine: "zapret2".into(),
                process_identity: None,
            },
        );
        assert_eq!(
            state.detach_unexpected_zapret2(10, generation),
            Some(vec![("discord".into(), "discord_1.conf".into())])
        );

        let next = state.advance_generation();
        state.procs.insert(
            11,
            DpiProc {
                pid: 11,
                category: "zapret2".into(),
                config_file: "beta".into(),
                generation: next,
                engine: "zapret2".into(),
                process_identity: None,
            },
        );
        assert_eq!(state.detach_unexpected_zapret2(11, generation), None);
        assert!(
            state.procs.contains_key(&11),
            "stale monitor не трогает новый PID"
        );

        state.stopping.insert(11);
        assert_eq!(state.detach_unexpected_zapret2(11, next), None);
        assert!(!state.procs.contains_key(&11));
    }

    fn insert_proc(state: &mut DpiState, pid: u32) {
        let generation = state.generation;
        state.procs.insert(
            pid,
            DpiProc {
                pid,
                category: "discord".into(),
                config_file: "discord_1.conf".into(),
                generation,
                engine: "legacy".into(),
                process_identity: None,
            },
        );
    }

    #[test]
    fn started_at_set_on_first_proc_and_cleared_when_empty() {
        let mut state = DpiState::default();
        // Нет процессов — старт не зафиксирован.
        assert_eq!(state.sync_started_at(1000), None);
        assert_eq!(state.started_at_unix, None);

        // Первый процесс фиксирует время старта.
        insert_proc(&mut state, 10);
        assert_eq!(state.sync_started_at(1000), Some(1000));

        // Опустошение сбрасывает.
        state.procs.clear();
        assert_eq!(state.sync_started_at(2000), None);
        assert_eq!(state.started_at_unix, None);
    }

    #[test]
    fn started_at_not_overwritten_while_session_stays_active() {
        let mut state = DpiState::default();
        insert_proc(&mut state, 10);
        assert_eq!(state.sync_started_at(1000), Some(1000));

        // Второй процесс той же сессии НЕ сдвигает время старта.
        insert_proc(&mut state, 11);
        assert_eq!(state.sync_started_at(1500), Some(1000));

        // Уход одного из двух процессов — сессия жива, время старта прежнее.
        state.procs.remove(&11);
        assert_eq!(state.sync_started_at(1800), Some(1000));
    }

    #[test]
    fn started_at_resets_for_new_session_after_full_stop() {
        let mut state = DpiState::default();
        insert_proc(&mut state, 10);
        assert_eq!(state.sync_started_at(1000), Some(1000));

        // Полная остановка (respawn): procs пуст → сброс.
        state.procs.clear();
        assert_eq!(state.sync_started_at(1200), None);

        // Новый запуск фиксирует НОВОЕ время, а не старое.
        insert_proc(&mut state, 20);
        assert_eq!(state.sync_started_at(1500), Some(1500));
    }
}
