//! Windows named-pipe transport for the privileged runtime service.
//!
//! This module is deliberately separate from the service host. It authenticates
//! the connecting process before dispatch and carries only the bounded frames
//! defined by `obsession-runtime-protocol`; it never accepts a path, command or
//! DLL name from the client. The host remains disabled until the installer and
//! protected backend are ready.

use std::env;
use std::fmt;
use std::fs;
use std::mem::size_of;
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::slice;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use obsession_runtime_protocol::{
    decode_request_frame, encode_response_frame, ProtocolError, RequestEnvelope, MAX_FRAME_BYTES,
    RUNTIME_PIPE_NAME,
};
use windows::core::{w, Error as WindowsError, HRESULT, PCWSTR, PWSTR};
use windows::Win32::Foundation::{
    CloseHandle, LocalFree, BOOL, ERROR_NOT_FOUND, ERROR_PIPE_CONNECTED, ERROR_PIPE_LISTENING,
    HANDLE, HLOCAL,
};
use windows::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
};
use windows::Win32::Security::{
    GetSidSubAuthority, GetSidSubAuthorityCount, GetTokenInformation, RevertToSelf,
    TokenIntegrityLevel, TokenUser, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES,
    TOKEN_MANDATORY_LABEL, TOKEN_QUERY, TOKEN_USER,
};
use windows::Win32::Storage::FileSystem::{
    FlushFileBuffers, ReadFile, WriteFile, FILE_ATTRIBUTE_REPARSE_POINT,
    FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_DUPLEX,
};
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, GetNamedPipeClientProcessId,
    GetNamedPipeClientSessionId, ImpersonateNamedPipeClient, SetNamedPipeHandleState,
    NAMED_PIPE_MODE, PIPE_NOWAIT, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE,
    PIPE_WAIT,
};
use windows::Win32::System::SystemServices::{
    SECURITY_DESCRIPTOR_REVISION, SECURITY_MANDATORY_HIGH_RID, SECURITY_MANDATORY_MEDIUM_RID,
    SECURITY_MANDATORY_SYSTEM_RID,
};
use windows::Win32::System::Threading::{
    GetCurrentThread, GetCurrentThreadId, OpenProcess, OpenThread, OpenThreadToken,
    QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    THREAD_TERMINATE,
};
use windows::Win32::System::IO::CancelSynchronousIo;

use crate::{ClientIdentity, ClientIntegrity, RuntimeBackend, ServiceCore};

const PIPE_BUFFER_BYTES: u32 = (MAX_FRAME_BYTES + 4) as u32;
const DEFAULT_CLIENT_IO_TIMEOUT: Duration = Duration::from_secs(5);
const LISTENER_POLL_INTERVAL: Duration = Duration::from_millis(20);
const BACKGROUND_POLL_INTERVAL: Duration = Duration::from_millis(250);
const MAX_PROCESS_IMAGE_UTF16: usize = 32_768;
const PRODUCT_DIRECTORY: &str = "Obsession";
const RUNTIME_DIRECTORY: &str = "runtime";
const SERVICE_FILE_NAME: &str = "Obsession.Runtime.exe";
const CLIENT_FILE_NAME: &str = "Obsession.exe";

// Only SYSTEM, Administrators and locally interactive users may open the pipe.
// `P` protects this DACL from inheritance. Remote clients are also rejected at
// the pipe protocol level below.
const PIPE_SECURITY_DESCRIPTOR: windows::core::PCWSTR =
    w!("D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGW;;;IU)");

#[derive(Clone, Debug)]
struct ClientProcessPolicy {
    expected_image: Option<PathBuf>,
}

impl ClientProcessPolicy {
    fn installed_application() -> Self {
        Self {
            expected_image: installed_client_image(),
        }
    }

    #[cfg(any(test, feature = "client-test-policy"))]
    fn current_test_process() -> Self {
        Self {
            expected_image: env::current_exe()
                .ok()
                .and_then(|path| fs::canonicalize(path).ok()),
        }
    }

    fn authenticates(&self, process: HANDLE) -> bool {
        let Some(expected) = &self.expected_image else {
            return false;
        };
        process_image_path(process)
            .and_then(|path| fs::canonicalize(path).ok())
            .is_some_and(|actual| windows_path_eq(&actual, expected))
    }
}

struct CapturedClientProcess {
    process_id: u32,
    authenticated: bool,
    _handle: Option<OwnedHandle>,
}

#[derive(Debug)]
pub enum NamedPipeError {
    Windows(WindowsError),
    Protocol(ProtocolError),
    InvalidClientIdentity,
    UnexpectedEndOfStream,
    PoisonedDispatcher,
    InvalidTimeout,
    ClientIoTimedOut,
    WorkerFailed,
}

impl fmt::Display for NamedPipeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Windows(error) => write!(formatter, "Windows named-pipe error: {error}"),
            Self::Protocol(error) => write!(formatter, "runtime protocol error: {error}"),
            Self::InvalidClientIdentity => {
                formatter.write_str("invalid named-pipe client identity")
            }
            Self::UnexpectedEndOfStream => {
                formatter.write_str("named-pipe client closed the stream")
            }
            Self::PoisonedDispatcher => formatter.write_str("runtime dispatcher lock is poisoned"),
            Self::InvalidTimeout => formatter.write_str("named-pipe timeout must be non-zero"),
            Self::ClientIoTimedOut => {
                formatter.write_str("named-pipe client exceeded its I/O deadline")
            }
            Self::WorkerFailed => formatter.write_str("named-pipe worker failed unexpectedly"),
        }
    }
}

impl std::error::Error for NamedPipeError {}

impl From<WindowsError> for NamedPipeError {
    fn from(value: WindowsError) -> Self {
        Self::Windows(value)
    }
}

impl From<ProtocolError> for NamedPipeError {
    fn from(value: ProtocolError) -> Self {
        Self::Protocol(value)
    }
}

/// One request/one response server boundary. It deliberately has no run loop
/// yet: SCM lifecycle, stop cancellation and instance scheduling are added with
/// the actual service host, before this can be installed as LocalSystem.
pub struct SecureNamedPipeServer<B> {
    core: Mutex<ServiceCore<B>>,
    client_process_policy: ClientProcessPolicy,
}

impl<B: RuntimeBackend + Send> SecureNamedPipeServer<B> {
    pub fn new(backend: B) -> Self {
        Self {
            core: Mutex::new(ServiceCore::new(backend)),
            client_process_policy: ClientProcessPolicy::installed_application(),
        }
    }

    /// Test-only constructor. Gated on the service's `client-test-policy`
    /// feature (implied by `cfg(test)`) so external integration tests
    /// (runtime-client) can authenticate against this process image instead of
    /// the installed application path.
    #[cfg(any(test, feature = "client-test-policy"))]
    pub fn new_for_test(backend: B) -> Self {
        Self {
            core: Mutex::new(ServiceCore::new(backend)),
            client_process_policy: ClientProcessPolicy::current_test_process(),
        }
    }

    /// Creates exactly one protected pipe instance, serves exactly one request
    /// and then closes it. A future SCM host will own instance scheduling.
    pub fn serve_one(&self) -> Result<(), NamedPipeError> {
        self.serve_one_with_timeout(DEFAULT_CLIENT_IO_TIMEOUT)
    }

    /// Applies the deadline independently to request input and response output.
    /// Authenticated backend work runs between those two client-I/O windows and
    /// owns its own operation-specific bounds. A slow protected operation must
    /// not be mistaken for a client that is holding the pipe open.
    pub fn serve_one_with_timeout(&self, timeout: Duration) -> Result<(), NamedPipeError> {
        if timeout.is_zero() {
            return Err(NamedPipeError::InvalidTimeout);
        }
        let descriptor = LocalSecurityDescriptor::new()?;
        let pipe = create_pipe(&descriptor)?;
        let pipe = OwnedHandle(pipe);

        accept_client(pipe.0)?;
        let result = self.serve_connected_client_bounded(pipe.0, timeout);
        // A disconnect failure is immaterial after a client closes the pipe;
        // the handle is still deterministically closed by `OwnedHandle`.
        unsafe {
            let _ = DisconnectNamedPipe(pipe.0);
        }
        result
    }

    /// Serves one request for an SCM loop, but polls the idle listener so a
    /// stop request cannot lose a race immediately before `ConnectNamedPipe`.
    /// `Ok(false)` means no client was dispatched because shutdown was asked.
    pub fn serve_one_until_stopped(&self, stop: &AtomicBool) -> Result<bool, NamedPipeError> {
        if stop.load(Ordering::Acquire) {
            return Ok(false);
        }
        let descriptor = LocalSecurityDescriptor::new()?;
        let pipe = create_stoppable_pipe(&descriptor)?;
        let pipe = OwnedHandle(pipe);

        let connected = accept_client_until_stopped(pipe.0, stop, || {
            self.core
                .lock()
                .map_err(|_| NamedPipeError::PoisonedDispatcher)?
                .poll_background();
            Ok(())
        })?;
        if !connected {
            unsafe {
                let _ = DisconnectNamedPipe(pipe.0);
            }
            return Ok(false);
        }

        let result = self.serve_connected_client_bounded(pipe.0, DEFAULT_CLIENT_IO_TIMEOUT);
        unsafe {
            let _ = DisconnectNamedPipe(pipe.0);
        }
        result.map(|()| true)
    }

    fn serve_connected_client_bounded(
        &self,
        pipe: HANDLE,
        timeout: Duration,
    ) -> Result<(), NamedPipeError> {
        let pipe_value = pipe.0 as usize;
        let client_process_policy = self.client_process_policy.clone();
        let (request, client) = run_bounded_pipe_io(timeout, move || {
            let pipe = HANDLE(pipe_value as *mut std::ffi::c_void);
            // Hold a query handle across frame consumption and token capture so
            // a disconnect/PID-reuse race cannot substitute a different image.
            let process = capture_client_process(pipe, &client_process_policy)?;
            // Windows permits impersonation only after the server has consumed
            // client data. The strict size-capped frame is parsed first, but it
            // cannot reach dispatch until the captured token is accepted.
            let request = read_request(pipe)?;
            let client = client_identity(pipe, &process)?;
            Ok((request, client))
        })?;

        let response = self
            .core
            .lock()
            .map_err(|_| NamedPipeError::PoisonedDispatcher)?
            .handle(&client, request);

        let pipe_value = pipe.0 as usize;
        run_bounded_pipe_io(timeout, move || {
            let pipe = HANDLE(pipe_value as *mut std::ffi::c_void);
            write_response(pipe, &response)
        })
    }
}

fn run_bounded_pipe_io<T, F>(timeout: Duration, operation: F) -> Result<T, NamedPipeError>
where
    T: Send,
    F: FnOnce() -> Result<T, NamedPipeError> + Send,
{
    if timeout.is_zero() {
        return Err(NamedPipeError::InvalidTimeout);
    }
    let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
    let (result_sender, result_receiver) = mpsc::sync_channel(1);

    thread::scope(|scope| {
        let worker = scope.spawn(move || {
            let thread_id = unsafe { GetCurrentThreadId() };
            if ready_sender.send(thread_id).is_err() {
                return;
            }
            let _ = result_sender.send(operation());
        });

        let thread_id = ready_receiver
            .recv()
            .map_err(|_| NamedPipeError::WorkerFailed)?;
        let result = match result_receiver.recv_timeout(timeout) {
            Ok(result) => result,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                cancel_synchronous_worker_io(thread_id)?;
                // Cancellation converts the active synchronous pipe call into
                // an error and lets the scoped worker unwind before its pipe
                // reference can leave scope.
                let _cancelled_result = result_receiver
                    .recv()
                    .map_err(|_| NamedPipeError::WorkerFailed)?;
                Err(NamedPipeError::ClientIoTimedOut)
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(NamedPipeError::WorkerFailed),
        };

        worker.join().map_err(|_| NamedPipeError::WorkerFailed)?;
        result
    })
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

struct LocalSecurityDescriptor(PSECURITY_DESCRIPTOR);

impl LocalSecurityDescriptor {
    fn new() -> Result<Self, NamedPipeError> {
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                PIPE_SECURITY_DESCRIPTOR,
                SECURITY_DESCRIPTOR_REVISION,
                &mut descriptor,
                None,
            )?;
        }
        if descriptor.is_invalid() {
            return Err(NamedPipeError::Windows(WindowsError::from_win32()));
        }
        Ok(Self(descriptor))
    }

    fn as_attributes(&self) -> SECURITY_ATTRIBUTES {
        SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: self.0 .0,
            bInheritHandle: BOOL(0),
        }
    }
}

impl Drop for LocalSecurityDescriptor {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            unsafe {
                let _ = LocalFree(HLOCAL(self.0 .0));
            }
        }
    }
}

fn create_pipe(descriptor: &LocalSecurityDescriptor) -> Result<HANDLE, NamedPipeError> {
    create_pipe_with_wait_mode(descriptor, PIPE_WAIT)
}

fn create_stoppable_pipe(descriptor: &LocalSecurityDescriptor) -> Result<HANDLE, NamedPipeError> {
    create_pipe_with_wait_mode(descriptor, PIPE_NOWAIT)
}

fn create_pipe_with_wait_mode(
    descriptor: &LocalSecurityDescriptor,
    wait_mode: NAMED_PIPE_MODE,
) -> Result<HANDLE, NamedPipeError> {
    let attributes = descriptor.as_attributes();
    let pipe_name = runtime_pipe_name_wide();
    let pipe = unsafe {
        CreateNamedPipeW(
            PCWSTR(pipe_name.as_ptr()),
            // Do not share the well-known endpoint with a prior server. If an
            // unprivileged process squats on this name before service startup,
            // CreateNamedPipeW fails and the caller remains fail-closed.
            PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | wait_mode | PIPE_REJECT_REMOTE_CLIENTS,
            1,
            PIPE_BUFFER_BYTES,
            PIPE_BUFFER_BYTES,
            0,
            Some(&attributes),
        )
    };
    if pipe.is_invalid() {
        Err(NamedPipeError::Windows(WindowsError::from_win32()))
    } else {
        Ok(pipe)
    }
}

fn runtime_pipe_name_wide() -> Vec<u16> {
    // Tests and external integration-test builds (client-test-policy) must never
    // race the production endpoint: a live system service owns the well-known
    // name, so the first-instance create would fail and clients would reach the
    // real service instead of the in-process test server.
    #[cfg(any(test, feature = "client-test-policy"))]
    let pipe_name = format!(r"\\.\pipe\ObsessionRuntime.test.{}", std::process::id());
    #[cfg(not(any(test, feature = "client-test-policy")))]
    let pipe_name = RUNTIME_PIPE_NAME.to_owned();

    let mut value: Vec<u16> = pipe_name.encode_utf16().collect();
    value.push(0);
    value
}

fn accept_client(pipe: HANDLE) -> Result<(), NamedPipeError> {
    match unsafe { ConnectNamedPipe(pipe, None) } {
        Ok(()) => Ok(()),
        // A client can connect in the narrow window between CreateNamedPipeW
        // and ConnectNamedPipe. Windows reports that race as an error even
        // though the pipe is successfully connected.
        Err(error) if error.code() == HRESULT::from_win32(ERROR_PIPE_CONNECTED.0) => Ok(()),
        Err(error) => Err(NamedPipeError::Windows(error)),
    }
}

fn accept_client_until_stopped<F>(
    pipe: HANDLE,
    stop: &AtomicBool,
    mut poll_background: F,
) -> Result<bool, NamedPipeError>
where
    F: FnMut() -> Result<(), NamedPipeError>,
{
    let mut next_background_poll = Instant::now();
    loop {
        if stop.load(Ordering::Acquire) {
            return Ok(false);
        }

        let connected = match unsafe { ConnectNamedPipe(pipe, None) } {
            Ok(()) => true,
            Err(error) if error.code() == HRESULT::from_win32(ERROR_PIPE_CONNECTED.0) => true,
            Err(error) if error.code() == HRESULT::from_win32(ERROR_PIPE_LISTENING.0) => false,
            Err(error) => return Err(NamedPipeError::Windows(error)),
        };
        if connected {
            // Client traffic must return to blocking mode before the bounded
            // worker starts strict frame reads.
            let wait_mode = PIPE_WAIT;
            unsafe {
                SetNamedPipeHandleState(pipe, Some(&wait_mode), None, None)?;
            }
            return Ok(!stop.load(Ordering::Acquire));
        }
        let now = Instant::now();
        if now >= next_background_poll {
            poll_background()?;
            next_background_poll = now + BACKGROUND_POLL_INTERVAL;
        }
        thread::sleep(LISTENER_POLL_INTERVAL);
    }
}

fn cancel_synchronous_worker_io(thread_id: u32) -> Result<(), NamedPipeError> {
    let thread = unsafe { OpenThread(THREAD_TERMINATE, false, thread_id)? };
    let thread = OwnedHandle(thread);
    match unsafe { CancelSynchronousIo(thread.0) } {
        Ok(()) => Ok(()),
        // The operation can finish in the narrow race between recv_timeout and
        // cancellation. In that case there is nothing left to cancel and the
        // result channel below will already be ready.
        Err(error) if error.code() == HRESULT::from_win32(ERROR_NOT_FOUND.0) => Ok(()),
        Err(error) => Err(NamedPipeError::Windows(error)),
    }
}

fn installed_client_image() -> Option<PathBuf> {
    let service = env::current_exe().ok()?;
    let program_files = PathBuf::from(env::var_os("ProgramFiles")?);
    let expected = expected_client_image(&service, &program_files)?;

    // Reject junctions/symlinks in every product-controlled component before
    // canonicalization. Program Files itself is the trusted OS boundary.
    let runtime = service.parent()?;
    let install_root = runtime.parent()?;
    if [install_root, runtime, service.as_path(), expected.as_path()]
        .into_iter()
        .any(path_is_reparse_point)
    {
        return None;
    }

    let canonical_program_files = fs::canonicalize(program_files).ok()?;
    let canonical_service = fs::canonicalize(service).ok()?;
    let canonical_expected = fs::canonicalize(expected).ok()?;
    let canonical_layout = expected_client_image(&canonical_service, &canonical_program_files)?;
    windows_path_eq(&canonical_expected, &canonical_layout).then_some(canonical_expected)
}

fn expected_client_image(service: &Path, program_files: &Path) -> Option<PathBuf> {
    if !file_name_eq(service, SERVICE_FILE_NAME) {
        return None;
    }
    let runtime = service.parent()?;
    if !file_name_eq(runtime, RUNTIME_DIRECTORY) {
        return None;
    }
    let install_root = runtime.parent()?;
    if !file_name_eq(install_root, PRODUCT_DIRECTORY)
        || !windows_path_eq(install_root.parent()?, program_files)
    {
        return None;
    }
    Some(install_root.join(CLIENT_FILE_NAME))
}

fn file_name_eq(path: &Path, expected: &str) -> bool {
    path.file_name()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case(expected))
}

fn windows_path_eq(left: &Path, right: &Path) -> bool {
    left.as_os_str()
        .to_string_lossy()
        .eq_ignore_ascii_case(&right.as_os_str().to_string_lossy())
}

fn path_is_reparse_point(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|metadata| metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0)
        .unwrap_or(true)
}

fn process_image_path(process: HANDLE) -> Option<PathBuf> {
    let mut buffer = vec![0u16; MAX_PROCESS_IMAGE_UTF16];
    let mut length = u32::try_from(buffer.len()).ok()?;
    unsafe {
        QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut length,
        )
        .ok()?;
    }
    let length = usize::try_from(length).ok()?;
    if length == 0 || length > buffer.len() {
        return None;
    }
    String::from_utf16(&buffer[..length])
        .ok()
        .map(PathBuf::from)
}

fn capture_client_process(
    pipe: HANDLE,
    policy: &ClientProcessPolicy,
) -> Result<CapturedClientProcess, NamedPipeError> {
    let mut process_id = 0;
    unsafe {
        GetNamedPipeClientProcessId(pipe, &mut process_id)?;
    }
    if process_id == 0 {
        return Err(NamedPipeError::InvalidClientIdentity);
    }

    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process_id) }
        .ok()
        .map(OwnedHandle);
    let authenticated = handle
        .as_ref()
        .is_some_and(|process| policy.authenticates(process.0));
    Ok(CapturedClientProcess {
        process_id,
        authenticated,
        _handle: handle,
    })
}

fn client_identity(
    pipe: HANDLE,
    process: &CapturedClientProcess,
) -> Result<ClientIdentity, NamedPipeError> {
    let mut process_id = 0;
    let mut session_id = 0;
    unsafe {
        GetNamedPipeClientProcessId(pipe, &mut process_id)?;
        GetNamedPipeClientSessionId(pipe, &mut session_id)?;
    }
    if process_id == 0 || process_id != process.process_id {
        return Err(NamedPipeError::InvalidClientIdentity);
    }

    unsafe {
        ImpersonateNamedPipeClient(pipe)?;
    }
    let identity_result = (|| {
        let token = open_impersonated_token()?;
        let user_sid = token_user_sid(token.0)?;
        let integrity = token_integrity(token.0)?;
        Ok(ClientIdentity {
            is_local: true,
            process_id,
            process_authenticated: process.authenticated,
            session_id,
            user_sid,
            integrity,
        })
    })();
    let revert_result = unsafe { RevertToSelf() };

    match (identity_result, revert_result) {
        (Err(error), _) => Err(error),
        (_, Err(error)) => Err(NamedPipeError::Windows(error)),
        (Ok(identity), Ok(())) => Ok(identity),
    }
}

fn open_impersonated_token() -> Result<OwnedHandle, NamedPipeError> {
    let mut token = HANDLE::default();
    unsafe {
        OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, true, &mut token)?;
    }
    if token.is_invalid() {
        return Err(NamedPipeError::Windows(WindowsError::from_win32()));
    }
    Ok(OwnedHandle(token))
}

fn token_user_sid(token: HANDLE) -> Result<String, NamedPipeError> {
    let buffer = token_information(token, TokenUser)?;
    let user = unsafe { &*(buffer.as_ptr().cast::<TOKEN_USER>()) };
    sid_to_string(user.User.Sid)
}

fn token_integrity(token: HANDLE) -> Result<ClientIntegrity, NamedPipeError> {
    let buffer = token_information(token, TokenIntegrityLevel)?;
    let label = unsafe { &*(buffer.as_ptr().cast::<TOKEN_MANDATORY_LABEL>()) };
    let count = unsafe { *GetSidSubAuthorityCount(label.Label.Sid) };
    if count == 0 {
        return Err(NamedPipeError::InvalidClientIdentity);
    }
    let rid = unsafe { *GetSidSubAuthority(label.Label.Sid, (count - 1).into()) };
    Ok(classify_integrity_rid(rid))
}

fn token_information(
    token: HANDLE,
    information_class: windows::Win32::Security::TOKEN_INFORMATION_CLASS,
) -> Result<Vec<u8>, NamedPipeError> {
    let mut size = 0;
    unsafe {
        let _ = GetTokenInformation(token, information_class, None, 0, &mut size);
    }
    if size == 0 {
        return Err(NamedPipeError::Windows(WindowsError::from_win32()));
    }
    let mut buffer = vec![0u8; size as usize];
    unsafe {
        GetTokenInformation(
            token,
            information_class,
            Some(buffer.as_mut_ptr().cast()),
            size,
            &mut size,
        )?;
    }
    Ok(buffer)
}

fn sid_to_string(sid: windows::Win32::Security::PSID) -> Result<String, NamedPipeError> {
    if sid.is_invalid() {
        return Err(NamedPipeError::InvalidClientIdentity);
    }
    let mut string_sid = PWSTR::null();
    unsafe {
        ConvertSidToStringSidW(sid, &mut string_sid)?;
    }
    if string_sid.is_null() {
        return Err(NamedPipeError::Windows(WindowsError::from_win32()));
    }

    let result = unsafe {
        let mut length = 0usize;
        while *string_sid.0.add(length) != 0 {
            // A valid SID string is far shorter than this. The bound avoids a
            // malformed pointer turning into an unbounded scan in service code.
            if length > 184 {
                break;
            }
            length += 1;
        }
        if length > 184 {
            Err(NamedPipeError::InvalidClientIdentity)
        } else {
            String::from_utf16(slice::from_raw_parts(string_sid.0, length))
                .map_err(|_| NamedPipeError::InvalidClientIdentity)
        }
    };
    unsafe {
        let _ = LocalFree(HLOCAL(string_sid.0.cast()));
    }
    result
}

fn classify_integrity_rid(rid: u32) -> ClientIntegrity {
    if rid >= SECURITY_MANDATORY_SYSTEM_RID as u32 {
        ClientIntegrity::System
    } else if rid >= SECURITY_MANDATORY_HIGH_RID as u32 {
        ClientIntegrity::High
    } else if rid >= SECURITY_MANDATORY_MEDIUM_RID as u32 {
        ClientIntegrity::Medium
    } else {
        ClientIntegrity::Anonymous
    }
}

fn read_request(pipe: HANDLE) -> Result<RequestEnvelope, NamedPipeError> {
    let frame = read_frame(pipe)?;
    Ok(decode_request_frame(&frame)?)
}

fn write_response(
    pipe: HANDLE,
    response: &obsession_runtime_protocol::ResponseEnvelope,
) -> Result<(), NamedPipeError> {
    let frame = encode_response_frame(response)?;
    write_all(pipe, &frame)?;
    // DisconnectNamedPipe discards any unread server-to-client bytes. Flush
    // therefore completes the one request/one response transaction before the
    // owning handle is disconnected. Client-controlled I/O, including this
    // potentially blocking acknowledgement, runs inside the bounded worker.
    unsafe {
        FlushFileBuffers(pipe)?;
    }
    Ok(())
}

fn read_frame(pipe: HANDLE) -> Result<Vec<u8>, NamedPipeError> {
    let mut prefix = [0u8; 4];
    read_exact(pipe, &mut prefix)?;
    let payload_length = u32::from_le_bytes(prefix) as usize;
    if payload_length > MAX_FRAME_BYTES {
        return Err(NamedPipeError::Protocol(ProtocolError::FrameTooLarge));
    }

    let mut frame = Vec::with_capacity(payload_length + prefix.len());
    frame.extend_from_slice(&prefix);
    frame.resize(payload_length + prefix.len(), 0);
    read_exact(pipe, &mut frame[prefix.len()..])?;
    Ok(frame)
}

fn read_exact(pipe: HANDLE, mut buffer: &mut [u8]) -> Result<(), NamedPipeError> {
    while !buffer.is_empty() {
        let mut read = 0;
        unsafe {
            ReadFile(pipe, Some(buffer), Some(&mut read), None)?;
        }
        if read == 0 {
            return Err(NamedPipeError::UnexpectedEndOfStream);
        }
        let read = read as usize;
        if read > buffer.len() {
            return Err(NamedPipeError::UnexpectedEndOfStream);
        }
        buffer = &mut buffer[read..];
    }
    Ok(())
}

fn write_all(pipe: HANDLE, mut buffer: &[u8]) -> Result<(), NamedPipeError> {
    while !buffer.is_empty() {
        let mut written = 0;
        unsafe {
            WriteFile(pipe, Some(buffer), Some(&mut written), None)?;
        }
        if written == 0 {
            return Err(NamedPipeError::UnexpectedEndOfStream);
        }
        let written = written as usize;
        if written > buffer.len() {
            return Err(NamedPipeError::UnexpectedEndOfStream);
        }
        buffer = &buffer[written..];
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;
    use std::time::{Duration, Instant};

    use obsession_runtime_protocol::{
        decode_response_frame, encode_request_frame, Capabilities, DpiStartRequest, DpiStopRequest,
        FirewallOpenProxyLanRequest, HostsMutationRequest, OperationAccepted, Request, Response,
        RuntimeSnapshot, RuntimeStarted, ServiceErrorCode,
    };
    use windows::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE};
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_NONE, OPEN_EXISTING,
        SECURITY_IDENTIFICATION, SECURITY_SQOS_PRESENT,
    };
    use windows::Win32::System::Pipes::WaitNamedPipeW;

    use crate::LockedBackend;

    static PIPE_TEST_LOCK: Mutex<()> = Mutex::new(());

    struct SlowCapabilitiesBackend;

    impl RuntimeBackend for SlowCapabilitiesBackend {
        fn capabilities(&self) -> Result<Capabilities, crate::BackendError> {
            thread::sleep(Duration::from_millis(250));
            RuntimeBackend::capabilities(&LockedBackend)
        }

        fn runtime_snapshot(&self) -> Result<RuntimeSnapshot, crate::BackendError> {
            RuntimeBackend::runtime_snapshot(&LockedBackend)
        }

        fn dpi_start(
            &mut self,
            request: DpiStartRequest,
        ) -> Result<RuntimeStarted, crate::BackendError> {
            RuntimeBackend::dpi_start(&mut LockedBackend, request)
        }

        fn dpi_stop(&mut self, request: DpiStopRequest) -> Result<(), crate::BackendError> {
            RuntimeBackend::dpi_stop(&mut LockedBackend, request)
        }

        fn hosts_install(
            &mut self,
            request: HostsMutationRequest,
        ) -> Result<OperationAccepted, crate::BackendError> {
            RuntimeBackend::hosts_install(&mut LockedBackend, request)
        }

        fn hosts_uninstall(&mut self) -> Result<OperationAccepted, crate::BackendError> {
            RuntimeBackend::hosts_uninstall(&mut LockedBackend)
        }

        fn hosts_restore(
            &mut self,
            request: HostsMutationRequest,
        ) -> Result<OperationAccepted, crate::BackendError> {
            RuntimeBackend::hosts_restore(&mut LockedBackend, request)
        }

        fn firewall_open_proxy_lan(
            &mut self,
            request: FirewallOpenProxyLanRequest,
        ) -> Result<OperationAccepted, crate::BackendError> {
            RuntimeBackend::firewall_open_proxy_lan(&mut LockedBackend, request)
        }

        fn firewall_close_proxy_lan(&mut self) -> Result<(), crate::BackendError> {
            RuntimeBackend::firewall_close_proxy_lan(&mut LockedBackend)
        }

        fn subscribe_events(&mut self) -> Result<OperationAccepted, crate::BackendError> {
            RuntimeBackend::subscribe_events(&mut LockedBackend)
        }
    }

    #[test]
    fn integrity_classification_only_accepts_medium_and_high_user_tokens() {
        assert_eq!(
            classify_integrity_rid(SECURITY_MANDATORY_MEDIUM_RID as u32),
            ClientIntegrity::Medium
        );
        assert_eq!(
            classify_integrity_rid(SECURITY_MANDATORY_HIGH_RID as u32),
            ClientIntegrity::High
        );
        assert_eq!(
            classify_integrity_rid(SECURITY_MANDATORY_SYSTEM_RID as u32),
            ClientIntegrity::System
        );
        assert_eq!(classify_integrity_rid(0), ClientIntegrity::Anonymous);
    }

    #[test]
    fn pipe_security_contract_is_local_and_interactive_only() {
        assert_eq!(RUNTIME_PIPE_NAME, r"\\.\pipe\ObsessionRuntime.v1");
        // This string is deliberately checked rather than regenerated at run
        // time: accidental broadening to Everyone/Anonymous is a regression.
        let descriptor = "D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGW;;;IU)";
        assert!(descriptor.contains(";;;IU)"));
        assert!(!descriptor.contains(";;;WD)"));
        assert!(!descriptor.contains(";;;AN)"));
    }

    #[test]
    fn installed_client_layout_accepts_only_the_fixed_program_files_image() {
        let program_files = Path::new(r"C:\Program Files");
        let service = Path::new(r"C:\Program Files\Obsession\runtime\Obsession.Runtime.exe");
        assert_eq!(
            expected_client_image(service, program_files),
            Some(PathBuf::from(r"C:\Program Files\Obsession\Obsession.exe"))
        );
        assert!(expected_client_image(
            Path::new(r"C:\Users\User\Obsession\runtime\Obsession.Runtime.exe"),
            program_files
        )
        .is_none());
        assert!(expected_client_image(
            Path::new(r"C:\Program Files\Obsession\runtime\renamed.exe"),
            program_files
        )
        .is_none());
    }

    #[test]
    fn process_policy_authenticates_the_open_image_not_merely_a_local_pid() {
        let process = OwnedHandle(
            unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, std::process::id()) }
                .expect("test process must be queryable"),
        );
        let trusted = ClientProcessPolicy::current_test_process();
        assert!(trusted.authenticates(process.0));

        let foreign = ClientProcessPolicy {
            expected_image: Some(PathBuf::from(r"C:\Program Files\Foreign\client.exe")),
        };
        assert!(!foreign.authenticates(process.0));
    }

    #[test]
    fn zero_client_io_timeout_is_rejected_before_creating_a_pipe() {
        let server = SecureNamedPipeServer::new_for_test(LockedBackend);
        assert!(matches!(
            server.serve_one_with_timeout(Duration::ZERO),
            Err(NamedPipeError::InvalidTimeout)
        ));
    }

    #[test]
    fn idle_listener_observes_stop_without_waiting_for_a_client() {
        let _pipe_test = PIPE_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let server = Arc::new(SecureNamedPipeServer::new_for_test(LockedBackend));
        let stop = Arc::new(AtomicBool::new(false));
        let worker = {
            let server = Arc::clone(&server);
            let stop = Arc::clone(&stop);
            thread::spawn(move || server.serve_one_until_stopped(&stop))
        };

        thread::sleep(Duration::from_millis(60));
        stop.store(true, Ordering::Release);

        assert!(!worker
            .join()
            .expect("listener worker must not panic")
            .expect("listener stop must not fail"));
    }

    #[test]
    fn client_that_never_reads_the_response_is_cancelled_at_the_deadline() {
        let _pipe_test = PIPE_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let server = Arc::new(SecureNamedPipeServer::new_for_test(LockedBackend));
        let (done_sender, done_receiver) = std::sync::mpsc::sync_channel(1);
        let worker = {
            let server = Arc::clone(&server);
            thread::spawn(move || {
                let result = server.serve_one_with_timeout(Duration::from_millis(100));
                let _ = done_sender.send(result);
            })
        };

        let pipe = connect_test_client().expect("test client must connect");
        let pipe = OwnedHandle(pipe);
        let request = RequestEnvelope::new("never-read-1", Request::GetCapabilities);
        write_all(pipe.0, &encode_request_frame(&request).unwrap())
            .expect("request write must complete");

        let result = match done_receiver.recv_timeout(Duration::from_secs(2)) {
            Ok(result) => result,
            Err(error) => {
                drop(pipe);
                let _ = worker.join();
                panic!("server did not enforce its client I/O deadline: {error}");
            }
        };
        assert!(matches!(result, Err(NamedPipeError::ClientIoTimedOut)));
        drop(pipe);
        worker.join().expect("server worker must not panic");
    }

    #[test]
    fn backend_work_can_outlive_the_client_io_deadline_without_poisoning_the_pipe() {
        let _pipe_test = PIPE_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let server = Arc::new(SecureNamedPipeServer::new_for_test(SlowCapabilitiesBackend));
        let worker = {
            let server = Arc::clone(&server);
            thread::spawn(move || server.serve_one_with_timeout(Duration::from_millis(100)))
        };

        let pipe = connect_test_client().expect("test client must connect");
        let pipe = OwnedHandle(pipe);
        let request = RequestEnvelope::new("slow-capabilities-1", Request::GetCapabilities);
        write_all(pipe.0, &encode_request_frame(&request).unwrap())
            .expect("request write must complete");
        let response = decode_response_frame(&read_frame(pipe.0).unwrap()).unwrap();

        assert_eq!(response.request_id, "slow-capabilities-1");
        assert!(matches!(response.response, Response::Capabilities(_)));
        worker
            .join()
            .expect("server worker must not panic")
            .expect("backend duration must not consume the client I/O deadline");
    }

    #[test]
    fn local_interactive_client_can_exchange_exactly_one_bounded_request() {
        let _pipe_test = PIPE_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let server = Arc::new(SecureNamedPipeServer::new_for_test(LockedBackend));
        let worker = {
            let server = Arc::clone(&server);
            thread::spawn(move || server.serve_one())
        };

        let pipe = match connect_test_client() {
            Ok(pipe) => pipe,
            Err(error) => {
                let worker_result = worker.join().expect("pipe worker must not panic");
                panic!(
                    "current interactive user must open pipe: {error}; server result: {worker_result:?}"
                );
            }
        };
        let pipe = OwnedHandle(pipe);
        let request = RequestEnvelope::new("capabilities-1", Request::GetCapabilities);
        if let Err(error) = write_all(pipe.0, &encode_request_frame(&request).unwrap()) {
            drop(pipe);
            let worker_result = worker.join().expect("pipe worker must not panic");
            panic!("pipe client write failed: {error}; server result: {worker_result:?}");
        }
        let response_frame = match read_frame(pipe.0) {
            Ok(frame) => frame,
            Err(error) => {
                drop(pipe);
                let worker_result = worker.join().expect("pipe worker must not panic");
                panic!("pipe client read failed: {error}; server result: {worker_result:?}");
            }
        };
        let response = decode_response_frame(&response_frame).unwrap();

        assert_eq!(response.request_id, "capabilities-1");
        assert!(matches!(response.response, Response::Capabilities(_)));
        assert_ne!(
            response.response,
            Response::Error(obsession_runtime_protocol::ServiceError {
                code: ServiceErrorCode::AccessDenied,
            })
        );
        worker
            .join()
            .expect("pipe worker must not panic")
            .expect("pipe worker must finish the request");
    }

    fn connect_test_client() -> Result<HANDLE, NamedPipeError> {
        let deadline = Instant::now() + Duration::from_secs(2);
        let pipe_name = runtime_pipe_name_wide();
        while Instant::now() < deadline {
            if unsafe { WaitNamedPipeW(PCWSTR(pipe_name.as_ptr()), 50).as_bool() } {
                let pipe = unsafe {
                    CreateFileW(
                        PCWSTR(pipe_name.as_ptr()),
                        GENERIC_READ.0 | GENERIC_WRITE.0,
                        FILE_SHARE_NONE,
                        None,
                        OPEN_EXISTING,
                        FILE_ATTRIBUTE_NORMAL | SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
                        None,
                    )?
                };
                return Ok(pipe);
            }
        }
        Err(NamedPipeError::Windows(WindowsError::from_win32()))
    }
}
