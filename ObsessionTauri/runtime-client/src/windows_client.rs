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
    CloseHandle, GetLastError, ERROR_FILE_NOT_FOUND, ERROR_INSUFFICIENT_BUFFER, ERROR_PIPE_BUSY,
    ERROR_SEM_TIMEOUT, GENERIC_READ, GENERIC_WRITE, HANDLE,
};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, ReadFile, WriteFile, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_NONE, OPEN_EXISTING,
    SECURITY_IDENTIFICATION, SECURITY_SQOS_PRESENT,
};
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::System::Pipes::{GetNamedPipeServerProcessId, WaitNamedPipeW};
use windows::Win32::System::Services::{
    CloseServiceHandle, OpenSCManagerW, OpenServiceW, QueryServiceConfigW, QueryServiceStatusEx,
    QUERY_SERVICE_CONFIGW, SC_HANDLE, SC_MANAGER_CONNECT, SC_STATUS_PROCESS_INFO,
    SERVICE_QUERY_CONFIG, SERVICE_QUERY_STATUS, SERVICE_RUNNING, SERVICE_STATUS_PROCESS,
    SERVICE_WIN32_OWN_PROCESS,
};
use windows::Win32::UI::Shell::{FOLDERID_ProgramFiles, SHGetKnownFolderPath, KF_FLAG_DEFAULT};

const SERVICE_RELATIVE_PATH: [&str; 3] = ["Obsession", "runtime", "Obsession.Runtime.exe"];
const SERVICE_NAME: PCWSTR = w!("ObsessionRuntime");
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
const MAX_PROCESS_PATH_UTF16: usize = 32_768;
/// The service deliberately owns one authenticated pipe instance and serializes
/// protected operations. A route-health probe may hold it for up to 25 seconds,
/// which is not evidence that the service disappeared.
const MIN_BUSY_PIPE_WAIT: Duration = Duration::from_secs(30);

#[derive(Debug)]
pub enum ClientError {
    Windows(WindowsError),
    Protocol(ProtocolError),
    UntrustedServer,
    ResponseIdMismatch,
    UnexpectedEndOfStream,
    InvalidTimeout,
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
    pub fn call(
        &self,
        request_id: impl Into<String>,
        request: Request,
    ) -> Result<ResponseEnvelope, ClientError> {
        let request = RequestEnvelope::new(request_id, request);
        let frame = encode_request_frame(&request)?;
        let pipe = self.open_authenticated_pipe()?;
        write_all(pipe.0, &frame)?;
        let response = decode_response_frame(&read_frame(pipe.0)?)?;
        if response.request_id != request.request_id {
            return Err(ClientError::ResponseIdMismatch);
        }
        Ok(response)
    }

    fn open_authenticated_pipe(&self) -> Result<OwnedHandle, ClientError> {
        let pipe_name = runtime_pipe_name_wide();
        let started = Instant::now();
        let unavailable_deadline = started + self.timeout;
        let busy_deadline = started + self.timeout.max(MIN_BUSY_PIPE_WAIT);
        let mut observed_busy = false;

        loop {
            wait_for_pipe(
                &pipe_name,
                unavailable_deadline,
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
                    FILE_ATTRIBUTE_NORMAL | SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
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
                        return Err(ClientError::Windows(error));
                    }
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error)
                    if is_missing_pipe_error(&error)
                        && (Instant::now() < unavailable_deadline
                            || (observed_busy && Instant::now() < busy_deadline)) =>
                {
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

    let expected = expected_service_path()?;
    if protected_path_has_reparse_point(&expected)? {
        return Err(ClientError::UntrustedServer);
    }
    verify_registered_service(process_id, &expected)?;
    Ok(())
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
    let mut value: Vec<u16> = RUNTIME_PIPE_NAME.encode_utf16().collect();
    value.push(0);
    value
}

fn read_frame(pipe: HANDLE) -> Result<Vec<u8>, ClientError> {
    let mut prefix = [0u8; 4];
    read_exact(pipe, &mut prefix)?;
    let payload_length = u32::from_le_bytes(prefix) as usize;
    if payload_length > MAX_FRAME_BYTES {
        return Err(ClientError::Protocol(ProtocolError::FrameTooLarge));
    }
    let mut frame = Vec::with_capacity(payload_length + prefix.len());
    frame.extend_from_slice(&prefix);
    frame.resize(payload_length + prefix.len(), 0);
    read_exact(pipe, &mut frame[prefix.len()..])?;
    Ok(frame)
}

fn read_exact(pipe: HANDLE, mut buffer: &mut [u8]) -> Result<(), ClientError> {
    while !buffer.is_empty() {
        let mut read = 0;
        unsafe {
            ReadFile(pipe, Some(buffer), Some(&mut read), None)?;
        }
        if read == 0 || read as usize > buffer.len() {
            return Err(ClientError::UnexpectedEndOfStream);
        }
        buffer = &mut buffer[read as usize..];
    }
    Ok(())
}

fn write_all(pipe: HANDLE, mut buffer: &[u8]) -> Result<(), ClientError> {
    while !buffer.is_empty() {
        let mut written = 0;
        unsafe {
            WriteFile(pipe, Some(buffer), Some(&mut written), None)?;
        }
        if written == 0 || written as usize > buffer.len() {
            return Err(ClientError::UnexpectedEndOfStream);
        }
        buffer = &buffer[written as usize..];
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;

    use obsession_runtime_service::named_pipe::SecureNamedPipeServer;
    use obsession_runtime_service::LockedBackend;

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
        let client = RuntimeClient::new(Duration::from_secs(2)).unwrap();
        // A developer machine may already have the production service holding
        // the global endpoint. In that case, authenticate the real LocalSystem
        // service instead of trying to squat on its first pipe instance.
        let installed_result = client.call("installed-capabilities-1", Request::GetCapabilities);
        if installed_result.is_ok() {
            return;
        }

        let server = Arc::new(SecureNamedPipeServer::new(LockedBackend));
        let worker = {
            let server = Arc::clone(&server);
            thread::spawn(move || server.serve_one())
        };

        let fake_result = client.call("capabilities-1", Request::GetCapabilities);
        assert!(
            matches!(fake_result, Err(ClientError::UntrustedServer)),
            "installed endpoint result: {installed_result:?}; fake endpoint result: {fake_result:?}"
        );

        assert!(worker.join().expect("fake server must not panic").is_err());
    }
}
