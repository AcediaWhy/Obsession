use std::fmt::Display;
use std::future::Future;
use std::sync::{mpsc, Arc};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use tokio::sync::mpsc::UnboundedReceiver;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ReadinessState {
    Marker,
    BoundedFallback,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ReadinessFailure {
    Exited(Option<i32>),
    WaitFailed(String),
}

pub(crate) fn is_startup_marker(line: &str) -> bool {
    let line = line.to_ascii_lowercase();
    line.contains("windivert initialized") || line.contains("capture is started")
}

pub(crate) async fn wait_for_readiness<F, E>(
    exit: F,
    mut markers: UnboundedReceiver<()>,
    fallback_after: Duration,
) -> Result<ReadinessState, ReadinessFailure>
where
    F: Future<Output = Result<Option<i32>, E>>,
    E: Display,
{
    tokio::pin!(exit);
    let fallback = tokio::time::sleep(fallback_after);
    tokio::pin!(fallback);
    let mut markers_open = true;

    loop {
        tokio::select! {
            status = &mut exit => {
                return status
                    .map(|code| Err(ReadinessFailure::Exited(code)))
                    .unwrap_or_else(|error| Err(ReadinessFailure::WaitFailed(error.to_string())));
            }
            marker = markers.recv(), if markers_open => {
                match marker {
                    Some(()) => return Ok(ReadinessState::Marker),
                    None => markers_open = false,
                }
            }
            _ = &mut fallback => return Ok(ReadinessState::BoundedFallback),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WorkerStopState {
    Joined,
    Panicked,
    TimedOut,
    ReaperFailed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct WorkerStopOutcome {
    pub(crate) name: &'static str,
    pub(crate) state: WorkerStopState,
}

impl WorkerStopOutcome {
    pub(crate) fn is_clean(self) -> bool {
        self.state == WorkerStopState::Joined
    }
}

/// Retains completion evidence for worker joins that outlive one bounded wait.
/// Dropping a timed-out ticket loses the only safe way to distinguish a slow
/// shutdown from a still-live capture worker, so Eyes stores unresolved tickets
/// in `AppState` and polls them before any replacement observer may start.
pub(crate) struct WorkerTeardown {
    names: Vec<&'static str>,
    states: Vec<Option<WorkerStopState>>,
    receiver: mpsc::Receiver<(usize, WorkerStopState)>,
    pending: usize,
}

impl WorkerTeardown {
    pub(crate) fn new(workers: Vec<(&'static str, JoinHandle<()>)>) -> Self {
        let names = workers.iter().map(|(name, _)| *name).collect::<Vec<_>>();
        let mut states = vec![None; workers.len()];
        let (tx, receiver) = mpsc::channel();
        let mut pending = 0usize;

        for (index, (name, worker)) in workers.into_iter().enumerate() {
            let tx = tx.clone();
            let reaper = std::thread::Builder::new()
                .name(format!("dpi-reaper-{name}"))
                .spawn(move || {
                    let state = if worker.join().is_ok() {
                        WorkerStopState::Joined
                    } else {
                        WorkerStopState::Panicked
                    };
                    let _ = tx.send((index, state));
                });
            if reaper.is_ok() {
                pending += 1;
            } else {
                // The original JoinHandle was moved into the failed spawn
                // closure and is now detached. Its exit can never be proven.
                states[index] = Some(WorkerStopState::ReaperFailed);
            }
        }
        drop(tx);

        Self {
            names,
            states,
            receiver,
            pending,
        }
    }

    fn record(&mut self, index: usize, state: WorkerStopState) {
        if self
            .states
            .get_mut(index)
            .is_some_and(|slot| slot.replace(state).is_none())
        {
            self.pending = self.pending.saturating_sub(1);
        }
    }

    fn mark_disconnected(&mut self) {
        for state in &mut self.states {
            if state.is_none() {
                *state = Some(WorkerStopState::ReaperFailed);
            }
        }
        self.pending = 0;
    }

    fn drain_ready(&mut self) {
        while self.pending > 0 {
            match self.receiver.try_recv() {
                Ok((index, state)) => self.record(index, state),
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.mark_disconnected();
                    break;
                }
            }
        }
    }

    pub(crate) fn wait_bounded(&mut self, timeout: Duration) -> Vec<WorkerStopOutcome> {
        self.drain_ready();
        let deadline = Instant::now() + timeout;
        while self.pending > 0 {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            match self.receiver.recv_timeout(remaining) {
                Ok((index, state)) => self.record(index, state),
                Err(mpsc::RecvTimeoutError::Timeout) => break,
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    self.mark_disconnected();
                    break;
                }
            }
        }
        self.outcomes()
    }

    pub(crate) fn outcomes(&self) -> Vec<WorkerStopOutcome> {
        self.names
            .iter()
            .enumerate()
            .map(|(index, name)| WorkerStopOutcome {
                name,
                state: self.states[index].unwrap_or(WorkerStopState::TimedOut),
            })
            .collect()
    }

    /// `Panicked` is terminal and therefore safe from a duplicate-capture
    /// perspective. `ReaperFailed` is permanently unresolved because the
    /// detached original worker can no longer provide completion evidence.
    pub(crate) fn is_resolved(&self) -> bool {
        self.pending == 0
            && self
                .states
                .iter()
                .all(|state| !matches!(state, Some(WorkerStopState::ReaperFailed) | None))
    }
}

#[cfg(test)]
pub(crate) fn join_workers_bounded(
    workers: Vec<(&'static str, JoinHandle<()>)>,
    timeout: Duration,
) -> Vec<WorkerStopOutcome> {
    WorkerTeardown::new(workers).wait_bounded(timeout)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ProcessIdentity(u64);

impl ProcessIdentity {
    pub(crate) const fn get(self) -> u64 {
        self.0
    }

    #[cfg(test)]
    pub(crate) const fn from_raw(value: u64) -> Self {
        Self(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct OwnedProcess {
    pub(crate) pid: u32,
    pub(crate) identity: Option<ProcessIdentity>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ProcessStopState {
    Exited,
    AlreadyExited,
    IdentityChanged,
    IdentityCheckFailed,
    KillFailed,
    VerificationFailed,
    TimedOut,
    ReaperFailed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ProcessStopOutcome {
    pub(crate) pid: u32,
    pub(crate) state: ProcessStopState,
}

impl ProcessStopOutcome {
    pub(crate) fn original_exited(self) -> bool {
        matches!(
            self.state,
            ProcessStopState::Exited
                | ProcessStopState::AlreadyExited
                | ProcessStopState::IdentityChanged
        )
    }
}

pub(crate) trait ProcessControl: Send + Sync + 'static {
    fn identity(&self, pid: u32) -> Result<Option<ProcessIdentity>, String>;
    /// Stops `pid` without outliving `deadline`.
    ///
    /// Implementations own any helper process they spawn and must reap it before
    /// returning. This contract lets `stop_processes_bounded` join every worker
    /// instead of leaving a blocking `taskkill.output()` thread detached.
    fn kill(&self, pid: u32, deadline: Instant) -> Result<(), ProcessKillError>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ProcessKillError {
    Failed(String),
    TimedOut,
}

fn stop_process<C: ProcessControl>(
    process: OwnedProcess,
    deadline: Instant,
    control: &C,
) -> ProcessStopState {
    match control.identity(process.pid) {
        Ok(Some(current)) if process.identity.is_some_and(|expected| expected != current) => {
            return ProcessStopState::IdentityChanged;
        }
        Ok(Some(_)) => {}
        Ok(None) => return ProcessStopState::AlreadyExited,
        Err(_) if process.identity.is_some() => return ProcessStopState::IdentityCheckFailed,
        // Без captured identity PID reuse проверить нельзя, но пропуск kill
        // гарантированно оставил бы потенциальный orphan.
        Err(_) => {}
    }

    if Instant::now() >= deadline {
        return ProcessStopState::TimedOut;
    }
    let kill_error = match control.kill(process.pid, deadline) {
        Ok(()) => None,
        Err(ProcessKillError::TimedOut) => return ProcessStopState::TimedOut,
        Err(ProcessKillError::Failed(error)) => Some(error),
    };
    let mut verification_failed = false;
    loop {
        match control.identity(process.pid) {
            Ok(None) => return ProcessStopState::Exited,
            Ok(Some(identity))
                if Some(identity) != process.identity && process.identity.is_some() =>
            {
                return ProcessStopState::IdentityChanged;
            }
            Ok(Some(_)) => {}
            Err(_) => verification_failed = true,
        }

        if kill_error.is_some() {
            return ProcessStopState::KillFailed;
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return if verification_failed {
                ProcessStopState::VerificationFailed
            } else {
                ProcessStopState::TimedOut
            };
        }
        std::thread::sleep(remaining.min(Duration::from_millis(25)));
    }
}

pub(crate) fn stop_processes_bounded<C: ProcessControl>(
    processes: Vec<OwnedProcess>,
    timeout: Duration,
    control: Arc<C>,
) -> Vec<ProcessStopOutcome> {
    let pids = processes
        .iter()
        .map(|process| process.pid)
        .collect::<Vec<_>>();
    let mut states = vec![None; processes.len()];
    let (tx, rx) = mpsc::channel();
    let deadline = Instant::now() + timeout;
    // Keep a small outer margin for reaping and joining a bounded taskkill
    // helper after its own deadline expires.
    let budget = deadline.saturating_duration_since(Instant::now());
    let reap_margin = (budget / 4).min(Duration::from_millis(50));
    let worker_deadline = deadline.checked_sub(reap_margin).unwrap_or(deadline);
    let mut pending = 0usize;
    let mut workers = Vec::with_capacity(processes.len());

    for (index, process) in processes.into_iter().enumerate() {
        let tx = tx.clone();
        let control = control.clone();
        let reaper = std::thread::Builder::new()
            .name(format!("dpi-process-reaper-{}", process.pid))
            .spawn(move || {
                let state = stop_process(process, worker_deadline, control.as_ref());
                let _ = tx.send((index, state));
                state
            });
        match reaper {
            Ok(worker) => {
                pending += 1;
                workers.push((index, worker));
            }
            Err(_) => states[index] = Some(ProcessStopState::ReaperFailed),
        }
    }
    drop(tx);

    while pending > 0 {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        match rx.recv_timeout(remaining) {
            Ok((index, state)) => {
                if states[index].replace(state).is_none() {
                    pending -= 1;
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout | mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    // `ProcessControl::kill` is deadline-bounded, so every worker is now
    // joinable. Joining is intentional: returning with a live worker would
    // detach its taskkill child and make the advertised stop bound untrue.
    for (index, worker) in workers {
        match worker.join() {
            Ok(state) => {
                states[index].get_or_insert(state);
            }
            Err(_) => {
                states[index].get_or_insert(ProcessStopState::ReaperFailed);
            }
        }
    }

    pids.into_iter()
        .enumerate()
        .map(|(index, pid)| ProcessStopOutcome {
            pid,
            state: states[index].unwrap_or(ProcessStopState::TimedOut),
        })
        .collect()
}

#[cfg(windows)]
pub(crate) struct WindowsProcessControl;

#[cfg(windows)]
fn windows_process_identity(pid: u32) -> Result<Option<ProcessIdentity>, String> {
    use windows::Win32::Foundation::{CloseHandle, FILETIME};
    use windows::Win32::System::Threading::{
        GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    let handle = match unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) } {
        Ok(handle) => handle,
        // HRESULT_FROM_WIN32(ERROR_INVALID_PARAMETER): PID больше не существует.
        Err(error) if error.code().0 as u32 == 0x8007_0057 => return Ok(None),
        Err(error) => return Err(format!("OpenProcess({pid}) failed: {error}")),
    };
    let mut creation = FILETIME::default();
    let mut exit = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    let result =
        unsafe { GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user) };
    let _ = unsafe { CloseHandle(handle) };
    result.map_err(|error| format!("GetProcessTimes({pid}) failed: {error}"))?;
    Ok(Some(ProcessIdentity(
        ((creation.dwHighDateTime as u64) << 32) | creation.dwLowDateTime as u64,
    )))
}

#[cfg(windows)]
impl ProcessControl for WindowsProcessControl {
    fn identity(&self, pid: u32) -> Result<Option<ProcessIdentity>, String> {
        windows_process_identity(pid)
    }

    fn kill(&self, pid: u32, deadline: Instant) -> Result<(), ProcessKillError> {
        use std::process::Stdio;

        let mut child = crate::util::std_command("taskkill")
            .args(["/F", "/T", "/PID", &pid.to_string()])
            // taskkill emits only diagnostics here. Avoid pipe back-pressure:
            // the exit status and subsequent identity check are authoritative.
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| {
                ProcessKillError::Failed(format!("taskkill({pid}) failed to start: {error}"))
            })?;

        loop {
            match child.try_wait() {
                Ok(Some(status)) if status.success() => return Ok(()),
                Ok(Some(status)) => {
                    return Err(ProcessKillError::Failed(format!(
                        "taskkill({pid}) exited with {:?}",
                        status.code()
                    )));
                }
                Ok(None) => {}
                Err(error) => {
                    // Best effort termination followed by wait prevents a
                    // started helper from being detached on the error path.
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(ProcessKillError::Failed(format!(
                        "taskkill({pid}) wait failed: {error}"
                    )));
                }
            }

            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                let kill_result = child.kill();
                let wait_result = child.wait();
                if let Err(error) = wait_result {
                    return Err(ProcessKillError::Failed(format!(
                        "taskkill({pid}) timed out and could not be reaped: {error}"
                    )));
                }
                if let Err(error) = kill_result {
                    // A race where taskkill exited between try_wait and kill is
                    // harmless once wait reaped it; the stop itself still used
                    // the full budget and is reported as timed out.
                    let _ = error;
                }
                return Err(ProcessKillError::TimedOut);
            }
            std::thread::sleep(remaining.min(Duration::from_millis(10)));
        }
    }
}

#[cfg(windows)]
pub(crate) fn capture_process_identity(pid: u32) -> Option<ProcessIdentity> {
    windows_process_identity(pid).ok().flatten()
}

#[cfg(not(windows))]
pub(crate) fn capture_process_identity(_pid: u32) -> Option<ProcessIdentity> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[tokio::test]
    async fn startup_marker_finishes_readiness_early() {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        tx.send(()).unwrap();
        let exit = std::future::pending::<Result<Option<i32>, io::Error>>();

        let result = wait_for_readiness(exit, rx, Duration::from_secs(1)).await;

        assert_eq!(result, Ok(ReadinessState::Marker));
    }

    #[tokio::test]
    async fn missing_marker_uses_bounded_fallback() {
        let (_tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let exit = std::future::pending::<Result<Option<i32>, io::Error>>();
        let started = Instant::now();

        let result = wait_for_readiness(exit, rx, Duration::from_millis(10)).await;

        assert_eq!(result, Ok(ReadinessState::BoundedFallback));
        assert!(started.elapsed() < Duration::from_millis(100));
    }

    #[tokio::test]
    async fn early_exit_fails_readiness() {
        let (_tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let exit = std::future::ready(Ok::<_, io::Error>(Some(17)));

        let result = wait_for_readiness(exit, rx, Duration::from_secs(1)).await;

        assert_eq!(result, Err(ReadinessFailure::Exited(Some(17))));
    }

    #[test]
    fn startup_marker_is_specific() {
        assert!(is_startup_marker(
            "WinDivert initialized. capture is started."
        ));
        assert!(!is_startup_marker("loading lua profiles"));
    }

    #[test]
    fn worker_join_is_parallel_and_bounded() {
        let fast = std::thread::spawn(|| {});
        let slow = std::thread::spawn(|| std::thread::sleep(Duration::from_millis(100)));
        let started = Instant::now();

        let outcomes = join_workers_bounded(
            vec![("fast", fast), ("slow", slow)],
            Duration::from_millis(20),
        );

        assert!(started.elapsed() < Duration::from_millis(80));
        assert_eq!(
            outcomes,
            vec![
                WorkerStopOutcome {
                    name: "fast",
                    state: WorkerStopState::Joined,
                },
                WorkerStopOutcome {
                    name: "slow",
                    state: WorkerStopState::TimedOut,
                },
            ]
        );
    }

    #[test]
    fn timed_out_worker_teardown_can_be_confirmed_on_a_later_poll() {
        let (release, blocked) = mpsc::channel();
        let slow = std::thread::spawn(move || {
            let _ = blocked.recv();
        });
        let mut teardown = WorkerTeardown::new(vec![("slow", slow)]);

        assert_eq!(
            teardown.wait_bounded(Duration::from_millis(1)),
            vec![WorkerStopOutcome {
                name: "slow",
                state: WorkerStopState::TimedOut,
            }]
        );
        assert!(!teardown.is_resolved());

        release.send(()).unwrap();
        assert_eq!(
            teardown.wait_bounded(Duration::from_secs(1)),
            vec![WorkerStopOutcome {
                name: "slow",
                state: WorkerStopState::Joined,
            }]
        );
        assert!(teardown.is_resolved());
    }

    #[test]
    fn worker_panic_is_typed() {
        let worker = std::thread::spawn(|| panic!("fixture"));
        let outcomes = join_workers_bounded(vec![("panic", worker)], Duration::from_millis(100));

        assert_eq!(outcomes[0].state, WorkerStopState::Panicked);
    }

    struct FakeControl {
        current: Option<ProcessIdentity>,
        after_kill: Option<ProcessIdentity>,
        kill_fails: bool,
        killed: AtomicBool,
    }

    impl FakeControl {
        fn new(current: Option<u64>, after_kill: Option<u64>, kill_fails: bool) -> Self {
            Self {
                current: current.map(ProcessIdentity),
                after_kill: after_kill.map(ProcessIdentity),
                kill_fails,
                killed: AtomicBool::new(false),
            }
        }
    }

    impl ProcessControl for FakeControl {
        fn identity(&self, _pid: u32) -> Result<Option<ProcessIdentity>, String> {
            if self.killed.load(Ordering::SeqCst) {
                Ok(self.after_kill)
            } else {
                Ok(self.current)
            }
        }

        fn kill(&self, _pid: u32, _deadline: Instant) -> Result<(), ProcessKillError> {
            self.killed.store(true, Ordering::SeqCst);
            if self.kill_fails {
                Err(ProcessKillError::Failed("fixture kill failure".into()))
            } else {
                Ok(())
            }
        }
    }

    #[test]
    fn owned_process_exit_is_verified() {
        let outcomes = stop_processes_bounded(
            vec![OwnedProcess {
                pid: 7,
                identity: Some(ProcessIdentity(11)),
            }],
            Duration::from_millis(100),
            Arc::new(FakeControl::new(Some(11), None, false)),
        );

        assert_eq!(outcomes[0].state, ProcessStopState::Exited);
    }

    #[test]
    fn owned_processes_are_stopped_in_parallel() {
        use std::collections::HashSet;
        use std::sync::{Barrier, Mutex};

        struct ParallelControl {
            barrier: Barrier,
            killed: Mutex<HashSet<u32>>,
        }

        impl ProcessControl for ParallelControl {
            fn identity(&self, pid: u32) -> Result<Option<ProcessIdentity>, String> {
                if self.killed.lock().unwrap().contains(&pid) {
                    Ok(None)
                } else {
                    Ok(Some(ProcessIdentity(pid as u64)))
                }
            }

            fn kill(&self, pid: u32, _deadline: Instant) -> Result<(), ProcessKillError> {
                self.barrier.wait();
                self.killed.lock().unwrap().insert(pid);
                Ok(())
            }
        }

        let outcomes = stop_processes_bounded(
            vec![
                OwnedProcess {
                    pid: 7,
                    identity: Some(ProcessIdentity(7)),
                },
                OwnedProcess {
                    pid: 8,
                    identity: Some(ProcessIdentity(8)),
                },
            ],
            Duration::from_millis(100),
            Arc::new(ParallelControl {
                barrier: Barrier::new(2),
                killed: Mutex::new(HashSet::new()),
            }),
        );

        assert!(outcomes.iter().all(|outcome| outcome.original_exited()));
    }

    #[test]
    fn pid_reuse_is_not_killed() {
        let control = Arc::new(FakeControl::new(Some(12), None, false));
        let outcomes = stop_processes_bounded(
            vec![OwnedProcess {
                pid: 7,
                identity: Some(ProcessIdentity(11)),
            }],
            Duration::from_millis(100),
            control.clone(),
        );

        assert_eq!(outcomes[0].state, ProcessStopState::IdentityChanged);
        assert!(!control.killed.load(Ordering::SeqCst));
    }

    #[test]
    fn kill_failure_is_typed() {
        let outcomes = stop_processes_bounded(
            vec![OwnedProcess {
                pid: 7,
                identity: Some(ProcessIdentity(11)),
            }],
            Duration::from_millis(100),
            Arc::new(FakeControl::new(Some(11), Some(11), true)),
        );

        assert_eq!(outcomes[0].state, ProcessStopState::KillFailed);
    }

    #[test]
    fn missing_captured_identity_still_attempts_kill() {
        struct UnavailableIdentityControl {
            killed: AtomicBool,
        }

        impl ProcessControl for UnavailableIdentityControl {
            fn identity(&self, _pid: u32) -> Result<Option<ProcessIdentity>, String> {
                if self.killed.load(Ordering::SeqCst) {
                    Ok(None)
                } else {
                    Err("fixture identity failure".into())
                }
            }

            fn kill(&self, _pid: u32, _deadline: Instant) -> Result<(), ProcessKillError> {
                self.killed.store(true, Ordering::SeqCst);
                Ok(())
            }
        }

        let control = Arc::new(UnavailableIdentityControl {
            killed: AtomicBool::new(false),
        });
        let outcome = stop_process(
            OwnedProcess {
                pid: 7,
                identity: None,
            },
            Instant::now() + Duration::from_secs(1),
            control.as_ref(),
        );

        assert!(control.killed.load(Ordering::SeqCst));
        assert_eq!(outcome, ProcessStopState::Exited);
    }

    #[test]
    fn process_stop_has_hard_deadline() {
        let started = Instant::now();
        let outcomes = stop_processes_bounded(
            vec![OwnedProcess {
                pid: 7,
                identity: Some(ProcessIdentity(11)),
            }],
            Duration::from_millis(20),
            Arc::new(FakeControl::new(Some(11), Some(11), false)),
        );

        assert!(started.elapsed() < Duration::from_millis(100));
        assert_eq!(outcomes[0].state, ProcessStopState::TimedOut);
    }

    #[test]
    fn blocking_kill_is_timed_out_and_joined_before_return() {
        struct DeadlineBlockingControl {
            kill_returned: AtomicBool,
        }

        impl ProcessControl for DeadlineBlockingControl {
            fn identity(&self, _pid: u32) -> Result<Option<ProcessIdentity>, String> {
                Ok(Some(ProcessIdentity(11)))
            }

            fn kill(&self, _pid: u32, deadline: Instant) -> Result<(), ProcessKillError> {
                while Instant::now() < deadline {
                    std::thread::sleep(Duration::from_millis(1));
                }
                self.kill_returned.store(true, Ordering::SeqCst);
                Err(ProcessKillError::TimedOut)
            }
        }

        let control = Arc::new(DeadlineBlockingControl {
            kill_returned: AtomicBool::new(false),
        });
        let started = Instant::now();
        let outcomes = stop_processes_bounded(
            vec![OwnedProcess {
                pid: 7,
                identity: Some(ProcessIdentity(11)),
            }],
            Duration::from_millis(40),
            control.clone(),
        );

        assert!(started.elapsed() < Duration::from_millis(150));
        assert_eq!(outcomes[0].state, ProcessStopState::TimedOut);
        assert!(
            control.kill_returned.load(Ordering::SeqCst),
            "stop returned while its kill worker was still detached"
        );
    }
}
