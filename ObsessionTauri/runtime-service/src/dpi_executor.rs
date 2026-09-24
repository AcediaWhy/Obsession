//! Exact Windows ownership for protected DPI processes.
//!
//! The executor launches a fixed verified image suspended, assigns it to a
//! kill-on-close Job Object, captures its creation identity, and only then
//! resumes it. Readiness and teardown are bounded. No global image-name kill or
//! caller-provided argv exists in this boundary.

use std::ffi::OsStr;
use std::mem::size_of;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use obsession_runtime_protocol::{DpiEngine, DpiRuntimeSnapshot, DpiSelection, RuntimeStarted};
use windows::core::{Error as WindowsError, PCWSTR, PWSTR};
use windows::Win32::Foundation::{
    CloseHandle, FILETIME, HANDLE, WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    SetInformationJobObject, TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_ACTIVE_PROCESS, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows::Win32::System::Threading::{
    CreateProcessW, GetExitCodeProcess, GetProcessTimes, ResumeThread, TerminateProcess,
    WaitForSingleObject, CREATE_NO_WINDOW, CREATE_SUSPENDED, PROCESS_INFORMATION, STARTUPINFOW,
};

use crate::dpi_materializer::{
    MaterializationError, MaterializedGeneration, MaterializedLaunch, ProtectedDataLayout,
};
use crate::protected_layout::VerifiedDpiPlan;

const READINESS_TIMEOUT: Duration = Duration::from_millis(750);
const STOP_TIMEOUT: Duration = Duration::from_secs(3);
const WAIT_POLL_INTERVAL: Duration = Duration::from_millis(10);
const RUNTIME_TERMINATION_EXIT_CODE: u32 = 0xE000_0001;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcessIdentity {
    pub pid: u32,
    pub creation_time_100ns: u64,
    pub executable_sha256: String,
}

pub trait RuntimeProcessGroup: Send {
    fn identities(&self) -> &[ProcessIdentity];
    fn stop(&mut self, timeout: Duration) -> Result<(), ProcessError>;
}

pub trait RuntimeProcessLauncher: Send {
    type Group: RuntimeProcessGroup;

    fn launch_group(
        &self,
        launches: &[MaterializedLaunch],
        readiness_timeout: Duration,
    ) -> Result<Self::Group, ProcessError>;
}

pub struct DpiExecutor<L: RuntimeProcessLauncher> {
    state_layout: ProtectedDataLayout,
    launcher: L,
    next_generation: AtomicU64,
    active: Option<ActiveRuntime<L::Group>>,
}

impl<L: RuntimeProcessLauncher> DpiExecutor<L> {
    pub fn new(state_layout: ProtectedDataLayout, launcher: L) -> Self {
        Self {
            state_layout,
            launcher,
            next_generation: AtomicU64::new(1),
            active: None,
        }
    }

    pub fn start(&mut self, plan: &VerifiedDpiPlan) -> Result<RuntimeStarted, ExecutorError> {
        if self.active.is_some() {
            return Err(ExecutorError::Busy);
        }
        let generation = self.allocate_generation();
        let materialized = self.state_layout.materialize(plan, generation)?;

        // This is deliberately repeated after materialization and immediately
        // before CreateProcessW. A catalog loaded at service startup is not an
        // integrity lease for later launches.
        if let Err(error) = plan.reverify() {
            let _ = self.state_layout.cleanup_generation(&materialized);
            return Err(ExecutorError::Materialization(error.into()));
        }
        if let Err(error) = materialized.reverify() {
            let _ = self.state_layout.cleanup_generation(&materialized);
            return Err(ExecutorError::Materialization(error));
        }

        let mut group = match self
            .launcher
            .launch_group(materialized.launches(), READINESS_TIMEOUT)
        {
            Ok(group) => group,
            Err(error) => {
                let _ = self.state_layout.cleanup_generation(&materialized);
                return Err(ExecutorError::Process(error));
            }
        };
        if !identities_match(group.identities(), materialized.launches()) {
            let _ = group.stop(STOP_TIMEOUT);
            let _ = self.state_layout.cleanup_generation(&materialized);
            return Err(ExecutorError::InvalidOwnership);
        }

        let selections = plan
            .strategies()
            .iter()
            .map(|strategy| DpiSelection {
                category: strategy.category(),
                strategy_id: strategy.strategy_id().to_owned(),
            })
            .collect();
        self.active = Some(ActiveRuntime {
            generation,
            engine: plan.engine(),
            selections,
            fingerprint: materialized.fingerprint().to_owned(),
            materialized,
            group,
        });
        Ok(RuntimeStarted { generation })
    }

    pub fn stop(&mut self, generation: u64) -> Result<(), ExecutorError> {
        let Some(active) = self.active.as_ref() else {
            return Err(ExecutorError::Conflict);
        };
        if generation == 0 || active.generation != generation {
            return Err(ExecutorError::Conflict);
        }
        let mut active = self.active.take().expect("checked active runtime");
        let stop_result = active
            .group
            .stop(STOP_TIMEOUT)
            .map_err(ExecutorError::Process);
        let cleanup_result = self
            .state_layout
            .cleanup_generation(&active.materialized)
            .map_err(ExecutorError::Materialization);
        stop_result.and(cleanup_result)
    }

    pub fn snapshot(&self) -> Option<DpiRuntimeSnapshot> {
        self.active.as_ref().map(|active| DpiRuntimeSnapshot {
            generation: active.generation,
            engine: active.engine,
            selections: active.selections.clone(),
        })
    }

    pub fn active_fingerprint(&self) -> Option<&str> {
        self.active
            .as_ref()
            .map(|active| active.fingerprint.as_str())
    }

    pub fn active_processes(&self) -> &[ProcessIdentity] {
        self.active
            .as_ref()
            .map(|active| active.group.identities())
            .unwrap_or(&[])
    }

    fn allocate_generation(&self) -> u64 {
        loop {
            let generation = self.next_generation.fetch_add(1, Ordering::Relaxed);
            if generation != 0 {
                return generation;
            }
        }
    }
}

impl<L: RuntimeProcessLauncher> Drop for DpiExecutor<L> {
    fn drop(&mut self) {
        if let Some(mut active) = self.active.take() {
            let _ = active.group.stop(STOP_TIMEOUT);
            let _ = self.state_layout.cleanup_generation(&active.materialized);
        }
    }
}

struct ActiveRuntime<G> {
    generation: u64,
    engine: DpiEngine,
    selections: Vec<DpiSelection>,
    fingerprint: String,
    materialized: MaterializedGeneration,
    group: G,
}

fn identities_match(identities: &[ProcessIdentity], launches: &[MaterializedLaunch]) -> bool {
    identities.len() == launches.len()
        && identities.iter().zip(launches).all(|(identity, launch)| {
            identity.pid != 0
                && identity.creation_time_100ns != 0
                && identity
                    .executable_sha256
                    .eq_ignore_ascii_case(launch.executable_sha256())
        })
}

#[derive(Debug)]
pub enum ExecutorError {
    Busy,
    Conflict,
    InvalidOwnership,
    Materialization(MaterializationError),
    Process(ProcessError),
}

impl std::fmt::Display for ExecutorError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Busy => formatter.write_str("a machine-wide DPI runtime is already active"),
            Self::Conflict => formatter.write_str("runtime generation does not match the owner"),
            Self::InvalidOwnership => {
                formatter.write_str("launched process group returned incomplete ownership identity")
            }
            Self::Materialization(error) => {
                write!(formatter, "DPI materialization failed: {error}")
            }
            Self::Process(error) => write!(formatter, "DPI process operation failed: {error}"),
        }
    }
}

impl std::error::Error for ExecutorError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Materialization(source) => Some(source),
            Self::Process(source) => Some(source),
            _ => None,
        }
    }
}

impl From<MaterializationError> for ExecutorError {
    fn from(value: MaterializationError) -> Self {
        Self::Materialization(value)
    }
}

#[derive(Debug)]
pub enum ProcessError {
    Windows(WindowsError),
    InvalidLaunchPath,
    NoLaunches,
    ResumeFailed,
    ExitedBeforeReady { pid: u32, exit_code: u32 },
    StopTimedOut,
    UnexpectedWaitResult(u32),
}

impl std::fmt::Display for ProcessError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Windows(error) => write!(formatter, "Windows error: {error}"),
            Self::InvalidLaunchPath => formatter.write_str("launch path contains invalid data"),
            Self::NoLaunches => formatter.write_str("process group contains no launches"),
            Self::ResumeFailed => formatter.write_str("could not resume assigned DPI process"),
            Self::ExitedBeforeReady { pid, exit_code } => {
                write!(
                    formatter,
                    "DPI process {pid} exited before readiness with code {exit_code}"
                )
            }
            Self::StopTimedOut => {
                formatter.write_str("DPI Job Object did not stop before deadline")
            }
            Self::UnexpectedWaitResult(result) => {
                write!(formatter, "unexpected Windows wait result {result}")
            }
        }
    }
}

impl std::error::Error for ProcessError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Windows(source) => Some(source),
            _ => None,
        }
    }
}

impl From<WindowsError> for ProcessError {
    fn from(value: WindowsError) -> Self {
        Self::Windows(value)
    }
}

pub struct WindowsJobLauncher {
    timestamps: crate::tcp_timestamps::TimestampController,
}

impl WindowsJobLauncher {
    pub fn new(layout: ProtectedDataLayout) -> std::io::Result<Self> {
        Ok(Self { timestamps: crate::tcp_timestamps::TimestampController::new(layout)? })
    }
}

fn timestamp_process_error(error: std::io::Error) -> ProcessError {
    ProcessError::Windows(WindowsError::new(
        windows::Win32::Foundation::E_FAIL,
        format!("TCP timestamps: {error}"),
    ))
}

impl RuntimeProcessLauncher for WindowsJobLauncher {
    type Group = WindowsJobGroup;

    fn launch_group(
        &self,
        launches: &[MaterializedLaunch],
        readiness_timeout: Duration,
    ) -> Result<Self::Group, ProcessError> {
        if launches.is_empty() {
            return Err(ProcessError::NoLaunches);
        }
        // Declared before the job: on launch failure the kill-on-close job is
        // destroyed before the lease restores the global setting.
        let timestamps = self.timestamps.acquire(launches).map_err(timestamp_process_error)?;
        let job = create_kill_on_close_job(launches.len() as u32)?;
        let mut processes = Vec::with_capacity(launches.len());
        for launch in launches {
            processes.push(spawn_assigned_suspended(&job, launch)?);
        }
        wait_for_bounded_readiness(&processes, readiness_timeout)?;
        let identities = processes
            .iter()
            .map(|process| process.identity.clone())
            .collect();
        Ok(WindowsJobGroup {
            job,
            processes,
            identities,
            terminated: false,
            timestamps,
        })
    }
}

pub struct WindowsJobGroup {
    job: OwnedHandle,
    processes: Vec<OwnedProcess>,
    identities: Vec<ProcessIdentity>,
    terminated: bool,
    timestamps: crate::tcp_timestamps::TimestampLease,
}

impl RuntimeProcessGroup for WindowsJobGroup {
    fn identities(&self) -> &[ProcessIdentity] {
        &self.identities
    }

    fn stop(&mut self, timeout: Duration) -> Result<(), ProcessError> {
        if !self.terminated {
            unsafe { TerminateJobObject(self.job.raw(), RUNTIME_TERMINATION_EXIT_CODE)? };
            self.terminated = true;
        }
        wait_for_all_stopped(&self.processes, timeout)?;
        self.timestamps.release().map_err(timestamp_process_error)
    }
}

impl Drop for WindowsJobGroup {
    fn drop(&mut self) {
        if !self.terminated {
            let _ = unsafe { TerminateJobObject(self.job.raw(), RUNTIME_TERMINATION_EXIT_CODE) };
            self.terminated = true;
        }
        let _ = wait_for_all_stopped(&self.processes, STOP_TIMEOUT);
        // Fields drop in declaration order, closing the job before the lease.
    }
}

struct OwnedProcess {
    handle: OwnedHandle,
    identity: ProcessIdentity,
}

struct OwnedHandle(HANDLE);

unsafe impl Send for OwnedHandle {}

impl OwnedHandle {
    fn new(handle: HANDLE) -> Result<Self, ProcessError> {
        if handle.is_invalid() {
            Err(ProcessError::Windows(WindowsError::from_win32()))
        } else {
            Ok(Self(handle))
        }
    }

    fn raw(&self) -> HANDLE {
        self.0
    }
}

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            let _ = unsafe { CloseHandle(self.0) };
        }
    }
}

fn create_kill_on_close_job(max_processes: u32) -> Result<OwnedHandle, ProcessError> {
    if max_processes == 0 {
        return Err(ProcessError::NoLaunches);
    }
    let handle = unsafe { CreateJobObjectW(None, PCWSTR::null())? };
    let handle = OwnedHandle::new(handle)?;
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.ActiveProcessLimit = max_processes;
    limits.BasicLimitInformation.LimitFlags =
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_ACTIVE_PROCESS;
    unsafe {
        SetInformationJobObject(
            handle.raw(),
            JobObjectExtendedLimitInformation,
            (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )?;
    }
    Ok(handle)
}

fn spawn_assigned_suspended(
    job: &OwnedHandle,
    launch: &MaterializedLaunch,
) -> Result<OwnedProcess, ProcessError> {
    let executable = wide_path(launch.executable())?;
    let current_directory = wide_path(launch.working_directory())?;
    let command_line = build_command_line(launch)?;
    let mut command_line = wide_string(&command_line)?;
    let startup = STARTUPINFOW {
        cb: size_of::<STARTUPINFOW>() as u32,
        ..Default::default()
    };
    let mut information = PROCESS_INFORMATION::default();
    unsafe {
        CreateProcessW(
            PCWSTR(executable.as_ptr()),
            PWSTR(command_line.as_mut_ptr()),
            None,
            None,
            false,
            CREATE_SUSPENDED | CREATE_NO_WINDOW,
            None,
            PCWSTR(current_directory.as_ptr()),
            &startup,
            &mut information,
        )?;
    }
    let process = OwnedHandle::new(information.hProcess)?;
    let thread_handle = match OwnedHandle::new(information.hThread) {
        Ok(handle) => handle,
        Err(error) => {
            let _ = unsafe { TerminateProcess(process.raw(), RUNTIME_TERMINATION_EXIT_CODE) };
            return Err(error);
        }
    };

    let creation_time_100ns = match process_creation_time(process.raw()) {
        Ok(value) => value,
        Err(error) => {
            let _ = unsafe { TerminateProcess(process.raw(), RUNTIME_TERMINATION_EXIT_CODE) };
            return Err(error);
        }
    };
    if let Err(error) = unsafe { AssignProcessToJobObject(job.raw(), process.raw()) } {
        let _ = unsafe { TerminateProcess(process.raw(), RUNTIME_TERMINATION_EXIT_CODE) };
        let _ = unsafe { WaitForSingleObject(process.raw(), 250) };
        return Err(error.into());
    }
    let previous_suspend_count = unsafe { ResumeThread(thread_handle.raw()) };
    if previous_suspend_count == u32::MAX {
        let _ = unsafe { TerminateProcess(process.raw(), RUNTIME_TERMINATION_EXIT_CODE) };
        let _ = unsafe { WaitForSingleObject(process.raw(), 250) };
        return Err(ProcessError::ResumeFailed);
    }
    drop(thread_handle);
    Ok(OwnedProcess {
        handle: process,
        identity: ProcessIdentity {
            pid: information.dwProcessId,
            creation_time_100ns,
            executable_sha256: launch.executable_sha256().to_owned(),
        },
    })
}

fn process_creation_time(handle: HANDLE) -> Result<u64, ProcessError> {
    let mut creation = FILETIME::default();
    let mut exit = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    unsafe { GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user)? };
    let value = (u64::from(creation.dwHighDateTime) << 32) | u64::from(creation.dwLowDateTime);
    if value == 0 {
        Err(ProcessError::InvalidLaunchPath)
    } else {
        Ok(value)
    }
}

fn wait_for_bounded_readiness(
    processes: &[OwnedProcess],
    timeout: Duration,
) -> Result<(), ProcessError> {
    if timeout.is_zero() {
        return Err(ProcessError::StopTimedOut);
    }
    let deadline = Instant::now() + timeout;
    loop {
        for process in processes {
            match unsafe { WaitForSingleObject(process.handle.raw(), 0) } {
                WAIT_OBJECT_0 => {
                    let mut exit_code = 0;
                    unsafe { GetExitCodeProcess(process.handle.raw(), &mut exit_code)? };
                    return Err(ProcessError::ExitedBeforeReady {
                        pid: process.identity.pid,
                        exit_code,
                    });
                }
                WAIT_TIMEOUT => {}
                WAIT_FAILED => return Err(ProcessError::Windows(WindowsError::from_win32())),
                result => return Err(ProcessError::UnexpectedWaitResult(result.0)),
            }
        }
        if Instant::now() >= deadline {
            return Ok(());
        }
        thread::sleep(WAIT_POLL_INTERVAL.min(deadline.saturating_duration_since(Instant::now())));
    }
}

fn wait_for_all_stopped(processes: &[OwnedProcess], timeout: Duration) -> Result<(), ProcessError> {
    let deadline = Instant::now() + timeout;
    for process in processes {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let wait_ms = remaining.as_millis().min(u128::from(u32::MAX)) as u32;
        match unsafe { WaitForSingleObject(process.handle.raw(), wait_ms) } {
            WAIT_OBJECT_0 => {}
            WAIT_TIMEOUT => return Err(ProcessError::StopTimedOut),
            WAIT_FAILED => return Err(ProcessError::Windows(WindowsError::from_win32())),
            result => return Err(ProcessError::UnexpectedWaitResult(result.0)),
        }
    }
    Ok(())
}

fn wide_path(path: &Path) -> Result<Vec<u16>, ProcessError> {
    if !path.is_absolute() {
        return Err(ProcessError::InvalidLaunchPath);
    }
    wide_os_string(path.as_os_str())
}

fn wide_string(value: &str) -> Result<Vec<u16>, ProcessError> {
    wide_os_string(OsStr::new(value))
}

fn wide_os_string(value: &OsStr) -> Result<Vec<u16>, ProcessError> {
    let mut wide = value.encode_wide().collect::<Vec<_>>();
    if wide.is_empty() || wide.contains(&0) {
        return Err(ProcessError::InvalidLaunchPath);
    }
    wide.push(0);
    Ok(wide)
}

fn quote_windows_argument(value: &str) -> String {
    if !value.is_empty()
        && !value
            .chars()
            .any(|character| character.is_whitespace() || character == '"')
    {
        return value.to_owned();
    }
    let mut output = String::from("\"");
    let mut backslashes = 0;
    for character in value.chars() {
        match character {
            '\\' => backslashes += 1,
            '"' => {
                output.extend(std::iter::repeat_n('\\', backslashes * 2 + 1));
                output.push('"');
                backslashes = 0;
            }
            other => {
                output.extend(std::iter::repeat_n('\\', backslashes));
                output.push(other);
                backslashes = 0;
            }
        }
    }
    output.extend(std::iter::repeat_n('\\', backslashes * 2));
    output.push('"');
    output
}

fn build_command_line(launch: &MaterializedLaunch) -> Result<String, ProcessError> {
    let mut values = Vec::with_capacity(launch.arguments().len() + 1);
    values.push(quote_windows_argument(
        &launch.executable().to_string_lossy(),
    ));
    values.extend(
        launch
            .arguments()
            .iter()
            .map(|argument| quote_windows_argument(argument)),
    );
    let command_line = values.join(" ");
    // CreateProcessW's mutable command line is limited to 32,767 UTF-16 code
    // units including the trailing NUL.
    if command_line.encode_utf16().count() >= 32_767 {
        return Err(ProcessError::InvalidLaunchPath);
    }
    Ok(command_line)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dpi_materializer::RUNTIME_STATE_RELATIVE;
    use crate::protected_layout::{ProtectedLayout, RESOURCE_MANIFEST};
    use obsession_runtime_protocol::{
        DpiCategory, DpiRuntimeOptions, DpiSelection, DpiStartRequest,
    };
    use sha2::{Digest, Sha256};
    use std::fs;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TestRoot(PathBuf);

    impl TestRoot {
        fn new() -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir().join(format!("obsession-executor-test-{nonce}"));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn sha(bytes: &[u8]) -> String {
        Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    fn setup_plan() -> (TestRoot, VerifiedDpiPlan, ProtectedDataLayout) {
        let test = TestRoot::new();
        let program_files = test.0.join("Program Files");
        let install_root = program_files.join("Obsession");
        let program_data = test.0.join("ProgramData");
        let state_root = program_data.join(RUNTIME_STATE_RELATIVE);
        fs::create_dir_all(&state_root).unwrap();

        let executable_relative = "runtime/legacy/winws.exe";
        let config_relative = "runtime/configs/discord.conf";
        let executable = b"fixed-engine";
        let config = b"--wf-tcp=80,443\n--new\n";
        for (relative, bytes) in [
            (executable_relative, executable.as_slice()),
            (config_relative, config.as_slice()),
        ] {
            let path = install_root.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, bytes).unwrap();
        }
        let manifest = install_root.join(RESOURCE_MANIFEST);
        fs::create_dir_all(manifest.parent().unwrap()).unwrap();
        fs::write(
            manifest,
            serde_json::to_vec(&serde_json::json!({
                "schema_version": 1,
                "engines": [{
                    "engine": "legacy",
                    "executable": executable_relative,
                    "files": [
                        {"path": executable_relative, "size": executable.len(), "sha256": sha(executable)},
                        {"path": config_relative, "size": config.len(), "sha256": sha(config)}
                    ],
                    "strategies": [{
                        "id": "discord_1.conf",
                        "category": "discord",
                        "artifact": config_relative,
                        "dependencies": []
                    }]
                }]
            }))
            .unwrap(),
        )
        .unwrap();

        let layout = ProtectedLayout::inspect(&program_files, &install_root).unwrap();
        let plan = layout
            .load_verified_catalog()
            .unwrap()
            .resolve_dpi_plan(&DpiStartRequest {
                engine: DpiEngine::Legacy,
                selections: vec![DpiSelection {
                    category: DpiCategory::Discord,
                    strategy_id: "discord_1.conf".into(),
                }],
                options: DpiRuntimeOptions {
                    zapret2_level: 0,
                    legacy_reliability: true,
                    zapret2_overrides: Vec::new(),
                },
            })
            .unwrap();
        let state = ProtectedDataLayout::inspect(&program_data, &state_root).unwrap();
        (test, plan, state)
    }

    #[derive(Default)]
    struct FakeState {
        launches: usize,
        stopped: usize,
        corrupt_identity: bool,
        stop_times_out: bool,
    }

    #[derive(Clone)]
    struct FakeLauncher(Arc<Mutex<FakeState>>);

    struct FakeGroup {
        state: Arc<Mutex<FakeState>>,
        identities: Vec<ProcessIdentity>,
    }

    impl RuntimeProcessLauncher for FakeLauncher {
        type Group = FakeGroup;

        fn launch_group(
            &self,
            launches: &[MaterializedLaunch],
            readiness_timeout: Duration,
        ) -> Result<Self::Group, ProcessError> {
            assert_eq!(readiness_timeout, READINESS_TIMEOUT);
            let mut state = self.0.lock().unwrap();
            state.launches += 1;
            let identities = launches
                .iter()
                .enumerate()
                .map(|(index, launch)| ProcessIdentity {
                    pid: index as u32 + 100,
                    creation_time_100ns: index as u64 + 500,
                    executable_sha256: if state.corrupt_identity {
                        "0".repeat(64)
                    } else {
                        launch.executable_sha256().to_owned()
                    },
                })
                .collect();
            drop(state);
            Ok(FakeGroup {
                state: self.0.clone(),
                identities,
            })
        }
    }

    impl RuntimeProcessGroup for FakeGroup {
        fn identities(&self) -> &[ProcessIdentity] {
            &self.identities
        }

        fn stop(&mut self, timeout: Duration) -> Result<(), ProcessError> {
            assert_eq!(timeout, STOP_TIMEOUT);
            let mut state = self.state.lock().unwrap();
            state.stopped += 1;
            if state.stop_times_out {
                Err(ProcessError::StopTimedOut)
            } else {
                Ok(())
            }
        }
    }

    #[test]
    fn new_executor_never_starts_or_restores_dpi_while_reading_status() {
        let (_test, plan, layout) = setup_plan();
        // Stale materialized files are not authority to resume an old session.
        let _stale = layout.materialize(&plan, 77).unwrap();
        let state = Arc::new(Mutex::new(FakeState::default()));
        let executor = DpiExecutor::new(layout, FakeLauncher(state.clone()));
        for _ in 0..3 {
            assert!(executor.snapshot().is_none());
            assert!(executor.active_processes().is_empty());
        }
        assert_eq!(state.lock().unwrap().launches, 0);
    }

    #[test]
    fn windows_quoting_preserves_response_file_paths() {
        assert_eq!(quote_windows_argument("plain"), "plain");
        assert_eq!(
            quote_windows_argument(r"@C:\Program Data\Obsession\effective.conf"),
            r#""@C:\Program Data\Obsession\effective.conf""#
        );
        assert_eq!(quote_windows_argument(r#"a\"b"#), r#""a\\\"b""#);
    }

    #[test]
    fn executor_owns_exact_generation_fingerprint_and_process_identity() {
        let (_test, plan, state_layout) = setup_plan();
        let fake_state = Arc::new(Mutex::new(FakeState::default()));
        let mut executor = DpiExecutor::new(state_layout.clone(), FakeLauncher(fake_state.clone()));

        let started = executor.start(&plan).unwrap();
        assert_ne!(started.generation, 0);
        assert_eq!(executor.snapshot().unwrap().generation, started.generation);
        assert_eq!(executor.active_fingerprint().unwrap().len(), 64);
        assert_eq!(executor.active_processes().len(), 1);
        assert!(matches!(executor.start(&plan), Err(ExecutorError::Busy)));
        assert!(matches!(
            executor.stop(started.generation + 1),
            Err(ExecutorError::Conflict)
        ));
        assert!(executor.snapshot().is_some());

        let generation_dir = state_layout
            .root()
            .join("dpi/generations")
            .join(started.generation.to_string());
        assert!(generation_dir.is_dir());
        executor.stop(started.generation).unwrap();
        assert!(executor.snapshot().is_none());
        assert!(!generation_dir.exists());
        let state = fake_state.lock().unwrap();
        assert_eq!(state.launches, 1);
        assert_eq!(state.stopped, 1);
    }

    #[test]
    fn incomplete_identity_is_killed_and_materialization_is_rolled_back() {
        let (_test, plan, state_layout) = setup_plan();
        let fake_state = Arc::new(Mutex::new(FakeState {
            corrupt_identity: true,
            ..FakeState::default()
        }));
        let mut executor = DpiExecutor::new(state_layout.clone(), FakeLauncher(fake_state.clone()));
        assert!(matches!(
            executor.start(&plan),
            Err(ExecutorError::InvalidOwnership)
        ));
        assert!(executor.snapshot().is_none());
        assert_eq!(fake_state.lock().unwrap().stopped, 1);
        let generations = state_layout.root().join("dpi/generations");
        assert_eq!(fs::read_dir(generations).unwrap().count(), 0);
    }

    #[test]
    fn protected_resource_tamper_fails_before_process_launch() {
        let (_test, plan, state_layout) = setup_plan();
        fs::write(plan.executable(), b"tampered-engine").unwrap();
        let fake_state = Arc::new(Mutex::new(FakeState::default()));
        let mut executor = DpiExecutor::new(state_layout, FakeLauncher(fake_state.clone()));
        assert!(matches!(
            executor.start(&plan),
            Err(ExecutorError::Materialization(
                MaterializationError::Layout(_)
            ))
        ));
        assert_eq!(fake_state.lock().unwrap().launches, 0);
    }

    #[test]
    fn stop_timeout_is_reported_but_stale_generation_is_not_retained() {
        let (_test, plan, state_layout) = setup_plan();
        let fake_state = Arc::new(Mutex::new(FakeState {
            stop_times_out: true,
            ..FakeState::default()
        }));
        let mut executor = DpiExecutor::new(state_layout.clone(), FakeLauncher(fake_state));
        let generation = executor.start(&plan).unwrap().generation;
        assert!(matches!(
            executor.stop(generation),
            Err(ExecutorError::Process(ProcessError::StopTimedOut))
        ));
        assert!(executor.snapshot().is_none());
        assert!(!state_layout
            .root()
            .join("dpi/generations")
            .join(generation.to_string())
            .exists());
    }
}
