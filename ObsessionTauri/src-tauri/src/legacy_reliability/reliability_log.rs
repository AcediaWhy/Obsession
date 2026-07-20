//! Privacy-safe, bounded local diagnostics for Legacy reliability.
//!
//! The public record schema deliberately has no domain, IP address, raw
//! evidence, payload, URL, or free-form reason fields. The writer adds the
//! schema version and wall-clock timestamp itself, rotates at UTC day
//! boundaries, and only removes files that exactly match its own naming
//! convention.

use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Days, NaiveDate, Utc};
use serde::Serialize;

use super::contracts::{EventEnvelope, LaneGeneration};

const LOG_SCHEMA_VERSION: u8 = 1;
const RETENTION_CALENDAR_DAYS: u64 = 7;
const FILE_PREFIX: &str = "legacy-reliability-";
const FILE_SUFFIX: &str = ".jsonl";
const DATE_BYTES: usize = 10;

/// Maximum JSON payload size, excluding the terminating line-feed byte.
pub const MAX_SERIALIZED_LINE_BYTES: usize = 16 * 1024;
/// Maximum size of one UTC daily file, including terminating line feeds.
pub const MAX_DAILY_FILE_BYTES: u64 = 8 * 1024 * 1024;
/// Maximum number of records accepted into one UTC daily file.
pub const MAX_DAILY_RECORDS: u64 = 10_000;

/// The kind of state transition represented by a log record.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReliabilityEventKind {
    SessionStarted,
    SensorHealthChanged,
    AssessmentUpdated,
    EnvironmentGateCompleted,
    IntentProposed,
    SessionClosed,
}

/// Privacy-safe summary of the lane assessment; never contains raw evidence.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AssessmentKind {
    InsufficientEvidence,
    Healthy,
    ResetSuspected,
    ResetQuorum,
    BlackholeQuorum,
    SensorUnreliable,
}

/// Stable Phase 2 classification tags written to local diagnostics.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ClassificationKind {
    Offline,
    DnsFailure,
    UpstreamDegraded,
    TargetUnavailable,
    ServiceSlow,
    DpiSuspected,
    DpiBlocked,
    SensorUnreliable,
}

/// A proposed Brain action. Phase 2 records this tag but does not execute it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IntentKind {
    Wait,
    SwitchLane,
    FreezeLane,
    RetrySameConfig,
    Rollback,
}

/// Lane identity without config contents, domains, addresses, or fingerprints.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ReliabilityLane {
    pub category: String,
    pub lane_generation: LaneGeneration,
}

impl ReliabilityLane {
    pub fn new(category: impl Into<String>, lane_generation: LaneGeneration) -> Self {
        Self {
            category: category.into(),
            lane_generation,
        }
    }
}

/// Bounded aggregate of Environment Gate activity.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct GateSummary {
    pub attempted_controls: u8,
    pub successful_controls: u8,
    pub attempted_targets: u16,
    pub successful_targets: u16,
    pub duration_ms: u64,
}

/// Numeric evidence counters only; individual flows and targets are omitted.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct ReliabilityCounters {
    pub accepted_flows: u64,
    pub rejected_flows: u64,
    pub reset_events: u32,
    pub distinct_reset_targets: u16,
    pub blackhole_flows: u32,
    pub distinct_blackhole_targets: u16,
    pub gaps: u64,
    pub queue_drops: u64,
}

/// One typed, privacy-safe reliability observation.
///
/// `schema_version` and `timestamp_unix_ms` are intentionally not caller
/// supplied; [`ReliabilityLog::append`] adds both from its `now` argument.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ReliabilityRecord {
    pub event: ReliabilityEventKind,
    #[serde(flatten)]
    pub envelope: EventEnvelope,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lane: Option<ReliabilityLane>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assessment: Option<AssessmentKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub classification: Option<ClassificationKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intent: Option<IntentKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gate: Option<GateSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub counters: Option<ReliabilityCounters>,
}

impl ReliabilityRecord {
    pub const fn new(event: ReliabilityEventKind, envelope: EventEnvelope) -> Self {
        Self {
            event,
            envelope,
            lane: None,
            assessment: None,
            classification: None,
            intent: None,
            gate: None,
            counters: None,
        }
    }
}

#[derive(Serialize)]
struct TimestampedRecord<'a> {
    schema_version: u8,
    timestamp_unix_ms: i64,
    #[serde(flatten)]
    record: &'a ReliabilityRecord,
}

/// Append-only daily JSONL log with bounded daily growth and seven-day UTC
/// retention.
pub struct ReliabilityLog {
    root: PathBuf,
    active_date: NaiveDate,
    file: File,
    daily_bytes: u64,
    daily_records: u64,
}

impl ReliabilityLog {
    /// Opens today's log, creating `root` when necessary and pruning files
    /// older than the inclusive seven-day retention window.
    pub fn open(root: PathBuf, now: DateTime<Utc>) -> io::Result<Self> {
        fs::create_dir_all(&root)?;
        let active_date = now.date_naive();
        // Retention is housekeeping, not a policy dependency. A locked old
        // file on Windows must not disable today's diagnostics.
        let _ = cleanup_expired(&root, active_date);
        let daily = open_daily_file(&root, active_date)?;
        Ok(Self {
            root,
            active_date,
            file: daily.file,
            daily_bytes: daily.bytes,
            daily_records: daily.records,
        })
    }

    /// Appends one complete JSON object and flushes it before returning.
    ///
    /// A changed UTC date triggers rollover and retention cleanup. Records
    /// larger than [`MAX_SERIALIZED_LINE_BYTES`] or exceeding the daily byte
    /// or record budget are rejected without starting a new write.
    pub fn append(&mut self, now: DateTime<Utc>, record: &ReliabilityRecord) -> io::Result<()> {
        let mut line = serialize_line(now, record)?;
        let date = now.date_naive();
        if date != self.active_date {
            self.rollover(date)?;
        }

        line.push(b'\n');
        self.reserve_line(line.len())?;
        self.file.write_all(&line)?;
        self.file.flush()
    }

    fn reserve_line(&mut self, line_bytes: usize) -> io::Result<()> {
        // Preserve the high-water mark if a prior write partially failed, but
        // also notice growth by another handle before accepting more data.
        self.daily_bytes = self.daily_bytes.max(self.file.metadata()?.len());

        if self.daily_records >= MAX_DAILY_RECORDS {
            return Err(daily_limit_error(format!(
                "Legacy reliability daily log reached the {MAX_DAILY_RECORDS}-record limit"
            )));
        }

        let line_bytes = u64::try_from(line_bytes).unwrap_or(u64::MAX);
        if line_bytes > MAX_DAILY_FILE_BYTES.saturating_sub(self.daily_bytes) {
            return Err(daily_limit_error(format!(
                "Legacy reliability daily log reached the {MAX_DAILY_FILE_BYTES}-byte limit"
            )));
        }

        // Reserve before I/O. If write_all performs a partial write and then
        // fails, the logger stays conservative for the rest of this process.
        self.daily_bytes = self.daily_bytes.saturating_add(line_bytes);
        self.daily_records = self.daily_records.saturating_add(1);
        Ok(())
    }

    fn rollover(&mut self, date: NaiveDate) -> io::Result<()> {
        self.file.flush()?;
        let _ = cleanup_expired(&self.root, date);
        let next = open_daily_file(&self.root, date)?;
        self.file = next.file;
        self.daily_bytes = next.bytes;
        self.daily_records = next.records;
        self.active_date = date;
        Ok(())
    }
}

fn daily_limit_error(message: String) -> io::Error {
    io::Error::other(message)
}

fn serialize_line(now: DateTime<Utc>, record: &ReliabilityRecord) -> io::Result<Vec<u8>> {
    let mut line = LimitedLine::new();
    serde_json::to_writer(
        &mut line,
        &TimestampedRecord {
            schema_version: LOG_SCHEMA_VERSION,
            timestamp_unix_ms: now.timestamp_millis(),
            record,
        },
    )
    .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    Ok(line.into_inner())
}

/// Prevents an oversized caller-owned string from causing an equally large
/// temporary serialization allocation before the line limit is checked.
struct LimitedLine {
    bytes: Vec<u8>,
}

impl LimitedLine {
    fn new() -> Self {
        Self {
            bytes: Vec::with_capacity(1024),
        }
    }

    fn into_inner(self) -> Vec<u8> {
        self.bytes
    }
}

impl Write for LimitedLine {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let remaining = MAX_SERIALIZED_LINE_BYTES.saturating_sub(self.bytes.len());
        if bytes.len() > remaining {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "Legacy reliability record exceeds the {MAX_SERIALIZED_LINE_BYTES}-byte limit"
                ),
            ));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct DailyFile {
    file: File,
    bytes: u64,
    records: u64,
}

fn open_daily_file(root: &Path, date: NaiveDate) -> io::Result<DailyFile> {
    let path = root.join(file_name(date));
    let file = OpenOptions::new().create(true).append(true).open(&path)?;
    let bytes = file.metadata()?.len();
    let records = if bytes > MAX_DAILY_FILE_BYTES {
        // No scan is needed: the byte budget already prevents another write.
        MAX_DAILY_RECORDS
    } else {
        count_records(&path)?
    };
    Ok(DailyFile {
        file,
        bytes,
        records,
    })
}

fn count_records(path: &Path) -> io::Result<u64> {
    let mut reader = BufReader::new(File::open(path)?);
    let mut records = 0_u64;
    let mut last_byte = None;

    loop {
        let buffer = reader.fill_buf()?;
        if buffer.is_empty() {
            break;
        }
        records =
            records.saturating_add(buffer.iter().filter(|byte| **byte == b'\n').count() as u64);
        last_byte = buffer.last().copied();
        let consumed = buffer.len();
        reader.consume(consumed);
        if records >= MAX_DAILY_RECORDS {
            return Ok(MAX_DAILY_RECORDS);
        }
    }

    if last_byte.is_some_and(|byte| byte != b'\n') {
        records = records.saturating_add(1);
    }
    Ok(records.min(MAX_DAILY_RECORDS))
}

fn file_name(date: NaiveDate) -> String {
    format!("{FILE_PREFIX}{}{FILE_SUFFIX}", date.format("%Y-%m-%d"))
}

fn cleanup_expired(root: &Path, current_date: NaiveDate) -> io::Result<()> {
    let keep_from = current_date
        .checked_sub_days(Days::new(RETENTION_CALENDAR_DAYS - 1))
        .unwrap_or(NaiveDate::MIN);

    for entry in fs::read_dir(root)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let Some(date) = exact_log_date(&entry.file_name()) else {
            continue;
        };
        if date < keep_from {
            match fs::remove_file(entry.path()) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
    }
    Ok(())
}

fn exact_log_date(name: &std::ffi::OsStr) -> Option<NaiveDate> {
    let name = name.to_str()?;
    let expected_len = FILE_PREFIX.len() + DATE_BYTES + FILE_SUFFIX.len();
    if name.len() != expected_len {
        return None;
    }
    let date_text = name.strip_prefix(FILE_PREFIX)?.strip_suffix(FILE_SUFFIX)?;
    let date = NaiveDate::parse_from_str(date_text, "%Y-%m-%d").ok()?;
    (date.format("%Y-%m-%d").to_string() == date_text).then_some(date)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    use chrono::TimeZone;
    use serde_json::{json, Value};

    use super::*;
    use crate::legacy_reliability::contracts::{RegistryVersion, SensorGeneration, SessionId};

    static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

    struct TestDir(PathBuf);

    impl TestDir {
        fn new(label: &str) -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("test clock is after Unix epoch")
                .as_nanos();
            let sequence = NEXT_DIR.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "obsession-reliability-log-{label}-{}-{nonce}-{sequence}",
                std::process::id()
            ));
            fs::create_dir(&path).expect("create isolated test directory");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn at(day: u32, hour: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 7, day, hour, 2, 3)
            .single()
            .expect("valid fixture timestamp")
    }

    fn record(event: ReliabilityEventKind) -> ReliabilityRecord {
        let mut record = ReliabilityRecord::new(
            event,
            EventEnvelope::new(
                SessionId::new(41),
                SensorGeneration::new(3),
                RegistryVersion::new(7),
            ),
        );
        record.lane = Some(ReliabilityLane::new(
            "youtube_twitch",
            LaneGeneration::new(11),
        ));
        record.assessment = Some(AssessmentKind::ResetQuorum);
        record.classification = Some(ClassificationKind::DpiSuspected);
        record.intent = Some(IntentKind::SwitchLane);
        record.gate = Some(GateSummary {
            attempted_controls: 3,
            successful_controls: 2,
            attempted_targets: 2,
            successful_targets: 0,
            duration_ms: 831,
        });
        record.counters = Some(ReliabilityCounters {
            accepted_flows: 4,
            rejected_flows: 1,
            reset_events: 3,
            distinct_reset_targets: 2,
            ..ReliabilityCounters::default()
        });
        record
    }

    fn read_lines(path: &Path) -> Vec<String> {
        fs::read_to_string(path)
            .expect("read JSONL fixture")
            .lines()
            .map(str::to_owned)
            .collect()
    }

    #[test]
    fn appends_one_typed_json_object_per_line() {
        let root = TestDir::new("jsonl");
        let mut log = ReliabilityLog::open(root.path().to_path_buf(), at(20, 8)).unwrap();
        log.append(at(20, 9), &record(ReliabilityEventKind::AssessmentUpdated))
            .unwrap();
        log.append(at(20, 10), &record(ReliabilityEventKind::IntentProposed))
            .unwrap();

        let lines = read_lines(&root.path().join("legacy-reliability-2026-07-20.jsonl"));
        assert_eq!(lines.len(), 2);
        let first: Value = serde_json::from_str(&lines[0]).unwrap();
        let second: Value = serde_json::from_str(&lines[1]).unwrap();
        assert_eq!(first["schema_version"], json!(1));
        assert_eq!(
            first["timestamp_unix_ms"],
            json!(at(20, 9).timestamp_millis())
        );
        assert_eq!(first["event"], json!("assessment_updated"));
        assert_eq!(first["session_id"], json!(41));
        assert_eq!(first["sensor_generation"], json!(3));
        assert_eq!(first["target_registry_version"], json!(7));
        assert_eq!(first["lane"]["category"], json!("youtube_twitch"));
        assert_eq!(first["lane"]["lane_generation"], json!(11));
        assert_eq!(first["classification"], json!("dpi_suspected"));
        assert_eq!(first["gate"]["successful_controls"], json!(2));
        assert_eq!(second["event"], json!("intent_proposed"));
    }

    #[test]
    fn rolls_over_on_utc_date_change_and_cleans_again() {
        let root = TestDir::new("rollover");
        let expires_on_rollover = root.path().join("legacy-reliability-2026-07-14.jsonl");
        fs::write(&expires_on_rollover, "old\n").unwrap();

        let mut log = ReliabilityLog::open(root.path().to_path_buf(), at(20, 23)).unwrap();
        assert!(expires_on_rollover.exists());
        log.append(at(20, 23), &record(ReliabilityEventKind::AssessmentUpdated))
            .unwrap();
        log.append(
            at(21, 0),
            &record(ReliabilityEventKind::EnvironmentGateCompleted),
        )
        .unwrap();

        assert!(!expires_on_rollover.exists());
        let day_one = read_lines(&root.path().join("legacy-reliability-2026-07-20.jsonl"));
        let day_two = read_lines(&root.path().join("legacy-reliability-2026-07-21.jsonl"));
        assert_eq!(day_one.len(), 1);
        assert_eq!(day_two.len(), 1);
        assert!(day_one[0].contains("assessment_updated"));
        assert!(day_two[0].contains("environment_gate_completed"));
    }

    #[test]
    fn retention_keeps_exactly_seven_calendar_dates_and_unrelated_files() {
        let root = TestDir::new("retention");
        let expired = [
            "legacy-reliability-2026-07-01.jsonl",
            "legacy-reliability-2026-07-13.jsonl",
        ];
        let retained = [
            "legacy-reliability-2026-07-14.jsonl",
            "legacy-reliability-2026-07-19.jsonl",
            "legacy-reliability-2026-07-20.jsonl",
            "legacy-reliability-2026-07-21.jsonl",
        ];
        let unrelated = [
            "legacy-reliability-2026-7-13.jsonl",
            "legacy-reliability-2026-07-13.log",
            "legacy-reliability-2026-07-13.jsonl.bak",
            "legacy-reliability-2026-02-30.jsonl",
            "other.jsonl",
        ];
        for name in expired
            .iter()
            .chain(retained.iter())
            .chain(unrelated.iter())
        {
            fs::write(root.path().join(name), "fixture\n").unwrap();
        }
        let exact_directory = root.path().join("legacy-reliability-2026-07-12.jsonl");
        fs::create_dir(&exact_directory).unwrap();

        let _log = ReliabilityLog::open(root.path().to_path_buf(), at(20, 12)).unwrap();

        for name in expired {
            assert!(!root.path().join(name).exists(), "expired: {name}");
        }
        for name in retained.into_iter().chain(unrelated) {
            assert!(root.path().join(name).exists(), "retained: {name}");
        }
        assert!(exact_directory.is_dir());
    }

    #[test]
    fn schema_cannot_leak_raw_domain_ip_or_evidence_fixture() {
        let root = TestDir::new("privacy");
        let mut log = ReliabilityLog::open(root.path().to_path_buf(), at(20, 12)).unwrap();
        log.append(at(20, 12), &record(ReliabilityEventKind::AssessmentUpdated))
            .unwrap();

        let contents =
            fs::read_to_string(root.path().join("legacy-reliability-2026-07-20.jsonl")).unwrap();
        for forbidden in [
            "private.video.example",
            "203.0.113.91",
            "raw_client_hello_fixture",
            "destination_ip",
            "domain",
            "evidence",
        ] {
            assert!(!contents.contains(forbidden), "leaked {forbidden:?}");
        }
    }

    #[test]
    fn reopening_restores_complete_and_partial_record_usage() {
        let root = TestDir::new("restore-usage");
        let path = root.path().join("legacy-reliability-2026-07-20.jsonl");
        fs::write(&path, "{}\n{}\n{}").unwrap();

        let log = ReliabilityLog::open(root.path().to_path_buf(), at(20, 12)).unwrap();

        assert_eq!(log.daily_bytes, fs::metadata(path).unwrap().len());
        assert_eq!(log.daily_records, 3);
    }

    #[test]
    fn daily_record_limit_survives_restart_and_rejects_without_growth() {
        let root = TestDir::new("record-cap");
        let path = root.path().join("legacy-reliability-2026-07-20.jsonl");
        fs::write(&path, "{}\n".repeat(MAX_DAILY_RECORDS as usize)).unwrap();
        let size_at_limit = fs::metadata(&path).unwrap().len();
        let mut log = ReliabilityLog::open(root.path().to_path_buf(), at(20, 12)).unwrap();

        let error = log
            .append(at(20, 12), &record(ReliabilityEventKind::AssessmentUpdated))
            .unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::Other);
        assert!(error.to_string().contains("record limit"));
        assert_eq!(fs::metadata(path).unwrap().len(), size_at_limit);
    }

    #[test]
    fn daily_byte_limit_allows_exact_fit_then_rejects_without_growth() {
        let root = TestDir::new("byte-cap");
        let path = root.path().join("legacy-reliability-2026-07-20.jsonl");
        let fixture = record(ReliabilityEventKind::AssessmentUpdated);
        let line_bytes = serialize_line(at(20, 12), &fixture).unwrap().len() as u64 + 1;
        File::create(&path)
            .unwrap()
            .set_len(MAX_DAILY_FILE_BYTES - line_bytes)
            .unwrap();
        let mut log = ReliabilityLog::open(root.path().to_path_buf(), at(20, 12)).unwrap();

        log.append(at(20, 12), &fixture).unwrap();
        assert_eq!(fs::metadata(&path).unwrap().len(), MAX_DAILY_FILE_BYTES);

        let error = log.append(at(20, 13), &fixture).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Other);
        assert!(error.to_string().contains("byte limit"));
        assert_eq!(fs::metadata(path).unwrap().len(), MAX_DAILY_FILE_BYTES);
    }

    #[test]
    fn rejects_lines_larger_than_sixteen_kib_without_partial_write() {
        let root = TestDir::new("bounded");
        let mut log = ReliabilityLog::open(root.path().to_path_buf(), at(20, 12)).unwrap();
        let mut oversized = record(ReliabilityEventKind::AssessmentUpdated);
        oversized.lane = Some(ReliabilityLane::new(
            "x".repeat(MAX_SERIALIZED_LINE_BYTES),
            LaneGeneration::new(1),
        ));

        let error = log.append(at(20, 12), &oversized).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        let path = root.path().join("legacy-reliability-2026-07-20.jsonl");
        assert_eq!(fs::metadata(path).unwrap().len(), 0);
    }
}
