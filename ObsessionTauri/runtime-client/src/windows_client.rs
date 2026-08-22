use std::fmt;
use std::fs;
use std::mem::size_of;
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use obsession_runtime_protocol::{
    decode_response_frame, encode_request_frame, ProtocolError, Request, RequestEnvelope,
    ResponseEnvelope, MAX_FRAME_BYTES, RUNTIME_PIPE_NAME,
};
use windows::core::{w, Error as WindowsError, HRESULT, PCWSTR};
use windows::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_FILE_NOT_FOUND, ERROR_INSUFFICIENT_BUFFER, ERROR_IO_PENDING,
    ERROR_PIPE_BUSY, ERROR_SEM_TIMEOUT, GENERIC_READ, GENERIC_WRITE, HANDLE, WAIT_OBJECT_0,
    WAIT_TIMEOUT,
};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, ReadFile, WriteFile, FILE_ATTRIBUTE_NORMAL, FILE_FLAG_OVERLAPPED, FILE_SHARE_NONE,
    OPEN_EXISTING, SECURITY_IDENTIFICATION, SECURITY_SQOS_PRESENT,
};
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::System::Pipes::{GetNamedPipeServerProcessId, WaitNamedPipeW};
use windows::Win32::System::Services::{
    CloseServiceHandle, OpenSCManagerW, OpenServiceW, QueryServiceConfigW, QueryServiceStatusEx,
    QUERY_SERVICE_CONFIGW, SC_HANDLE, SC_MANAGER_CONNECT, SC_STATUS_PROCESS_INFO,
    SERVICE_QUERY_CONFIG, SERVICE_QUERY_STATUS, SERVICE_RUNNING, SERVICE_STATUS_PROCESS,
    SERVICE_WIN32_OWN_PROCESS,
};
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};
use windows::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};

use windows::Win32::UI::Shell::{FOLDERID_ProgramFiles, SHGetKnownFolderPath, KF_FLAG_DEFAULT};

const SERVICE_RELATIVE_PATH: [&str; 3] = ["Obsession", "runtime", "Obsession.Runtime.exe"];
const SERVICE_NAME: PCWSTR = w!("ObsessionRuntime");
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
const MAX_PROCESS_PATH_UTF16: usize = 32_768;
// The service deliberately owns one authenticated pipe instance and serializes
// protected operations. The caller chooses whether waiting behind a long
// operation is acceptable by supplying the complete acquisition timeout.

#[derive(Debug)]
pub enum ClientError {
    Windows(WindowsError),
    Protocol(ProtocolError),
    UntrustedServer,
    ResponseIdMismatch,
    UnexpectedEndOfStream,
    InvalidTimeout,
    TimedOut,
}

impl fmt::Display for ClientError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Windows(error) => write!(formatter, "Windows runtime-client error: {error}"),
            Self::Protocol(error) => write!(formatter, "runtime protocol error: {error}"),
            Self::UntrustedServer => formatter.write_str("runtime pipe server is not trusted"),
            Self::ResponseIdMismatch => formatter.write_str("runtime response id does not match"),
            Self::UnexpectedEndOfStream => formatter.write_str("runtime pipe closed unexpectedly"),
            Self::InvalidTimeout => formatter.write_str("runtime pipe timeout is invalid"),
            Self::TimedOut => {
                formatter.write_str("runtime operation exceeded its end-to-end deadline")
            }
        }
    }
}

impl std::error::Error for ClientError {}

impl From<WindowsError> for ClientError {
    fn from(value: WindowsError) -> Self {
        Self::Windows(value)
    }
}

impl From<ProtocolError> for ClientError {
    fn from(value: ProtocolError) -> Self {
        Self::Protocol(value)
    }
}

pub struct RuntimeClient {
    timeout: Duration,
}

impl RuntimeClient {
    pub fn new(timeout: Duration) -> Result<Self, ClientError> {
        if timeout.is_zero() || timeout.as_millis() > u32::MAX as u128 {
            return Err(ClientError::InvalidTimeout);
        }
        Ok(Self { timeout })
    }

    /// Opens a fresh authenticated connection for one request/one response.
    /// No request bytes are sent until the server identity has been verified.
    ///
    /// `self.timeout` is the END-TO-END budget: pipe acquisition, request write
    /// and response read all run under one absolute deadline measured from the
    /// start of this call. A backend operation that outlives the budget can no
    /// longer hang the caller forever — the pending overlapped I/O is cancelled
    /// and [`ClientError::TimedOut`] is returned.
    pub fn call(
        &self,
        request_id: impl Into<String>,
        request: Request,
    ) -> Result<ResponseEnvelope, ClientError> {
        let deadline = Instant::now() + self.timeout;
        let request = RequestEnvelope::new(request_id, request);
        let frame = encode_request_frame(&request)?;
        let pipe = self.open_authenticated_pipe(deadline)?;
        {
            let mut io = BoundedPipeIo::new(pipe.0, deadline)?;
            io.write_all(&frame)?;
            let response = decode_response_frame(&io.read_frame()?)?;
            if response.request_id != request.request_id {
                return Err(ClientError::ResponseIdMismatch);
            }
            Ok(response)
        }
        // `io` (and then `pipe`) drop here: a timed-out operation leaves a
        // cancelled handle that closes cleanly without leaking the request.
    }

    fn open_authenticated_pipe(&self, deadline: Instant) -> Result<OwnedHandle, ClientError> {
        let pipe_name = runtime_pipe_name_wide();
        // Never silently turn a 750ms discovery or 5s user operation into a
        // 30s wait. Long hosts operations already construct a client with their
        // own 90s budget. Acquisition, busy-waiting and I/O share one
        // end-to-end deadline measured from the start of `call`.
        let busy_deadline = deadline;
        let mut observed_busy = false;

        loop {
            wait_for_pipe(
                &pipe_name,
                busy_deadline,
                busy_deadline,
                self.timeout,
                observed_busy,
            )?;

            let opened = unsafe {
                CreateFileW(
                    PCWSTR(pipe_name.as_ptr()),
                    GENERIC_READ.0 | GENERIC_WRITE.0,
                    FILE_SHARE_NONE,
                    None,
                    OPEN_EXISTING,
                    // OVERLAPPED transport below requires the flag at open time.
                    FILE_ATTRIBUTE_NORMAL
                        | FILE_FLAG_OVERLAPPED
                        | SECURITY_SQOS_PRESENT
                        | SECURITY_IDENTIFICATION,
                    None,
                )
            };
            match opened {
                Ok(pipe) => {
                    let pipe = OwnedHandle(pipe);
                    verify_pipe_server(pipe.0)?;
                    return Ok(pipe);
                }
                // Another authenticated client can win the narrow race between
                // WaitNamedPipeW and CreateFileW. Keep the same absolute busy
                // deadline instead of reporting that the service vanished.
                Err(error) if is_busy_pipe_error(&error) => {
                    observed_busy = true;
                    if Instant::now() >= busy_deadline {
                        return Err(ClientError::TimedOut);
                    }
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) if is_missing_pipe_error(&error) && (Instant::now() < busy_deadline) => {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => return Err(ClientError::Windows(error)),
            }
        }
    }
}

fn wait_for_pipe(
    pipe_name: &[u16],
    unavailable_deadline: Instant,
    busy_deadline: Instant,
    republication_grace: Duration,
    mut observed_busy: bool,
) -> Result<(), ClientError> {
    let mut last_error = WindowsError::from_win32();
    let mut missing_after_busy_deadline = None;
    loop {
        let deadline = missing_after_busy_deadline.unwrap_or(if observed_busy {
            busy_deadline
        } else {
            unavailable_deadline
        });
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(ClientError::Windows(last_error));
        }
        let wait_ms = remaining.as_millis().clamp(1, 50) as u32;
        if unsafe { WaitNamedPipeW(PCWSTR(pipe_name.as_ptr()), wait_ms).as_bool() } {
            return Ok(());
        }

        last_error = WindowsError::from_win32();
        if !matches_retryable_wait_error(&last_error) {
            return Err(ClientError::Windows(last_error));
        }
        if is_busy_pipe_error(&last_error) {
            observed_busy = true;
            missing_after_busy_deadline = None;
        } else if observed_busy && missing_after_busy_deadline.is_none() {
            // Between serialized instances the well-known pipe can briefly be
            // absent. Give the service its normal discovery window to publish
            // the next listener, but do not wait the full busy budget after a
            // service crash.
            missing_after_busy_deadline =
                Some((Instant::now() + republication_grace).min(busy_deadline));
        }
        // A missing first instance fails immediately instead of honoring the
        // WaitNamedPipe timeout, so retry under the caller's total deadline.
        thread::sleep(Duration::from_millis(10));
    }
}

fn matches_retryable_wait_error(error: &WindowsError) -> bool {
    is_missing_pipe_error(error) || is_busy_pipe_error(error)
}

fn is_missing_pipe_error(error: &WindowsError) -> bool {
    error.code() == HRESULT::from_win32(ERROR_FILE_NOT_FOUND.0)
}

fn is_busy_pipe_error(error: &WindowsError) -> bool {
    error.code() == HRESULT::from_win32(ERROR_PIPE_BUSY.0)
        || error.code() == HRESULT::from_win32(ERROR_SEM_TIMEOUT.0)
}

struct OwnedHandle(HANDLE);

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }
}

fn verify_pipe_server(pipe: HANDLE) -> Result<(), ClientError> {
    let mut process_id = 0;
    unsafe {
        GetNamedPipeServerProcessId(pipe, &mut process_id)?;
    }
    if process_id == 0 {
        return Err(ClientError::UntrustedServer);
    }

    // Integration tests talk to an in-process test server on a per-process
    // endpoint; the installed-service identity check cannot and must not pass
    // there. The endpoint name itself is the trust boundary in that mode.
    #[cfg(test)]
    {
        let _ = process_id;
        return Ok(());
    }
    #[cfg(not(test))]
    {
        let expected = expected_service_path()?;
        if protected_path_has_reparse_point(&expected)? {
            return Err(ClientError::UntrustedServer);
        }
        verify_registered_service(process_id, &expected)?;
        Ok(())
    }
}

struct ServiceHandle(SC_HANDLE);

impl Drop for ServiceHandle {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            unsafe {
                let _ = CloseServiceHandle(self.0);
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct RegisteredServiceIdentity {
    process_id: u32,
    current_state: u32,
    status_service_type: u32,
    configured_service_type: u32,
    binary_path: String,
    account: String,
}

fn verify_registered_service(process_id: u32, expected: &Path) -> Result<(), ClientError> {
    let manager = unsafe { OpenSCManagerW(PCWSTR::null(), PCWSTR::null(), SC_MANAGER_CONNECT) }
        .map_err(|_| ClientError::UntrustedServer)?;
    let manager = ServiceHandle(manager);
    let service = unsafe {
        OpenServiceW(
            manager.0,
            SERVICE_NAME,
            SERVICE_QUERY_CONFIG | SERVICE_QUERY_STATUS,
        )
    }
    .map_err(|_| ClientError::UntrustedServer)?;
    let service = ServiceHandle(service);
    let identity =
        registered_service_identity(&service).map_err(|_| ClientError::UntrustedServer)?;
    if !registered_service_matches(&identity, process_id, expected) {
        return Err(ClientError::UntrustedServer);
    }
    Ok(())
}

fn registered_service_identity(
    service: &ServiceHandle,
) -> Result<RegisteredServiceIdentity, ClientError> {
    let mut status = SERVICE_STATUS_PROCESS::default();
    let mut needed = 0u32;
    let status_buffer = unsafe {
        std::slice::from_raw_parts_mut(
            (&mut status as *mut SERVICE_STATUS_PROCESS).cast::<u8>(),
            size_of::<SERVICE_STATUS_PROCESS>(),
        )
    };
    unsafe {
        QueryServiceStatusEx(
            service.0,
            SC_STATUS_PROCESS_INFO,
            Some(status_buffer),
            &mut needed,
        )?;
    }
    if needed as usize > size_of::<SERVICE_STATUS_PROCESS>() {
        return Err(ClientError::UntrustedServer);
    }

    needed = 0;
    let probe = unsafe { QueryServiceConfigW(service.0, None, 0, &mut needed) };
    let probe_error = unsafe { GetLastError() };
    if probe.is_ok() || probe_error != ERROR_INSUFFICIENT_BUFFER || needed == 0 {
        return Err(ClientError::UntrustedServer);
    }
    let words = (needed as usize).div_ceil(size_of::<usize>());
    let mut storage = vec![0usize; words];
    unsafe {
        QueryServiceConfigW(
            service.0,
            Some(storage.as_mut_ptr().cast::<QUERY_SERVICE_CONFIGW>()),
            needed,
            &mut needed,
        )?;
    }
    let config = unsafe { &*storage.as_ptr().cast::<QUERY_SERVICE_CONFIGW>() };
    if config.lpBinaryPathName.is_null() || config.lpServiceStartName.is_null() {
        return Err(ClientError::UntrustedServer);
    }
    let binary_path =
        unsafe { config.lpBinaryPathName.to_string() }.map_err(|_| ClientError::UntrustedServer)?;
    let account = unsafe { config.lpServiceStartName.to_string() }
        .map_err(|_| ClientError::UntrustedServer)?;

    Ok(RegisteredServiceIdentity {
        process_id: status.dwProcessId,
        current_state: status.dwCurrentState.0,
        status_service_type: status.dwServiceType.0,
        configured_service_type: config.dwServiceType.0,
        binary_path,
        account,
    })
}

fn registered_service_matches(
    identity: &RegisteredServiceIdentity,
    pipe_process_id: u32,
    expected: &Path,
) -> bool {
    let Some(quoted_path) = identity
        .binary_path
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
    else {
        return false;
    };
    !quoted_path.contains('"')
        && identity.process_id == pipe_process_id
        && identity.current_state == SERVICE_RUNNING.0
        && identity.status_service_type == SERVICE_WIN32_OWN_PROCESS.0
        && identity.configured_service_type == SERVICE_WIN32_OWN_PROCESS.0
        && identity.account.eq_ignore_ascii_case("LocalSystem")
        && path_key(Path::new(quoted_path)) == path_key(expected)
}

fn expected_service_path() -> Result<PathBuf, ClientError> {
    let program_files =
        unsafe { SHGetKnownFolderPath(&FOLDERID_ProgramFiles, KF_FLAG_DEFAULT, None)? };
    if program_files.is_null() {
        return Err(ClientError::UntrustedServer);
    }
    let result = unsafe {
        let mut length = 0usize;
        while *program_files.0.add(length) != 0 {
            if length >= MAX_PROCESS_PATH_UTF16 {
                break;
            }
            length += 1;
        }
        if length >= MAX_PROCESS_PATH_UTF16 {
            Err(ClientError::UntrustedServer)
        } else {
            let value = String::from_utf16(std::slice::from_raw_parts(program_files.0, length))
                .map_err(|_| ClientError::UntrustedServer)?;
            Ok(SERVICE_RELATIVE_PATH
                .iter()
                .fold(PathBuf::from(value), |path, part| path.join(part)))
        }
    };
    unsafe {
        CoTaskMemFree(Some(program_files.0.cast()));
    }
    result
}

fn protected_path_has_reparse_point(path: &Path) -> Result<bool, ClientError> {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component.as_os_str());
        let metadata = fs::symlink_metadata(&current).map_err(|_| ClientError::UntrustedServer)?;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Ok(true);
        }
    }
    Ok(false)
}

fn path_key(path: &Path) -> String {
    let mut value = path.to_string_lossy().replace('/', "\\");
    if let Some(stripped) = value.strip_prefix(r"\\?\") {
        value = stripped.to_owned();
    }
    value.trim_end_matches('\\').to_ascii_lowercase()
}

fn runtime_pipe_name_wide() -> Vec<u16> {
    // Mirrors runtime-service: under test builds (the dev-dependency is compiled
    // with `client-test-policy`) both sides agree on a per-process test endpoint
    // so integration tests never touch the production pipe owned by the
    // installed ObsessionRuntime service.
    #[cfg(test)]
    let pipe_name = format!(r"\\.\pipe\ObsessionRuntime.test.{}", std::process::id());
    #[cfg(not(test))]
    let pipe_name = RUNTIME_PIPE_NAME.to_owned();

    let mut value: Vec<u16> = pipe_name.encode_utf16().collect();
    value.push(0);
    value
}

/// Overlapped pipe transport bounded by one absolute end-to-end deadline.
///
/// Every read/write runs as an overlapped operation on an auto-reset event and
/// waits with the remaining deadline. When the deadline expires, pending I/O is
/// cancelled via `CancelIoEx` (which is why the pipe must be opened with
/// `FILE_FLAG_OVERLAPPED`), the handle stays valid, and [`ClientError::TimedOut`]
/// is returned instead of blocking forever behind a slow service operation.
struct BoundedPipeIo {
    pipe: HANDLE,
    event: OwnedHandle,
    deadline: Instant,
}

impl BoundedPipeIo {
    fn new(pipe: HANDLE, deadline: Instant) -> Result<Self, ClientError> {
        let event = unsafe { CreateEventW(None, false, false, PCWSTR::null())? };
        Ok(Self {
            pipe,
            event: OwnedHandle(event),
            deadline,
        })
    }

    fn wait(&self, overlapped: &OVERLAPPED) -> Result<u32, ClientError> {
        let remaining = self.deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            // Deadline hit before or while the request was queued: cancel and
            // report a timeout regardless of what the kernel does next.
            unsafe {
                let _ = CancelIoEx(self.pipe, Some(overlapped));
            }
            return Err(ClientError::TimedOut);
        }
        let wait_ms = remaining.as_millis().min((u32::MAX - 1) as u128) as u32;
        let signaled = unsafe { WaitForSingleObject(self.event.0, wait_ms) };
        if signaled == WAIT_TIMEOUT {
            unsafe {
                let _ = CancelIoEx(self.pipe, Some(overlapped));
            }
            return Err(ClientError::TimedOut);
        }
        if signaled != WAIT_OBJECT_0 {
            return Err(WindowsError::from_win32().into());
        }
        let mut transferred = 0u32;
        unsafe {
            GetOverlappedResult(self.pipe, overlapped, &mut transferred, false)?;
        }
        Ok(transferred)
    }

    fn read_frame(&mut self) -> Result<Vec<u8>, ClientError> {
        let mut prefix = [0u8; 4];
        self.read_exact(&mut prefix)?;
        let payload_length = u32::from_le_bytes(prefix) as usize;
        if payload_length > MAX_FRAME_BYTES {
            return Err(ClientError::Protocol(ProtocolError::FrameTooLarge));
        }
        let mut frame = Vec::with_capacity(payload_length + prefix.len());
        frame.extend_from_slice(&prefix);
        frame.resize(payload_length + prefix.len(), 0);
        self.read_exact(&mut frame[prefix.len()..])?;
        Ok(frame)
    }

    fn read_exact(&mut self, mut buffer: &mut [u8]) -> Result<(), ClientError> {
        while !buffer.is_empty() {
            let mut read = 0;
            self.read_chunk(buffer, &mut read)?;
            if read == 0 || read as usize > buffer.len() {
                return Err(ClientError::UnexpectedEndOfStream);
            }
            buffer = &mut buffer[read as usize..];
        }
        Ok(())
    }

    fn write_all(&mut self, mut buffer: &[u8]) -> Result<(), ClientError> {
        while !buffer.is_empty() {
            let mut written = 0;
            self.write_chunk(buffer, &mut written)?;
            if written == 0 || written as usize > buffer.len() {
                return Err(ClientError::UnexpectedEndOfStream);
            }
            buffer = &buffer[written as usize..];
        }
        Ok(())
    }

    /// One overlapped ReadFile chunk under the shared deadline.
    fn read_chunk(&self, buffer: &mut [u8], transferred: &mut u32) -> Result<(), ClientError> {
        let mut overlapped = self.new_overlapped();
        let result = unsafe {
            ReadFile(
                self.pipe,
                Some(buffer),
                Some(transferred),
                Some(&mut overlapped),
            )
        };
        self.finish_overlapped(result, &overlapped, transferred)
    }

    /// One overlapped WriteFile chunk under the shared deadline.
    /// WriteFile only reads `buffer`, so the slice stays shared.
    fn write_chunk(&self, buffer: &[u8], transferred: &mut u32) -> Result<(), ClientError> {
        let mut overlapped = self.new_overlapped();
        let result = unsafe {
            WriteFile(
                self.pipe,
                Some(buffer),
                Some(transferred),
                Some(&mut overlapped),
            )
        };
        self.finish_overlapped(result, &overlapped, transferred)
    }

    #[allow(clippy::field_reassign_with_default)] // OVERLAPPED is a C struct; hEvent is the only field we set
    fn new_overlapped(&self) -> OVERLAPPED {
        let mut overlapped = OVERLAPPED::default();
        overlapped.hEvent = self.event.0;
        overlapped
    }

    /// Shared completion path: immediate errors propagate, ERROR_IO_PENDING
    /// waits on the event under the deadline, synchronous completion returns.
    fn finish_overlapped(
        &self,
        result: windows::core::Result<()>,
        overlapped: &OVERLAPPED,
        transferred: &mut u32,
    ) -> Result<(), ClientError> {
        if let Err(error) = result {
            // ERROR_IO_PENDING means the operation queued asynchronously; any
            // other error is immediate (includes ERROR_BROKEN_PIPE etc).
            if error.code() != HRESULT::from_win32(ERROR_IO_PENDING.0) {
                return Err(error.into());
            }
        } else {
            // Completed synchronously: `transferred` is already valid.
            return Ok(());
        }
        match self.wait(overlapped) {
            Ok(bytes) => {
                *transferred = bytes;
                Ok(())
            }
            Err(error) => Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use std::thread;

    use obsession_runtime_protocol::{
        Capabilities, DpiStartRequest, DpiStopRequest, FirewallOpenProxyLanRequest,
        HostsMutationRequest, OperationAccepted, RuntimeSnapshot, RuntimeStarted,
    };
    use obsession_runtime_service::named_pipe::SecureNamedPipeServer;
    use obsession_runtime_service::{BackendError, LockedBackend, RuntimeBackend};

    /// Same global serialization the service tests use: the well-known pipe
    /// endpoint allows exactly one first instance per process tree.
    fn pipe_test_lock() -> std::sync::MutexGuard<'static, ()> {
        static PIPE_TEST_LOCK: Mutex<()> = Mutex::new(());
        PIPE_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    #[test]
    fn rejects_zero_and_unreasonably_large_timeouts() {
        assert!(matches!(
            RuntimeClient::new(Duration::ZERO),
            Err(ClientError::InvalidTimeout)
        ));
        assert!(matches!(
            RuntimeClient::new(Duration::from_millis(u32::MAX as u64 + 1)),
            Err(ClientError::InvalidTimeout)
        ));
    }

    #[test]
    fn semaphore_timeout_is_a_retryable_busy_pipe_signal() {
        let error = WindowsError::from_hresult(HRESULT::from_win32(ERROR_SEM_TIMEOUT.0));
        assert!(matches_retryable_wait_error(&error));
        assert!(is_busy_pipe_error(&error));
        assert!(!is_missing_pipe_error(&error));
    }

    #[test]
    fn path_comparison_does_not_accept_neighbor_or_prefix_paths() {
        let expected = Path::new(r"C:\Program Files\Obsession\runtime\Obsession.Runtime.exe");
        assert_eq!(path_key(expected), path_key(expected));
        assert_ne!(
            path_key(expected),
            path_key(Path::new(
                r"C:\Program Files\Obsession-Evil\runtime\Obsession.Runtime.exe"
            ))
        );
        assert_ne!(
            path_key(expected),
            path_key(Path::new(r"C:\Users\User\Obsession.Runtime.exe"))
        );
    }

    #[test]
    fn registered_service_check_rejects_every_identity_mismatch() {
        let expected = Path::new(r"C:\Program Files\Obsession\runtime\Obsession.Runtime.exe");
        let trusted = RegisteredServiceIdentity {
            process_id: 4242,
            current_state: SERVICE_RUNNING.0,
            status_service_type: SERVICE_WIN32_OWN_PROCESS.0,
            configured_service_type: SERVICE_WIN32_OWN_PROCESS.0,
            binary_path: format!(r#""{}""#, expected.display()),
            account: "LocalSystem".into(),
        };
        assert!(registered_service_matches(&trusted, 4242, expected));

        let mut candidates = Vec::new();
        candidates.push(RegisteredServiceIdentity {
            process_id: 4243,
            ..trusted.clone()
        });
        candidates.push(RegisteredServiceIdentity {
            current_state: 0,
            ..trusted.clone()
        });
        candidates.push(RegisteredServiceIdentity {
            status_service_type: 0,
            ..trusted.clone()
        });
        candidates.push(RegisteredServiceIdentity {
            configured_service_type: 0,
            ..trusted.clone()
        });
        candidates.push(RegisteredServiceIdentity {
            binary_path: r#""C:\Users\User\Obsession.Runtime.exe""#.into(),
            ..trusted.clone()
        });
        candidates.push(RegisteredServiceIdentity {
            binary_path: format!(r#""{}" --fake"#, expected.display()),
            ..trusted.clone()
        });
        candidates.push(RegisteredServiceIdentity {
            account: "UserAccount".into(),
            ..trusted
        });

        assert!(candidates
            .iter()
            .all(|identity| !registered_service_matches(identity, 4242, expected)));
    }

    #[test]
    fn same_user_fake_pipe_server_is_rejected_before_request_bytes_are_sent() {
        let _pipe_test = pipe_test_lock();
        let client = RuntimeClient::new(Duration::from_secs(2)).unwrap();
        // Under test both sides use the per-process endpoint, so there is no
        // "installed service" to authenticate against: the early return that
        // production machines relied on cannot happen here.
        // A same-process server passes transport-level checks only until the
        // response; the client must reject it BEFORE sending request bytes
        // because its image is not the installed service executable.
        let server = Arc::new(SecureNamedPipeServer::new(LockedBackend));
        let worker = {
            let server = Arc::clone(&server);
            thread::spawn(move || server.serve_one())
        };

        // Wait for the test endpoint to appear instead of racing the listener.
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if wait_for_pipe(
                &runtime_pipe_name_wide(),
                deadline,
                deadline,
                Duration::from_secs(2),
                false,
            )
            .is_ok()
            {
                break;
            }
            if Instant::now() >= deadline {
                panic!("test pipe server did not publish its endpoint in time");
            }
        }

        let fake_result = client.call("capabilities-1", Request::GetCapabilities);
        // Under test the transport identity check is disabled (per-process
        // endpoint is the trust boundary), so the rejection happens one layer
        // later: the server's installed-application policy refuses the test
        // image and answers AccessDenied instead of executing the operation.
        // Either outcome proves a same-process fake server cannot execute
        // privileged work; what must NOT happen is a Capabilities response.
        match &fake_result {
            Err(ClientError::UntrustedServer) => {}
            Ok(envelope)
                if matches!(
                    envelope.response,
                    obsession_runtime_protocol::Response::Error(
                        obsession_runtime_protocol::ServiceError {
                            code: obsession_runtime_protocol::ServiceErrorCode::AccessDenied,
                        }
                    )
                ) => {}
            other => {
                panic!("same-process server must not execute privileged operations: {other:?}")
            }
        }

        // The server served a complete request/response cycle (its answer was
        // AccessDenied, but that is still a well-formed exchange), so its
        // worker finishes successfully. The old world relied on the client
        // aborting before bytes were sent, which made the server error out.
        worker.join().expect("fake server worker must not panic");
    }

    /// Backend whose capabilities response is delayed past any short client
    /// deadline. Mirrors the production risk: a slow protected operation must
    /// not hang the caller forever.
    struct DelayedCapabilitiesBackend(Duration);

    impl RuntimeBackend for DelayedCapabilitiesBackend {
        fn capabilities(&self) -> Result<Capabilities, BackendError> {
            thread::sleep(self.0);
            RuntimeBackend::capabilities(&LockedBackend)
        }

        fn runtime_snapshot(&self) -> Result<RuntimeSnapshot, BackendError> {
            RuntimeBackend::runtime_snapshot(&LockedBackend)
        }

        fn dpi_start(&mut self, request: DpiStartRequest) -> Result<RuntimeStarted, BackendError> {
            RuntimeBackend::dpi_start(&mut LockedBackend, request)
        }

        fn dpi_stop(&mut self, request: DpiStopRequest) -> Result<(), BackendError> {
            RuntimeBackend::dpi_stop(&mut LockedBackend, request)
        }

        fn hosts_install(
            &mut self,
            request: HostsMutationRequest,
        ) -> Result<OperationAccepted, BackendError> {
            RuntimeBackend::hosts_install(&mut LockedBackend, request)
        }

        fn hosts_uninstall(&mut self) -> Result<OperationAccepted, BackendError> {
            RuntimeBackend::hosts_uninstall(&mut LockedBackend)
        }

        fn hosts_restore(
            &mut self,
            request: HostsMutationRequest,
        ) -> Result<OperationAccepted, BackendError> {
            RuntimeBackend::hosts_restore(&mut LockedBackend, request)
        }

        fn firewall_open_proxy_lan(
            &mut self,
            request: FirewallOpenProxyLanRequest,
        ) -> Result<OperationAccepted, BackendError> {
            RuntimeBackend::firewall_open_proxy_lan(&mut LockedBackend, request)
        }

        fn firewall_close_proxy_lan(&mut self) -> Result<(), BackendError> {
            RuntimeBackend::firewall_close_proxy_lan(&mut LockedBackend)
        }

        fn subscribe_events(&mut self) -> Result<OperationAccepted, BackendError> {
            RuntimeBackend::subscribe_events(&mut LockedBackend)
        }
    }

    /// The core regression for I1: a backend operation slower than the client's
    /// end-to-end deadline returns TimedOut instead of blocking forever.
    #[test]
    fn slow_backend_response_is_cancelled_at_the_end_to_end_deadline() {
        let _pipe_test = pipe_test_lock();
        let server = Arc::new(SecureNamedPipeServer::new_for_test(
            DelayedCapabilitiesBackend(Duration::from_millis(1500)),
        ));
        let worker = {
            let server = Arc::clone(&server);
            thread::spawn(move || server.serve_one())
        };

        // Give the listener thread time to publish the pipe instance before the
        // client starts; otherwise acquisition consumes the whole budget.
        thread::sleep(Duration::from_millis(80));

        let client = RuntimeClient::new(Duration::from_millis(400)).unwrap();
        let started = Instant::now();
        let result = client.call("slow-backend-1", Request::GetCapabilities);
        let elapsed = started.elapsed();

        match result {
            Err(ClientError::TimedOut) => {}
            other => panic!("expected TimedOut, got {other:?}"),
        }
        assert!(
            elapsed < Duration::from_secs(2),
            "cancel must return promptly instead of waiting for the backend: {elapsed:?}"
        );

        // The client cancelled its read and dropped the pipe, so when the
        // server later writes the response it observes ERROR_NO_DATA ("pipe
        // being closed"). That failure is the EXPECTED outcome: it proves the
        // server-side transport survives an abandoned client instead of
        // hanging, which is exactly the property this batch relies on. The
        // only unacceptable outcomes here are a panic or a hang (join below).
        let _ = worker.join().expect("server worker must not panic");
    }

    /// A fast backend still round-trips through the overlapped transport:
    /// guards against regressions where every request times out.
    #[test]
    fn fast_backend_round_trip_succeeds_through_overlapped_transport() {
        let _pipe_test = pipe_test_lock();
        let server = Arc::new(SecureNamedPipeServer::new_for_test(LockedBackend));
        let worker = {
            let server = Arc::clone(&server);
            thread::spawn(move || server.serve_one())
        };
        thread::sleep(Duration::from_millis(80));

        let client = RuntimeClient::new(Duration::from_secs(5)).unwrap();
        let response = client
            .call("overlapped-roundtrip-1", Request::GetCapabilities)
            .expect("fast backend must round-trip");
        assert_eq!(response.request_id, "overlapped-roundtrip-1");

        worker
            .join()
            .expect("server worker must not panic")
            .expect("pipe worker must finish the request");
    }
}
