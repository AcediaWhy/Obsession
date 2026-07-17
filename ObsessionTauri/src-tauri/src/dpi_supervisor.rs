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

pub(crate) fn join_workers_bounded(
    workers: Vec<(&'static str, JoinHandle<()>)>,
    timeout: Duration,
) -> Vec<WorkerStopOutcome> {
    let names = workers.iter().map(|(name, _)| *name).collect::<Vec<_>>();
    let mut states = vec![None; workers.len()];
    let (tx, rx) = mpsc::channel();
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
            states[index] = Some(WorkerStopState::ReaperFailed);
        }
    }
    drop(tx);

    let deadline = Instant::now() + timeout;
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

    names
        .into_iter()
        .enumerate()
        .map(|(index, name)| WorkerStopOutcome {
            name,
            state: states[index].unwrap_or(WorkerStopState::TimedOut),
        })
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ProcessIdentity(u64);

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
    fn kill(&self, pid: u32) -> Result<(), String>;
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

    let kill_error = control.kill(process.pid).err();
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
    let worker_deadline = deadline
        .checked_sub(Duration::from_millis(5))
        .unwrap_or(deadline);
    let mut pending = 0usize;

    for (index, process) in processes.into_iter().enumerate() {
        let tx = tx.clone();
        let control = control.clone();
        let reaper = std::thread::Builder::new()
            .name(format!("dpi-process-reaper-{}", process.pid))
            .spawn(move || {
                let state = stop_process(process, worker_deadline, control.as_ref());
                let _ = tx.send((index, state));
            });
        if reaper.is_ok() {
            pending += 1;
        } else {
            states[index] = Some(ProcessStopState::ReaperFailed);
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

    fn kill(&self, pid: u32) -> Result<(), String> {
        let output = crate::util::std_command("taskkill")
            .args(["/F", "/T", "/PID", &pid.to_string()])
            .output()
            .map_err(|error| format!("taskkill({pid}) failed to start: {error}"))?;
        if output.status.success() {
            Ok(())
        } else {
            Err(format!(
                "taskkill({pid}) exited with {:?}: {}",
                output.status.code(),
                String::from_utf8_lossy(&output.stderr).trim()
            ))
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

        fn kill(&self, _pid: u32) -> Result<(), String> {
            self.killed.store(true, Ordering::SeqCst);
            if self.kill_fails {
                Err("fixture kill failure".into())
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

            fn kill(&self, pid: u32) -> Result<(), String> {
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
                Err("fixture identity failure".into())
            }

            fn kill(&self, _pid: u32) -> Result<(), String> {
                self.killed.store(true, Ordering::SeqCst);
                Ok(())
            }
        }

        let control = Arc::new(UnavailableIdentityControl {
            killed: AtomicBool::new(false),
        });
        let outcomes = stop_processes_bounded(
            vec![OwnedProcess {
                pid: 7,
                identity: None,
            }],
            Duration::from_millis(10),
            control.clone(),
        );

        assert!(control.killed.load(Ordering::SeqCst));
        assert_eq!(outcomes[0].state, ProcessStopState::VerificationFailed);
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
}
