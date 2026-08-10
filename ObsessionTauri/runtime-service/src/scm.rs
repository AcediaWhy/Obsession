//! Windows Service Control Manager host for the protected runtime.
//!
//! A complete startup preflight is performed before the service reports
//! `RUNNING`. Failure leaves a queryable locked backend for this service
//! lifetime; it never retries into a privileged capability behind the client's
//! back. The machine installer still does not register or start this service.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread;
use std::time::Duration;

use windows::core::{w, Error as WindowsError, PCWSTR, PWSTR};
use windows::Win32::Foundation::{
    ERROR_CALL_NOT_IMPLEMENTED, ERROR_SERVICE_SPECIFIC_ERROR, NO_ERROR,
};
use windows::Win32::System::Services::{
    RegisterServiceCtrlHandlerExW, SetServiceStatus, StartServiceCtrlDispatcherW,
    SERVICE_ACCEPT_SHUTDOWN, SERVICE_ACCEPT_STOP, SERVICE_CONTROL_INTERROGATE,
    SERVICE_CONTROL_SHUTDOWN, SERVICE_CONTROL_STOP, SERVICE_RUNNING, SERVICE_START_PENDING,
    SERVICE_STATUS, SERVICE_STATUS_CURRENT_STATE, SERVICE_STATUS_HANDLE, SERVICE_STOPPED,
    SERVICE_STOP_PENDING, SERVICE_TABLE_ENTRYW, SERVICE_WIN32_OWN_PROCESS,
};

use crate::dpi_executor::WindowsJobLauncher;
use crate::named_pipe::SecureNamedPipeServer;
use crate::protected_backend::{BackendInitializationError, ProtectedDpiBackend};
use crate::{LockedBackend, RuntimeBackend};

pub const SERVICE_NAME: &str = "ObsessionRuntime";

const SERVICE_NAME_WIDE: PCWSTR = w!("ObsessionRuntime");
const START_WAIT_HINT_MS: u32 = 5_000;
const STOP_WAIT_HINT_MS: u32 = 6_000;
const CLIENT_FAILURE_BACKOFF: Duration = Duration::from_millis(100);

static STOP_REQUESTED: AtomicBool = AtomicBool::new(false);
// windows::SERVICE_STATUS_HANDLE contains a raw pointer and is not Sync. SCM
// owns the handle for the service lifetime; callbacks exchange only its stable
// numeric value and never close it.
static STATUS_HANDLE_VALUE: AtomicUsize = AtomicUsize::new(0);

/// Blocks in the Windows SCM dispatcher. Running the executable directly does
/// not invoke `service_main`; Windows returns
/// `ERROR_FAILED_SERVICE_CONTROLLER_CONNECT` instead.
pub fn run_dispatcher() -> Result<(), WindowsError> {
    let entries = [
        SERVICE_TABLE_ENTRYW {
            lpServiceName: PWSTR(SERVICE_NAME_WIDE.0 as *mut u16),
            lpServiceProc: Some(service_main),
        },
        SERVICE_TABLE_ENTRYW::default(),
    ];
    unsafe { StartServiceCtrlDispatcherW(entries.as_ptr()) }
}

unsafe extern "system" fn service_main(_argument_count: u32, _argument_vectors: *mut PWSTR) {
    STOP_REQUESTED.store(false, Ordering::Release);

    let status_handle = match unsafe {
        RegisterServiceCtrlHandlerExW(SERVICE_NAME_WIDE, Some(control_handler), None)
    } {
        Ok(handle) => handle,
        Err(_) => return,
    };
    STATUS_HANDLE_VALUE.store(status_handle.0 as usize, Ordering::Release);

    if report_status(SERVICE_START_PENDING, NO_ERROR.0, 1, START_WAIT_HINT_MS).is_err() {
        STATUS_HANDLE_VALUE.store(0, Ordering::Release);
        return;
    }
    if STOP_REQUESTED.load(Ordering::Acquire) {
        let _ = report_status(SERVICE_STOPPED, NO_ERROR.0, 0, 0);
        STATUS_HANDLE_VALUE.store(0, Ordering::Release);
        return;
    }
    let backend = ProtectedDpiBackend::discover();
    if STOP_REQUESTED.load(Ordering::Acquire) {
        let _ = report_status(SERVICE_STOPPED, NO_ERROR.0, 0, 0);
        STATUS_HANDLE_VALUE.store(0, Ordering::Release);
        return;
    }
    if report_status(SERVICE_RUNNING, NO_ERROR.0, 0, 0).is_err() {
        let _ = report_status(SERVICE_STOPPED, ERROR_SERVICE_SPECIFIC_ERROR.0, 0, 0);
        STATUS_HANDLE_VALUE.store(0, Ordering::Release);
        return;
    }

    run_protected_transport_until_stopped(backend);

    let _ = report_status(SERVICE_STOPPED, NO_ERROR.0, 0, 0);
    STATUS_HANDLE_VALUE.store(0, Ordering::Release);
}

unsafe extern "system" fn control_handler(
    control: u32,
    _event_type: u32,
    _event_data: *mut std::ffi::c_void,
    _context: *mut std::ffi::c_void,
) -> u32 {
    match control {
        SERVICE_CONTROL_STOP | SERVICE_CONTROL_SHUTDOWN => {
            if !STOP_REQUESTED.swap(true, Ordering::AcqRel) {
                let _ = report_status(SERVICE_STOP_PENDING, NO_ERROR.0, 1, STOP_WAIT_HINT_MS);
            }
            NO_ERROR.0
        }
        SERVICE_CONTROL_INTERROGATE => NO_ERROR.0,
        _ => ERROR_CALL_NOT_IMPLEMENTED.0,
    }
}

fn run_protected_transport_until_stopped(
    backend: Result<ProtectedDpiBackend<WindowsJobLauncher>, BackendInitializationError>,
) {
    match backend {
        Ok(backend) => serve_until_stopped(backend),
        Err(_error) => serve_until_stopped(LockedBackend),
    }
}

fn serve_until_stopped<B: RuntimeBackend + Send>(backend: B) {
    let server = SecureNamedPipeServer::new(backend);
    while !STOP_REQUESTED.load(Ordering::Acquire) {
        match server.serve_one_until_stopped(&STOP_REQUESTED) {
            Ok(true) => {}
            Ok(false) => break,
            Err(_) if STOP_REQUESTED.load(Ordering::Acquire) => break,
            // A malformed/disconnecting client must not kill the LocalSystem
            // process or cause a hot failure loop. When startup preflight
            // failed, LockedBackend still exposes no privileged operation.
            Err(_) => thread::sleep(CLIENT_FAILURE_BACKOFF),
        }
    }
}

fn report_status(
    state: SERVICE_STATUS_CURRENT_STATE,
    exit_code: u32,
    checkpoint: u32,
    wait_hint: u32,
) -> Result<(), WindowsError> {
    let handle_value = STATUS_HANDLE_VALUE.load(Ordering::Acquire);
    if handle_value == 0 {
        return Err(WindowsError::from_win32());
    }
    let handle = SERVICE_STATUS_HANDLE(handle_value as *mut std::ffi::c_void);
    let status = build_status(state, exit_code, checkpoint, wait_hint);
    unsafe { SetServiceStatus(handle, &status) }
}

fn build_status(
    state: SERVICE_STATUS_CURRENT_STATE,
    exit_code: u32,
    checkpoint: u32,
    wait_hint: u32,
) -> SERVICE_STATUS {
    let accepts = if state == SERVICE_RUNNING {
        SERVICE_ACCEPT_STOP | SERVICE_ACCEPT_SHUTDOWN
    } else {
        0
    };
    SERVICE_STATUS {
        dwServiceType: SERVICE_WIN32_OWN_PROCESS,
        dwCurrentState: state,
        dwControlsAccepted: accepts,
        dwWin32ExitCode: exit_code,
        dwServiceSpecificExitCode: 0,
        dwCheckPoint: checkpoint,
        dwWaitHint: wait_hint,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_identity_is_fixed_and_not_client_controlled() {
        assert_eq!(SERVICE_NAME, "ObsessionRuntime");
        assert_eq!(SERVICE_NAME_WIDE, w!("ObsessionRuntime"));
    }

    #[test]
    fn only_running_state_accepts_stop_and_shutdown_controls() {
        let starting = build_status(SERVICE_START_PENDING, 0, 1, START_WAIT_HINT_MS);
        assert_eq!(starting.dwControlsAccepted, 0);
        assert_eq!(starting.dwCheckPoint, 1);

        let running = build_status(SERVICE_RUNNING, 0, 0, 0);
        assert_eq!(
            running.dwControlsAccepted,
            SERVICE_ACCEPT_STOP | SERVICE_ACCEPT_SHUTDOWN
        );

        let stopping = build_status(SERVICE_STOP_PENDING, 0, 1, STOP_WAIT_HINT_MS);
        assert_eq!(stopping.dwControlsAccepted, 0);
        assert_eq!(stopping.dwWaitHint, STOP_WAIT_HINT_MS);

        let stopped = build_status(SERVICE_STOPPED, 0, 0, 0);
        assert_eq!(stopped.dwControlsAccepted, 0);
    }
}
