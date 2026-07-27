//! Глобальное состояние приложения, управляемое Tauri (`app.state`).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::error::Error;
use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use crate::legacy_reliability::contracts::LaneGeneration;
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
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DpiProc {
    pub pid: u32,
    pub category: String,
    pub config_file: String,
    pub generation: u64,
    /// Per-category epoch. `None` is reserved for compatibility/Zapret2
    /// processes and is never accepted by the scoped Legacy executor.
    pub lane_generation: Option<LaneGeneration>,
    /// Fingerprint of the exact config plus referenced resources. A missing or
    /// empty value makes the process ineligible for scoped replacement.
    pub config_fingerprint: Option<String>,
    pub engine: String,
    pub process_identity: Option<crate::dpi_supervisor::ProcessIdentity>,
}

/// Unforgeable-enough in-memory ownership token for one exact Legacy process.
/// PID alone is intentionally insufficient because Windows can reuse it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct LegacyProcessOwner {
    pub pid: u32,
    pub process_identity: crate::dpi_supervisor::ProcessIdentity,
    pub runtime_generation: u64,
    pub lane_generation: LaneGeneration,
    pub category: String,
    pub config_file: String,
    pub config_fingerprint: String,
}

impl LegacyProcessOwner {
    pub fn matches(&self, process: &DpiProc) -> bool {
        process.engine == "legacy"
            && process.pid == self.pid
            && process.process_identity == Some(self.process_identity)
            && process.generation == self.runtime_generation
            && process.lane_generation == Some(self.lane_generation)
            && process.category == self.category
            && process.config_file == self.config_file
            && process.config_fingerprint.as_deref() == Some(self.config_fingerprint.as_str())
    }
}

impl DpiProc {
    /// Builds an exact owner only for fully-fenced Legacy processes. Old
    /// compatibility launches fail closed instead of silently falling back to
    /// PID-only ownership.
    pub fn exact_legacy_owner(&self) -> Option<LegacyProcessOwner> {
        if self.engine != "legacy"
            || self.generation == 0
            || self.category.trim().is_empty()
            || self.config_file.trim().is_empty()
        {
            return None;
        }
        let process_identity = self.process_identity?;
        let lane_generation = self.lane_generation?;
        let config_fingerprint = self.config_fingerprint.as_ref()?;
        if lane_generation.get() == 0 || config_fingerprint.trim().is_empty() {
            return None;
        }
        Some(LegacyProcessOwner {
            pid: self.pid,
            process_identity,
            runtime_generation: self.generation,
            lane_generation,
            category: self.category.clone(),
            config_file: self.config_file.clone(),
            config_fingerprint: config_fingerprint.clone(),
        })
    }
}

/// Exact rollback anchor for a category. `selection_index` preserves stable UI
/// ordering while removal/reinstall touches no neighboring selection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LegacyCategoryRuntimeSnapshot {
    pub owner: LegacyProcessOwner,
    pub selection_index: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LegacyCategoryRemoval {
    /// `None` means an exact monitor already finalized the same removal.
    pub process: Option<DpiProc>,
    pub intentional: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LegacyCategoryStateError {
    NotLegacyRuntime,
    RuntimeInconsistent,
    CategoryNotFound,
    DuplicateCategory,
    MissingExactOwnership,
    StaleOwner,
    AlreadyStopping,
    CategoryOccupied,
    InvalidReplacement,
}

impl fmt::Display for LegacyCategoryStateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::NotLegacyRuntime => "DPI runtime is not Legacy",
            Self::RuntimeInconsistent => "Legacy process ownership and active launch disagree",
            Self::CategoryNotFound => "Legacy category is not active",
            Self::DuplicateCategory => "more than one process or selection owns the category",
            Self::MissingExactOwnership => {
                "Legacy process is missing identity, lane generation, or config fingerprint"
            }
            Self::StaleOwner => "Legacy category owner is stale",
            Self::AlreadyStopping => "Legacy category owner is already stopping",
            Self::CategoryOccupied => "Legacy category already has a process or selection",
            Self::InvalidReplacement => "replacement process does not satisfy the exact snapshot",
        };
        formatter.write_str(message)
    }
}

impl Error for LegacyCategoryStateError {}

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
    /// Scoped Legacy stops use the complete owner token. Kept alongside the
    /// PID-only compatibility set until all global start/stop call sites migrate.
    pub legacy_stopping: HashSet<LegacyProcessOwner>,
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
        self.legacy_stopping.clear();
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

    /// Checks the stable-state invariant used by scoped Legacy mutations:
    /// every live Legacy category has exactly one selection with the same
    /// config and no Zapret2 process is mixed into that launch.
    fn validate_legacy_runtime(&self) -> Result<(), LegacyCategoryStateError> {
        let legacy_processes = self
            .procs
            .values()
            .filter(|process| process.engine == "legacy")
            .collect::<Vec<_>>();

        let selections = match self.active_launch.as_ref() {
            Some(DpiLaunchSpec::Legacy { selections }) => selections,
            Some(DpiLaunchSpec::Zapret2 { .. }) => {
                return Err(LegacyCategoryStateError::NotLegacyRuntime);
            }
            None if legacy_processes.is_empty() && self.procs.is_empty() => return Ok(()),
            None => return Err(LegacyCategoryStateError::RuntimeInconsistent),
        };

        if self.procs.len() != legacy_processes.len() {
            return Err(LegacyCategoryStateError::RuntimeInconsistent);
        }

        let mut selected = BTreeMap::new();
        for (category, config_file) in selections {
            if selected
                .insert(category.as_str(), config_file.as_str())
                .is_some()
            {
                return Err(LegacyCategoryStateError::DuplicateCategory);
            }
        }
        let mut running = BTreeMap::new();
        for process in legacy_processes {
            if running.insert(process.category.as_str(), process).is_some() {
                return Err(LegacyCategoryStateError::DuplicateCategory);
            }
        }
        if selected.len() != running.len()
            || selected.iter().any(|(category, config_file)| {
                running
                    .get(category)
                    .is_none_or(|process| process.config_file != *config_file)
            })
        {
            return Err(LegacyCategoryStateError::RuntimeInconsistent);
        }
        Ok(())
    }

    /// Captures the exact process/config/generations that own one category.
    /// Any incomplete compatibility process fails closed.
    pub fn snapshot_legacy_category(
        &self,
        category: &str,
    ) -> Result<LegacyCategoryRuntimeSnapshot, LegacyCategoryStateError> {
        self.validate_legacy_runtime()?;
        let selections = match self.active_launch.as_ref() {
            Some(DpiLaunchSpec::Legacy { selections }) => selections,
            _ => return Err(LegacyCategoryStateError::NotLegacyRuntime),
        };

        let matching_selections = selections
            .iter()
            .enumerate()
            .filter(|(_, (selected_category, _))| selected_category == category)
            .collect::<Vec<_>>();
        let [(selection_index, (_, selected_config))] = matching_selections.as_slice() else {
            return Err(if matching_selections.is_empty() {
                LegacyCategoryStateError::CategoryNotFound
            } else {
                LegacyCategoryStateError::DuplicateCategory
            });
        };

        let matching_processes = self
            .procs
            .values()
            .filter(|process| process.engine == "legacy" && process.category == category)
            .collect::<Vec<_>>();
        let [process] = matching_processes.as_slice() else {
            return Err(if matching_processes.is_empty() {
                LegacyCategoryStateError::CategoryNotFound
            } else {
                LegacyCategoryStateError::DuplicateCategory
            });
        };
        if process.config_file != *selected_config {
            return Err(LegacyCategoryStateError::RuntimeInconsistent);
        }
        let owner = process
            .exact_legacy_owner()
            .ok_or(LegacyCategoryStateError::MissingExactOwnership)?;
        if owner.runtime_generation != self.generation {
            return Err(LegacyCategoryStateError::StaleOwner);
        }
        Ok(LegacyCategoryRuntimeSnapshot {
            owner,
            selection_index: *selection_index,
        })
    }

    /// Marks only the exact owner intentional. A reused PID or a restarted lane
    /// cannot consume this marker.
    pub fn mark_legacy_category_stopping(
        &mut self,
        snapshot: &LegacyCategoryRuntimeSnapshot,
    ) -> Result<(), LegacyCategoryStateError> {
        let current = self.snapshot_legacy_category(&snapshot.owner.category)?;
        if current != *snapshot {
            return Err(LegacyCategoryStateError::StaleOwner);
        }
        if !self.legacy_stopping.insert(snapshot.owner.clone()) {
            return Err(LegacyCategoryStateError::AlreadyStopping);
        }
        Ok(())
    }

    /// Finalizes one exact process exit and creates the provisional selection
    /// absence used by candidate start. It is idempotent for the same snapshot,
    /// but rejects a new owner that appeared under the same category or PID.
    pub fn remove_exact_legacy_category(
        &mut self,
        snapshot: &LegacyCategoryRuntimeSnapshot,
    ) -> Result<LegacyCategoryRemoval, LegacyCategoryStateError> {
        if !self.is_current_generation(snapshot.owner.runtime_generation) {
            return Err(LegacyCategoryStateError::StaleOwner);
        }
        self.validate_legacy_runtime()?;

        let category_processes = self
            .procs
            .values()
            .filter(|process| {
                process.engine == "legacy" && process.category == snapshot.owner.category
            })
            .collect::<Vec<_>>();
        if category_processes.len() > 1 {
            return Err(LegacyCategoryStateError::DuplicateCategory);
        }
        if let Some(process) = category_processes.first() {
            if !snapshot.owner.matches(process) {
                return Err(LegacyCategoryStateError::StaleOwner);
            }
        }

        let mut selection_index = None;
        match self.active_launch.as_ref() {
            Some(DpiLaunchSpec::Legacy { selections }) => {
                for (index, (category, config_file)) in selections.iter().enumerate() {
                    if category == &snapshot.owner.category {
                        if selection_index.is_some() {
                            return Err(LegacyCategoryStateError::DuplicateCategory);
                        }
                        if config_file != &snapshot.owner.config_file {
                            return Err(LegacyCategoryStateError::StaleOwner);
                        }
                        selection_index = Some(index);
                    }
                }
            }
            Some(DpiLaunchSpec::Zapret2 { .. }) => {
                return Err(LegacyCategoryStateError::NotLegacyRuntime);
            }
            None if self.procs.is_empty() => {}
            None => return Err(LegacyCategoryStateError::RuntimeInconsistent),
        }
        if category_processes.is_empty() != selection_index.is_none() {
            return Err(LegacyCategoryStateError::RuntimeInconsistent);
        }

        let process = self.procs.remove(&snapshot.owner.pid);
        let mut launch_became_empty = false;
        if let Some(index) = selection_index {
            let Some(DpiLaunchSpec::Legacy { selections }) = self.active_launch.as_mut() else {
                return Err(LegacyCategoryStateError::RuntimeInconsistent);
            };
            selections.remove(index);
            launch_became_empty = selections.is_empty();
        }
        if launch_became_empty {
            self.active_launch = None;
        }
        let intentional = self.legacy_stopping.remove(&snapshot.owner);
        Ok(LegacyCategoryRemoval {
            process,
            intentional,
        })
    }

    fn install_legacy_category(
        &mut self,
        anchor: &LegacyCategoryRuntimeSnapshot,
        process: DpiProc,
        exact_previous_config: bool,
        superseded_lane: Option<LaneGeneration>,
    ) -> Result<LegacyProcessOwner, LegacyCategoryStateError> {
        if !self.is_current_generation(anchor.owner.runtime_generation) {
            return Err(LegacyCategoryStateError::StaleOwner);
        }
        self.validate_legacy_runtime()?;
        let owner = process
            .exact_legacy_owner()
            .ok_or(LegacyCategoryStateError::MissingExactOwnership)?;
        if owner.runtime_generation != self.generation
            || owner.category != anchor.owner.category
            || owner.lane_generation == anchor.owner.lane_generation
            || superseded_lane.is_some_and(|generation| generation == owner.lane_generation)
            || (exact_previous_config
                && (owner.config_file != anchor.owner.config_file
                    || owner.config_fingerprint != anchor.owner.config_fingerprint))
        {
            return Err(LegacyCategoryStateError::InvalidReplacement);
        }
        if self.procs.contains_key(&owner.pid)
            || self.procs.values().any(|running| {
                running.engine == "legacy" && running.category == anchor.owner.category
            })
        {
            return Err(LegacyCategoryStateError::CategoryOccupied);
        }
        match self.active_launch.as_ref() {
            Some(DpiLaunchSpec::Legacy { selections })
                if selections
                    .iter()
                    .all(|(category, _)| category != &anchor.owner.category) => {}
            Some(DpiLaunchSpec::Legacy { .. }) => {
                return Err(LegacyCategoryStateError::CategoryOccupied);
            }
            Some(DpiLaunchSpec::Zapret2 { .. }) => {
                return Err(LegacyCategoryStateError::NotLegacyRuntime);
            }
            None if self.procs.is_empty() => {}
            None => return Err(LegacyCategoryStateError::RuntimeInconsistent),
        }

        let selection = (owner.category.clone(), owner.config_file.clone());
        match self.active_launch.as_mut() {
            Some(DpiLaunchSpec::Legacy { selections }) => {
                selections.insert(anchor.selection_index.min(selections.len()), selection);
            }
            None => {
                self.active_launch = Some(DpiLaunchSpec::Legacy {
                    selections: vec![selection],
                });
            }
            Some(DpiLaunchSpec::Zapret2 { .. }) => unreachable!("validated above"),
        }
        self.procs.insert(owner.pid, process);
        self.legacy_stopping.remove(&anchor.owner);
        Ok(owner)
    }

    /// Installs a candidate (or same-config retry) into the exact provisional
    /// absence created from `previous`.
    pub fn commit_legacy_category_replacement(
        &mut self,
        previous: &LegacyCategoryRuntimeSnapshot,
        replacement: DpiProc,
    ) -> Result<LegacyProcessOwner, LegacyCategoryStateError> {
        self.install_legacy_category(previous, replacement, false, None)
    }

    /// Restores the exact previous config/fingerprint after a failed candidate.
    /// The restored process must have a fresh lane generation relative to both
    /// the previous and failed candidate owners.
    pub fn rollback_legacy_category_replacement(
        &mut self,
        previous: &LegacyCategoryRuntimeSnapshot,
        failed_candidate: &LegacyCategoryRuntimeSnapshot,
        restored: DpiProc,
    ) -> Result<LegacyProcessOwner, LegacyCategoryStateError> {
        if failed_candidate.owner.category != previous.owner.category
            || failed_candidate.owner.runtime_generation != previous.owner.runtime_generation
        {
            return Err(LegacyCategoryStateError::InvalidReplacement);
        }
        self.install_legacy_category(
            previous,
            restored,
            true,
            Some(failed_candidate.owner.lane_generation),
        )
    }

    /// Verifies that a delayed Legacy startup continuation still refers to
    /// the exact live process set it created. Process monitors may mutate this
    /// state while startup is sleeping or building the registry.
    pub fn owns_exact_legacy_runtime(
        &self,
        generation: u64,
        pids: &[u32],
        selections: &[(String, String)],
    ) -> bool {
        if !self.is_current_generation(generation)
            || pids.len() != selections.len()
            || self.procs.len() != pids.len()
            || !matches!(
                self.active_launch.as_ref(),
                Some(DpiLaunchSpec::Legacy { selections: active }) if active == selections
            )
        {
            return false;
        }

        pids.iter().all(|pid| {
            self.procs.get(pid).is_some_and(|process| {
                process.generation == generation && process.engine == "legacy"
            })
        }) && selections.iter().all(|(category, config_file)| {
            self.procs
                .values()
                .any(|process| process.category == *category && process.config_file == *config_file)
        })
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
    /// AbortHandle таймера авто-закрытия: отменяется при stop/shutdown, чтобы
    /// не копились спящие задачи (раньше жили до пробуждения даже после stop).
    pub lan_expiry_abort: Option<tokio::task::AbortHandle>,
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
    /// Сериализует запись settings.json на диск (fsync+rename) ОТДЕЛЬНО от
    /// std::Mutex settings: дисковый I/O не должен блокировать читателей
    /// настроек (dpi_start, CloseRequested, хоткей). Держится коротко и только
    /// вокруг save — вне лока settings.
    pub settings_save_gate: tokio::sync::Mutex<()>,
    pub settings_revision: RevisionClock,
    /// Сериализует hosts-операции (install/uninstall/restore) на всё время
    /// чтение→сеть→запись: два конкурентных install не должны смешивать
    /// pre-op снапшоты и затирать чужие изменения.
    pub hosts_gate: tokio::sync::Mutex<()>,
    pub hosts_revision: RevisionClock,
    /// Активный наблюдатель трафика («Глаза»), пока запущен winws.
    #[cfg(windows)]
    pub eyes: Mutex<Option<crate::eyes::EyesHandle>>,
    /// Bounded Eyes joins that have not yet produced terminal worker evidence.
    /// A replacement observer is forbidden while this set is non-empty.
    #[cfg(windows)]
    pub eyes_teardowns: Mutex<Vec<crate::dpi_supervisor::WorkerTeardown>>,
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
    /// Observe-only Legacy Reliability Manager. Не пересекается с Brain и
    /// adaptive Zapret2 coordinator; хранит только текущую session ingress.
    pub legacy_manager: Mutex<Option<crate::legacy_reliability::runtime::LegacyReliabilityHandle>>,
    /// Последняя публичная lifecycle/health проекция observe-only Manager.
    pub legacy_reliability_status:
        Mutex<crate::legacy_reliability::status::LegacyReliabilityStatus>,
    pub legacy_reliability_revision: RevisionClock,
    /// Process-owned Phase 3/4 recovery coordinator. Assisted approval and
    /// Automatic execution both refer only to backend-owned state in this instance.
    pub legacy_recovery: Mutex<crate::legacy_reliability::recovery_runtime::LegacyRecoveryRuntime>,
    /// Changes only when Automatic authorization changes (mode, global pause,
    /// or frozen categories). Automatic actions carry the exact value so a
    /// pause->resume race cannot revive an older queued action.
    pub legacy_automation_revision: RevisionClock,
    /// Dedicated Phase 4 trust memory. It is intentionally separate from the
    /// compatibility `netcache.json`, which can be seeded by manual UI tests.
    pub legacy_trust_cache: Mutex<crate::legacy_reliability::cache::LegacyTrustCacheStore>,
    /// Process-unique component of persisted confirmation session keys. Raw
    /// Legacy SessionId values restart from one after an application restart.
    pub legacy_cache_boot_nonce: u128,
    /// Exact owners installed by the one-shot same-config crash retry. If one
    /// of these owners exits unexpectedly, the incident is exhausted and must
    /// not recursively start another retry.
    pub legacy_crash_retry_owners: Mutex<HashSet<LegacyProcessOwner>>,
    /// One process-wide monotonic epoch shared by assessment reconciliation,
    /// proposal TTLs and executor results. It deliberately survives Legacy
    /// session and Eyes generation restarts.
    legacy_monotonic_origin: Instant,
    /// Monotonic session/sensor clocks. Они не используют Observation.ts_ms:
    /// часы Eyes сбрасываются при каждом restart.
    pub legacy_session_revision: RevisionClock,
    pub legacy_sensor_revision: RevisionClock,
}

impl AppState {
    pub fn new(paths: Paths, settings: Settings) -> Self {
        let legacy_trust_cache =
            crate::legacy_reliability::cache::LegacyTrustCacheStore::load(&paths);
        let legacy_cache_boot_nonce = rand::random::<u128>();
        let recovery_mode = if settings.legacy_reliability_enabled {
            match settings.legacy_reliability_mode.as_str() {
                "assisted" => crate::legacy_reliability::recovery::RecoveryMode::Assisted,
                "automatic" => crate::legacy_reliability::recovery::RecoveryMode::Automatic,
                _ => crate::legacy_reliability::recovery::RecoveryMode::ObserveOnly,
            }
        } else {
            crate::legacy_reliability::recovery::RecoveryMode::ObserveOnly
        };
        let mut legacy_recovery =
            crate::legacy_reliability::recovery_runtime::LegacyRecoveryRuntime::new(recovery_mode);
        legacy_recovery.set_automatic_paused(
            !settings.legacy_reliability_enabled || settings.legacy_automatic_paused,
        );
        for category in &settings.legacy_reliability_frozen_categories {
            legacy_recovery.freeze_category(category.clone());
        }
        let mut initial_reliability =
            crate::legacy_reliability::status::LegacyReliabilityStatus::inactive();
        initial_reliability.mode = match recovery_mode {
            crate::legacy_reliability::recovery::RecoveryMode::ObserveOnly => {
                crate::legacy_reliability::status::LegacyReliabilityMode::ObserveOnly
            }
            crate::legacy_reliability::recovery::RecoveryMode::Assisted => {
                crate::legacy_reliability::status::LegacyReliabilityMode::Assisted
            }
            crate::legacy_reliability::recovery::RecoveryMode::Automatic => {
                crate::legacy_reliability::status::LegacyReliabilityMode::Automatic
            }
        };
        initial_reliability.automatic_paused =
            !settings.legacy_reliability_enabled || settings.legacy_automatic_paused;
        initial_reliability.frozen_categories =
            settings.legacy_reliability_frozen_categories.clone();
        Self {
            paths,
            shutting_down: AtomicBool::new(false),
            dpi: Mutex::new(DpiState::default()),
            dpi_gate: tokio::sync::Mutex::new(()),
            test_cancel: AtomicBool::new(false),
            proxy: Mutex::new(ProxyState::default()),
            proxy_gate: tokio::sync::Mutex::new(()),
            settings: Mutex::new(settings),
            settings_save_gate: tokio::sync::Mutex::new(()),
            settings_revision: RevisionClock::default(),
            hosts_gate: tokio::sync::Mutex::new(()),
            hosts_revision: RevisionClock::default(),
            #[cfg(windows)]
            eyes: Mutex::new(None),
            #[cfg(windows)]
            eyes_teardowns: Mutex::new(Vec::new()),
            brain: Mutex::new(None),
            brain_revision: RevisionClock::default(),
            adaptive: Mutex::new(None),
            adaptive_revision: RevisionClock::default(),
            netid: Mutex::new(None),
            netid_gate: tokio::sync::Mutex::new(()),
            legacy_manager: Mutex::new(None),
            legacy_reliability_status: Mutex::new(initial_reliability),
            legacy_reliability_revision: RevisionClock::default(),
            legacy_recovery: Mutex::new(legacy_recovery),
            legacy_automation_revision: RevisionClock::default(),
            legacy_trust_cache: Mutex::new(legacy_trust_cache),
            legacy_cache_boot_nonce,
            legacy_crash_retry_owners: Mutex::new(HashSet::new()),
            legacy_monotonic_origin: Instant::now(),
            legacy_session_revision: RevisionClock::default(),
            legacy_sensor_revision: RevisionClock::default(),
        }
    }

    /// Milliseconds since the process-wide recovery epoch. Saturation keeps
    /// the clock monotonic even on an unrealistically long-running process.
    pub fn legacy_monotonic_ms(&self) -> u64 {
        self.legacy_monotonic_origin
            .elapsed()
            .as_millis()
            .min(u128::from(u64::MAX)) as u64
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
        let legacy_reliability = RevisionClock::default();

        assert_eq!(brain.bump(), 1);
        assert_eq!(adaptive.bump(), 1);
        assert_eq!(adaptive.bump(), 2);
        assert_eq!(legacy_reliability.bump(), 1);
        assert_eq!(settings.current(), 0);
        assert_eq!(hosts.current(), 0);
        assert_eq!(brain.current(), 1);
        assert_eq!(adaptive.current(), 2);
        assert_eq!(legacy_reliability.current(), 1);
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
    fn exact_legacy_runtime_rejects_stale_or_missing_processes() {
        let mut state = DpiState::default();
        let generation = state.advance_generation();
        let selections = vec![("discord".into(), "discord_1.conf".into())];
        state.active_launch = Some(DpiLaunchSpec::Legacy {
            selections: selections.clone(),
        });
        insert_proc(&mut state, 10);

        assert!(state.owns_exact_legacy_runtime(generation, &[10], &selections));
        assert!(!state.owns_exact_legacy_runtime(generation.wrapping_add(1), &[10], &selections));
        state.procs.clear();
        assert!(!state.owns_exact_legacy_runtime(generation, &[10], &selections));
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
                lane_generation: None,
                config_fingerprint: None,
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
                lane_generation: None,
                config_fingerprint: None,
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
                lane_generation: None,
                config_fingerprint: None,
                engine: "legacy".into(),
                process_identity: None,
            },
        );
    }

    fn exact_legacy_proc(
        pid: u32,
        identity: u64,
        generation: u64,
        lane_generation: u64,
        category: &str,
        config_file: &str,
        config_fingerprint: &str,
    ) -> DpiProc {
        DpiProc {
            pid,
            category: category.into(),
            config_file: config_file.into(),
            generation,
            lane_generation: Some(LaneGeneration::new(lane_generation)),
            config_fingerprint: Some(config_fingerprint.into()),
            engine: "legacy".into(),
            process_identity: Some(crate::dpi_supervisor::ProcessIdentity::from_raw(identity)),
        }
    }

    fn two_category_legacy_state() -> DpiState {
        let generation = 7;
        let discord = exact_legacy_proc(
            10,
            110,
            generation,
            1,
            "discord",
            "discord_1.conf",
            "discord-fingerprint-1",
        );
        let youtube = exact_legacy_proc(
            20,
            120,
            generation,
            4,
            "youtube",
            "youtube_1.conf",
            "youtube-fingerprint-1",
        );
        DpiState {
            generation,
            procs: HashMap::from([(discord.pid, discord), (youtube.pid, youtube)]),
            active_launch: Some(DpiLaunchSpec::Legacy {
                selections: vec![
                    ("discord".into(), "discord_1.conf".into()),
                    ("youtube".into(), "youtube_1.conf".into()),
                ],
            }),
            ..DpiState::default()
        }
    }

    #[test]
    fn scoped_replacement_preserves_neighbor_pid_identity_and_generations() {
        let mut state = two_category_legacy_state();
        let neighbor_before = state.procs.get(&20).unwrap().clone();
        let previous = state.snapshot_legacy_category("discord").unwrap();

        state.mark_legacy_category_stopping(&previous).unwrap();
        let removal = state.remove_exact_legacy_category(&previous).unwrap();
        assert!(removal.intentional);
        assert_eq!(removal.process.unwrap().pid, 10);
        assert_eq!(state.procs.get(&20), Some(&neighbor_before));
        assert_eq!(
            state.active_launch,
            Some(DpiLaunchSpec::Legacy {
                selections: vec![("youtube".into(), "youtube_1.conf".into())],
            })
        );

        let candidate = exact_legacy_proc(
            11,
            111,
            state.generation,
            2,
            "discord",
            "discord_2.conf",
            "discord-fingerprint-2",
        );
        state
            .commit_legacy_category_replacement(&previous, candidate)
            .unwrap();

        assert_eq!(state.procs.get(&20), Some(&neighbor_before));
        assert_eq!(
            state.active_launch,
            Some(DpiLaunchSpec::Legacy {
                selections: vec![
                    ("discord".into(), "discord_2.conf".into()),
                    ("youtube".into(), "youtube_1.conf".into()),
                ],
            })
        );
    }

    #[test]
    fn exact_selection_replacement_keeps_neighbor_order() {
        let mut state = two_category_legacy_state();
        let previous = state.snapshot_legacy_category("youtube").unwrap();
        state.mark_legacy_category_stopping(&previous).unwrap();
        state.remove_exact_legacy_category(&previous).unwrap();
        state
            .commit_legacy_category_replacement(
                &previous,
                exact_legacy_proc(
                    21,
                    121,
                    state.generation,
                    5,
                    "youtube",
                    "youtube_2.conf",
                    "youtube-fingerprint-2",
                ),
            )
            .unwrap();

        assert_eq!(
            state.active_launch,
            Some(DpiLaunchSpec::Legacy {
                selections: vec![
                    ("discord".into(), "discord_1.conf".into()),
                    ("youtube".into(), "youtube_2.conf".into()),
                ],
            })
        );
    }

    #[test]
    fn stale_owner_cannot_mark_or_remove_reused_pid() {
        let mut state = two_category_legacy_state();
        let stale = state.snapshot_legacy_category("discord").unwrap();
        let reused = exact_legacy_proc(
            stale.owner.pid,
            999,
            state.generation,
            2,
            "discord",
            "discord_1.conf",
            "discord-fingerprint-1",
        );
        state.procs.insert(reused.pid, reused.clone());

        assert_eq!(
            state.mark_legacy_category_stopping(&stale),
            Err(LegacyCategoryStateError::StaleOwner)
        );
        assert_eq!(
            state.remove_exact_legacy_category(&stale),
            Err(LegacyCategoryStateError::StaleOwner)
        );
        assert_eq!(state.procs.get(&stale.owner.pid), Some(&reused));
        assert!(state.legacy_stopping.is_empty());
    }

    #[test]
    fn rollback_restores_exact_previous_config_without_touching_neighbor() {
        let mut state = two_category_legacy_state();
        let neighbor_before = state.procs.get(&20).unwrap().clone();
        let previous = state.snapshot_legacy_category("discord").unwrap();
        state.mark_legacy_category_stopping(&previous).unwrap();
        state.remove_exact_legacy_category(&previous).unwrap();
        state
            .commit_legacy_category_replacement(
                &previous,
                exact_legacy_proc(
                    11,
                    111,
                    state.generation,
                    2,
                    "discord",
                    "discord_2.conf",
                    "discord-fingerprint-2",
                ),
            )
            .unwrap();

        let failed_candidate = state.snapshot_legacy_category("discord").unwrap();
        state
            .mark_legacy_category_stopping(&failed_candidate)
            .unwrap();
        state
            .remove_exact_legacy_category(&failed_candidate)
            .unwrap();
        state
            .rollback_legacy_category_replacement(
                &previous,
                &failed_candidate,
                exact_legacy_proc(
                    12,
                    112,
                    state.generation,
                    3,
                    "discord",
                    "discord_1.conf",
                    "discord-fingerprint-1",
                ),
            )
            .unwrap();

        let restored = state.snapshot_legacy_category("discord").unwrap();
        assert_eq!(restored.owner.config_file, previous.owner.config_file);
        assert_eq!(
            restored.owner.config_fingerprint,
            previous.owner.config_fingerprint
        );
        assert_eq!(restored.owner.lane_generation, LaneGeneration::new(3));
        assert_eq!(state.procs.get(&20), Some(&neighbor_before));
    }

    #[test]
    fn rollback_rejects_different_previous_fingerprint() {
        let mut state = two_category_legacy_state();
        let previous = state.snapshot_legacy_category("discord").unwrap();
        state.mark_legacy_category_stopping(&previous).unwrap();
        state.remove_exact_legacy_category(&previous).unwrap();
        state
            .commit_legacy_category_replacement(
                &previous,
                exact_legacy_proc(
                    11,
                    111,
                    state.generation,
                    2,
                    "discord",
                    "discord_2.conf",
                    "discord-fingerprint-2",
                ),
            )
            .unwrap();
        let failed_candidate = state.snapshot_legacy_category("discord").unwrap();
        state
            .mark_legacy_category_stopping(&failed_candidate)
            .unwrap();
        state
            .remove_exact_legacy_category(&failed_candidate)
            .unwrap();

        let result = state.rollback_legacy_category_replacement(
            &previous,
            &failed_candidate,
            exact_legacy_proc(
                12,
                112,
                state.generation,
                3,
                "discord",
                "discord_1.conf",
                "changed-fingerprint",
            ),
        );
        assert_eq!(result, Err(LegacyCategoryStateError::InvalidReplacement));
        assert_eq!(state.procs.get(&20).unwrap().category, "youtube");
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
