//! Continuously drained, bounded stdout/stderr for a diagnostic DPI child.
//! Only the output and NUL input handles are inherited by that exact child.
//! Packet hexdumps and unrelated output are discarded before retention.

use std::mem::{size_of, size_of_val};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use obsession_runtime_protocol::DpiProcessDiagnostics;
use windows::core::w;
use windows::Win32::Foundation::{SetHandleInformation, HANDLE, HANDLE_FLAGS, HANDLE_FLAG_INHERIT};
use windows::Win32::Security::SECURITY_ATTRIBUTES;
use windows::Win32::Storage::FileSystem::{
    CreateFileW, ReadFile, FILE_ATTRIBUTE_NORMAL, FILE_GENERIC_READ, FILE_SHARE_READ,
    FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::Win32::System::Pipes::{CreatePipe, PeekNamedPipe};
use windows::Win32::System::Threading::{
    DeleteProcThreadAttributeList, InitializeProcThreadAttributeList, UpdateProcThreadAttribute,
    LPPROC_THREAD_ATTRIBUTE_LIST, PROC_THREAD_ATTRIBUTE_HANDLE_LIST, STARTF_USESTDHANDLES,
    STARTUPINFOEXW,
};

use crate::dpi_executor::{OwnedHandle, ProcessError};

const MAX_LINE: usize = 512;
const MAX_LINES: usize = 128;
// At most one diagnostic Zapret2 child exists per merged generation. This
// bound also leaves room for JSON escaping inside the 64 KiB IPC frame.
const MAX_RETAINED_BYTES: usize = 16 * 1024;
// Leave most of the bounded IPC budget for exact connection receipts. The
// legacy observer can emit many records for unrelated background traffic.
const MAX_GENERAL_LINES: usize = 32;
const MAX_GENERAL_BYTES: usize = 4 * 1024;

fn priority_record(line: &str) -> bool {
    line.starts_with("OBS_DIAG|flow_") || !line.starts_with("OBS_DIAG|")
}

#[derive(Default)]
struct OutputBuffer {
    lines: Vec<String>,
    pending: Vec<u8>,
    oversized_line: bool,
    retained_bytes: usize,
    observed_bytes: u64,
    discarded_lines: u64,
    truncated: bool,
    stream_closed: bool,
}

impl OutputBuffer {
    fn ingest(&mut self, bytes: &[u8]) {
        self.observed_bytes = self.observed_bytes.saturating_add(bytes.len() as u64);
        for &byte in bytes {
            if byte == b'\n' {
                if self.oversized_line {
                    self.discarded_lines += 1;
                    self.truncated = true;
                } else {
                    let line = std::mem::take(&mut self.pending);
                    self.retain(&line);
                }
                self.pending.clear();
                self.oversized_line = false;
            } else if self.pending.len() < MAX_LINE {
                self.pending.push(byte);
            } else {
                self.oversized_line = true;
            }
        }
    }

    fn retain(&mut self, bytes: &[u8]) {
        let mut line = String::from_utf8_lossy(bytes).trim().to_string();
        if line.len() > MAX_LINE {
            let mut boundary = MAX_LINE;
            while !line.is_char_boundary(boundary) {
                boundary -= 1;
            }
            line.truncate(boundary);
            self.truncated = true;
        }
        let lower = line.to_ascii_lowercase();
        let diagnostic = line.starts_with("OBS_DIAG|");
        let error = [
            "error",
            "warning",
            "lua error",
            "rawsend",
            "sendto",
            "windivert",
        ]
        .iter()
        .any(|prefix| lower.starts_with(prefix))
            || lower.contains("lua error:");
        // Reject control sequences as well as unstructured packet contents.
        if (!diagnostic && !error) || line.chars().any(|c| c.is_control()) {
            self.discarded_lines += 1;
            return;
        }
        if line == "OBS_DIAG|limit" {
            self.truncated = true;
        }
        let priority = priority_record(&line);
        if !priority {
            let general = self.lines.iter().filter(|value| !priority_record(value));
            let (count, bytes) = general.fold((0, 0), |(count, bytes), value| {
                (count + 1, bytes + value.len())
            });
            if count >= MAX_GENERAL_LINES || bytes + line.len() > MAX_GENERAL_BYTES {
                self.discarded_lines += 1;
                self.truncated = true;
                return;
            }
        }
        while self.lines.len() >= MAX_LINES || self.retained_bytes + line.len() > MAX_RETAINED_BYTES
        {
            if !priority {
                self.discarded_lines += 1;
                self.truncated = true;
                return;
            }
            // Keep recent flow receipts, preferring to evict legacy records.
            // Truncation remains explicit: retained positives cannot establish
            // absence of an earlier event or justify negative inference.
            let index = self
                .lines
                .iter()
                .position(|value| !priority_record(value))
                .unwrap_or(0);
            self.retained_bytes -= self.lines.remove(index).len();
            self.discarded_lines += 1;
            self.truncated = true;
        }
        self.retained_bytes += line.len();
        self.lines.push(line);
    }

    fn close(&mut self) {
        if !self.pending.is_empty() && !self.oversized_line {
            let tail = std::mem::take(&mut self.pending);
            self.retain(&tail);
        }
        self.pending.clear();
        self.stream_closed = true;
    }

    fn snapshot(&self, pid: u32, exit_code: Option<u32>) -> DpiProcessDiagnostics {
        DpiProcessDiagnostics {
            pid,
            exit_code,
            trace_enabled: true,
            stream_closed: self.stream_closed,
            observed_bytes: self.observed_bytes,
            discarded_lines: self.discarded_lines,
            truncated: self.truncated,
            lines: self.lines.clone(),
        }
    }
}

pub(crate) struct OutputCapture {
    state: Arc<Mutex<OutputBuffer>>,
    stopping: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl OutputCapture {
    fn start(read: OwnedHandle) -> Result<Self, ProcessError> {
        let state = Arc::new(Mutex::new(OutputBuffer::default()));
        let stopping = Arc::new(AtomicBool::new(false));
        let state_thread = state.clone();
        let stop_thread = stopping.clone();
        let worker = thread::Builder::new()
            .name("dpi-diagnostic-output".into())
            .spawn(move || {
                let mut buffer = [0u8; 8192];
                loop {
                    let mut available = 0;
                    if unsafe {
                        PeekNamedPipe(read.raw(), None, 0, None, Some(&mut available), None)
                    }
                    .is_err()
                    {
                        break;
                    }
                    if available > 0 {
                        let count = buffer.len().min(available as usize);
                        let mut read_bytes = 0;
                        if unsafe {
                            ReadFile(
                                read.raw(),
                                Some(&mut buffer[..count]),
                                Some(&mut read_bytes),
                                None,
                            )
                        }
                        .is_err()
                        {
                            break;
                        }
                        if read_bytes == 0 {
                            break;
                        }
                        if let Ok(mut state) = state_thread.lock() {
                            state.ingest(&buffer[..read_bytes as usize]);
                        }
                    } else {
                        if stop_thread.load(Ordering::Acquire) {
                            break;
                        }
                        thread::sleep(Duration::from_millis(5));
                    }
                    // Drop must remain bounded even if a malformed child keeps
                    // writing. The Job Object owns termination independently.
                    if stop_thread.load(Ordering::Acquire) {
                        break;
                    }
                }
                if let Ok(mut state) = state_thread.lock() {
                    state.close();
                }
            })
            .map_err(|error| {
                ProcessError::Windows(windows::core::Error::new(
                    windows::Win32::Foundation::E_FAIL,
                    format!("DPI output reader: {error}"),
                ))
            })?;
        Ok(Self {
            state,
            stopping,
            worker: Some(worker),
        })
    }

    pub(crate) fn snapshot(&self, pid: u32, exit_code: Option<u32>) -> DpiProcessDiagnostics {
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .snapshot(pid, exit_code)
    }

    /// Called only after the exact child has exited. Give its closed pipe a
    /// bounded chance to drain before generation cleanup removes the evidence.
    pub(crate) fn completed_snapshot(&self, pid: u32, exit_code: u32) -> DpiProcessDiagnostics {
        let deadline = std::time::Instant::now() + Duration::from_millis(150);
        loop {
            let snapshot = self.snapshot(pid, Some(exit_code));
            if snapshot.stream_closed || std::time::Instant::now() >= deadline {
                return snapshot;
            }
            thread::sleep(Duration::from_millis(5));
        }
    }
}

impl Drop for OutputCapture {
    fn drop(&mut self) {
        self.stopping.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

pub(crate) struct DiagnosticStartup {
    pub(crate) startup: STARTUPINFOEXW,
    // The backing allocation outlives the initialized attribute list.
    _attributes: Vec<usize>,
    // UpdateProcThreadAttribute retains the value pointer through creation.
    // Heap allocation keeps the handle array stable when this struct moves.
    _inherited: Box<[HANDLE; 2]>,
    _write: OwnedHandle,
    _stdin: OwnedHandle,
    pub(crate) capture: Option<OutputCapture>,
}

impl DiagnosticStartup {
    pub(crate) fn new() -> Result<Self, ProcessError> {
        let security = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            bInheritHandle: true.into(),
            ..Default::default()
        };
        let mut read = HANDLE::default();
        let mut write = HANDLE::default();
        unsafe {
            CreatePipe(&mut read, &mut write, Some(&security), 64 * 1024)?;
        }
        let read = OwnedHandle::new(read)?;
        let write = OwnedHandle::new(write)?;
        unsafe {
            SetHandleInformation(read.raw(), HANDLE_FLAG_INHERIT.0, HANDLE_FLAGS(0))?;
        }
        let stdin = OwnedHandle::new(unsafe {
            CreateFileW(
                w!("NUL"),
                FILE_GENERIC_READ.0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                Some(&security),
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                None,
            )?
        })?;
        let inherited = Box::new([write.raw(), stdin.raw()]);
        let mut required = 0;
        let _ = unsafe {
            InitializeProcThreadAttributeList(
                LPPROC_THREAD_ATTRIBUTE_LIST::default(),
                1,
                0,
                &mut required,
            )
        };
        if required == 0 {
            return Err(ProcessError::Windows(windows::core::Error::from_win32()));
        }
        let mut attributes = vec![0usize; required.div_ceil(size_of::<usize>())];
        let pointer = LPPROC_THREAD_ATTRIBUTE_LIST(attributes.as_mut_ptr().cast());
        unsafe {
            InitializeProcThreadAttributeList(pointer, 1, 0, &mut required)?;
        }
        let mut startup = STARTUPINFOEXW::default();
        startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
        startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
        startup.StartupInfo.hStdOutput = write.raw();
        startup.StartupInfo.hStdError = write.raw();
        startup.StartupInfo.hStdInput = stdin.raw();
        startup.lpAttributeList = pointer;
        let mut result = Self {
            startup,
            _attributes: attributes,
            _inherited: inherited,
            _write: write,
            _stdin: stdin,
            capture: None,
        };
        unsafe {
            UpdateProcThreadAttribute(
                pointer,
                0,
                PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
                Some(result._inherited.as_ptr().cast()),
                size_of_val(result._inherited.as_ref()),
                None,
                None,
            )?;
        }
        result.capture = Some(OutputCapture::start(read)?);
        Ok(result)
    }
}

impl Drop for DiagnosticStartup {
    fn drop(&mut self) {
        unsafe {
            DeleteProcThreadAttributeList(self.startup.lpAttributeList);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Storage::FileSystem::WriteFile;

    #[test]
    fn packet_contents_are_discarded_and_partial_lines_are_joined() {
        let mut buffer = OutputBuffer::default();
        buffer.ingest(b"16 03 01 00 a5 packet contents\nOBS_D");
        buffer.ingest(b"IAG|ready|v1\nLua error: missing function\n");
        assert_eq!(
            buffer.lines,
            ["OBS_DIAG|ready|v1", "Lua error: missing function"]
        );
        assert_eq!(buffer.discarded_lines, 1);
    }

    #[test]
    fn oversized_and_excess_output_stays_bounded() {
        let mut buffer = OutputBuffer::default();
        buffer.ingest(&vec![b'x'; 1024 * 1024]);
        buffer.ingest(b"\n");
        for _ in 0..10000 {
            buffer.ingest(b"OBS_DIAG|send|discord|fake|rawsend|ok\n");
        }
        assert!(buffer.pending.len() <= MAX_LINE);
        assert!(buffer.lines.len() <= MAX_LINES);
        assert!(buffer.retained_bytes <= MAX_RETAINED_BYTES);
        assert!(buffer.truncated);
        let response = obsession_runtime_protocol::ResponseEnvelope {
            protocol_version: obsession_runtime_protocol::PROTOCOL_VERSION,
            request_id: "trace".into(),
            response: obsession_runtime_protocol::Response::DpiDiagnostics(
                obsession_runtime_protocol::DpiDiagnostics {
                    packet_capture: None,
                    generation: 1,
                    processes: vec![buffer.snapshot(1, None)],
                },
            ),
        };
        assert!(
            obsession_runtime_protocol::encode_response_frame(&response)
                .unwrap()
                .len()
                < obsession_runtime_protocol::MAX_FRAME_BYTES
        );
    }

    #[test]
    fn background_observer_cannot_crowd_out_later_owned_flow_receipts() {
        let mut buffer = OutputBuffer::default();
        for _ in 0..10000 {
            buffer.ingest(b"OBS_DIAG|send|unrelated|fake|rawsend|ok\n");
        }
        buffer.ingest(b"OBS_DIAG|flow_packet|owned-connection\n");
        buffer.ingest(b"LUA ERROR: exact current program failed\n");
        assert_eq!(buffer.lines.len(), MAX_GENERAL_LINES + 2);
        assert!(buffer
            .lines
            .iter()
            .any(|line| line == "OBS_DIAG|flow_packet|owned-connection"));
        assert!(buffer
            .lines
            .iter()
            .any(|line| line == "LUA ERROR: exact current program failed"));
        assert!(buffer.truncated);
    }

    #[test]
    fn overflowing_flow_receipts_preserve_recent_records_and_report_loss() {
        let mut buffer = OutputBuffer::default();
        for serial in 0..MAX_LINES * 3 {
            buffer.ingest(format!("OBS_DIAG|flow_packet|{serial}\n").as_bytes());
        }
        assert_eq!(buffer.lines.len(), MAX_LINES);
        assert_eq!(buffer.lines.first().unwrap(), "OBS_DIAG|flow_packet|256");
        assert_eq!(buffer.lines.last().unwrap(), "OBS_DIAG|flow_packet|383");
        assert_eq!(buffer.discarded_lines, (MAX_LINES * 2) as u64);
        assert!(buffer.truncated);
        assert!(buffer.retained_bytes <= MAX_RETAINED_BYTES);
        assert_eq!(
            buffer.retained_bytes,
            buffer.lines.iter().map(String::len).sum::<usize>()
        );
    }

    #[test]
    fn pipe_is_drained_past_retention_limit_and_shutdown_is_bounded() {
        let startup = DiagnosticStartup::new().unwrap();
        let block = b"OBS_DIAG|send|discord|fake|rawsend|ok\n".repeat(256);
        for _ in 0..128 {
            let mut written = 0;
            unsafe {
                WriteFile(startup._write.raw(), Some(&block), Some(&mut written), None).unwrap();
            }
            assert_eq!(written as usize, block.len());
        }
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        while startup
            .capture
            .as_ref()
            .unwrap()
            .snapshot(1, None)
            .observed_bytes
            < (block.len() * 128) as u64
        {
            assert!(std::time::Instant::now() < deadline);
            thread::sleep(Duration::from_millis(5));
        }
        assert!(
            startup
                .capture
                .as_ref()
                .unwrap()
                .snapshot(1, None)
                .truncated
        );
        let started = std::time::Instant::now();
        drop(startup);
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn completed_startup_copies_the_final_error_before_capture_cleanup() {
        let mut startup = DiagnosticStartup::new().unwrap();
        let message = b"OBS_DIAG|ready|v1\nLUA ERROR: startup fixture without newline";
        unsafe {
            WriteFile(startup._write.raw(), Some(message), None, None).unwrap();
        }
        let capture = startup.capture.take().unwrap();
        drop(startup); // closes the last writer, like an exited child
        let snapshot = capture.completed_snapshot(42, 87);
        drop(capture);
        assert!(snapshot.stream_closed);
        assert_eq!(snapshot.exit_code, Some(87));
        assert_eq!(
            snapshot.lines,
            [
                "OBS_DIAG|ready|v1",
                "LUA ERROR: startup fixture without newline"
            ]
        );
    }
}
