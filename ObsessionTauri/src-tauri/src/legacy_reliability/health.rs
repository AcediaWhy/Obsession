//! Cross-platform Eyes health and queue-gap aggregation.
//!
//! Packet producers update [`AtomicHealthCounters`] out of band from the
//! bounded flow queue. [`HealthTracker`] consumes snapshots on a logical clock,
//! so neither a full data queue nor wall-clock changes can hide lost evidence.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use super::contracts::{EyeHealthCounters, EyeHealthState};

/// A degraded sensor needs this much time without new errors or drops before
/// it may report Ready again.
pub const HEALTH_CLEAN_WINDOW_MS: u64 = 10_000;

/// Counters shared with the packet/capture ingress.
///
/// All values are cumulative for one sensor generation. A new Eyes generation
/// must create a new counter set and a new [`HealthTracker`].
#[derive(Debug, Default)]
pub struct AtomicHealthCounters {
    packet_count: AtomicU64,
    parse_errors: AtomicU64,
    queue_drops: AtomicU64,
    last_event_ts: AtomicU64,
    has_last_event_ts: AtomicBool,
    sensor_failed: AtomicBool,
    pending_queue_drops: AtomicU64,
    first_queue_drop_ts: AtomicU64,
    last_queue_drop_ts: AtomicU64,
    drop_window_revision: AtomicU64,
}

impl AtomicHealthCounters {
    /// Records one packet observed by capture.
    pub fn record_packet(&self, monotonic_ts: u64) {
        self.record_event_time(monotonic_ts);
        saturating_add(&self.packet_count, 1);
    }

    /// Records a packet/flow that capture could not parse.
    ///
    /// This does not increment `packet_count`; ingress records receipt and
    /// parsing independently, which avoids hidden double counting.
    pub fn record_parse_error(&self, monotonic_ts: u64) {
        self.record_event_time(monotonic_ts);
        saturating_add(&self.parse_errors, 1);
    }

    /// Records a terminal capture/tracker failure that cannot be inferred
    /// from queue counters alone. The manager will keep the sensor Blind for
    /// this generation until the session is replaced.
    pub fn record_sensor_failure(&self, monotonic_ts: u64) {
        self.record_event_time(monotonic_ts);
        self.sensor_failed.store(true, Ordering::Release);
    }

    /// Records one flow event rejected by the bounded data queue.
    pub fn record_queue_drop(&self, monotonic_ts: u64) {
        self.record_queue_drops(1, monotonic_ts);
    }

    /// Records several data-queue drops from an already aggregated producer.
    pub fn record_queue_drops(&self, count: u64, monotonic_ts: u64) {
        if count == 0 {
            return;
        }

        self.record_event_time(monotonic_ts);
        let revision = self.lock_drop_window();
        let pending = self.pending_queue_drops.load(Ordering::Relaxed);
        if pending == 0 {
            self.first_queue_drop_ts
                .store(monotonic_ts, Ordering::Relaxed);
            self.last_queue_drop_ts
                .store(monotonic_ts, Ordering::Relaxed);
        } else {
            self.first_queue_drop_ts
                .fetch_min(monotonic_ts, Ordering::Relaxed);
            self.last_queue_drop_ts
                .fetch_max(monotonic_ts, Ordering::Relaxed);
        }
        self.pending_queue_drops
            .store(pending.saturating_add(count), Ordering::Relaxed);
        let total = self.queue_drops.load(Ordering::Relaxed);
        self.queue_drops
            .store(total.saturating_add(count), Ordering::Relaxed);
        self.unlock_drop_window(revision);
    }

    /// Takes a non-destructive cumulative snapshot.
    ///
    /// In particular, this never clears queue drops. Only the consumer-side
    /// delivery acknowledgement may clear a tracker's pending Gap.
    pub fn snapshot(&self) -> HealthCounterSnapshot {
        let (queue_drops, drop_window) = self.snapshot_drop_window();
        let counters = EyeHealthCounters {
            packet_count: self.packet_count.load(Ordering::Acquire),
            parse_errors: self.parse_errors.load(Ordering::Acquire),
            queue_drops,
            last_event_ts: load_optional_timestamp(&self.last_event_ts, &self.has_last_event_ts),
        };

        HealthCounterSnapshot::new(
            counters,
            drop_window,
            self.sensor_failed.load(Ordering::Acquire),
        )
    }

    /// Clears an exact producer-side drop window after its Gap reached the
    /// control channel. A newer drop changes the token and makes this fail.
    pub fn acknowledge_drop_window(&self, token: DropWindowToken) -> bool {
        if self
            .drop_window_revision
            .compare_exchange(
                token.0,
                token.0.wrapping_add(1),
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_err()
        {
            return false;
        }

        let had_pending = self.pending_queue_drops.load(Ordering::Relaxed) != 0;
        if had_pending {
            self.pending_queue_drops.store(0, Ordering::Relaxed);
            self.first_queue_drop_ts.store(0, Ordering::Relaxed);
            self.last_queue_drop_ts.store(0, Ordering::Relaxed);
        }
        self.unlock_drop_window(token.0);
        had_pending
    }

    fn record_event_time(&self, monotonic_ts: u64) {
        self.last_event_ts
            .fetch_max(monotonic_ts, Ordering::Relaxed);
        self.has_last_event_ts.store(true, Ordering::Release);
    }

    fn snapshot_drop_window(&self) -> (u64, Option<DropWindowSnapshot>) {
        loop {
            let revision = self.drop_window_revision.load(Ordering::Acquire);
            if !revision.is_multiple_of(2) {
                std::hint::spin_loop();
                continue;
            }

            let queue_drops = self.queue_drops.load(Ordering::Relaxed);
            let dropped_events = self.pending_queue_drops.load(Ordering::Relaxed);
            let from_ts = self.first_queue_drop_ts.load(Ordering::Relaxed);
            let to_ts = self.last_queue_drop_ts.load(Ordering::Relaxed);
            if self.drop_window_revision.load(Ordering::Acquire) != revision {
                std::hint::spin_loop();
                continue;
            }

            let window = (dropped_events != 0).then_some(DropWindowSnapshot {
                from_ts,
                to_ts,
                dropped_events,
                token: DropWindowToken(revision),
            });
            return (queue_drops, window);
        }
    }

    fn lock_drop_window(&self) -> u64 {
        loop {
            let revision = self.drop_window_revision.load(Ordering::Acquire);
            if !revision.is_multiple_of(2) {
                std::hint::spin_loop();
                continue;
            }
            if self
                .drop_window_revision
                .compare_exchange_weak(
                    revision,
                    revision.wrapping_add(1),
                    Ordering::AcqRel,
                    Ordering::Acquire,
                )
                .is_ok()
            {
                return revision;
            }
        }
    }

    fn unlock_drop_window(&self, previous_revision: u64) {
        self.drop_window_revision
            .store(next_even_revision(previous_revision), Ordering::Release);
    }
}

fn saturating_add(counter: &AtomicU64, amount: u64) {
    let _ = counter.fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
        Some(current.saturating_add(amount))
    });
}

fn load_optional_timestamp(value: &AtomicU64, present: &AtomicBool) -> Option<u64> {
    present
        .load(Ordering::Acquire)
        .then(|| value.load(Ordering::Acquire))
}

/// One immutable read of producer-side counters.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HealthCounterSnapshot {
    pub counters: EyeHealthCounters,
    drop_window: Option<DropWindowSnapshot>,
    pub sensor_failed: bool,
}

impl HealthCounterSnapshot {
    pub const fn new(
        counters: EyeHealthCounters,
        drop_window: Option<DropWindowSnapshot>,
        sensor_failed: bool,
    ) -> Self {
        Self {
            counters,
            drop_window,
            sensor_failed,
        }
    }

    pub const fn drop_window(self) -> Option<DropWindowSnapshot> {
        self.drop_window
    }
}

/// Producer-side identity of an exact pending drop window.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DropWindowToken(u64);

/// Exact first/last timestamps retained outside the bounded flow queue.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DropWindowSnapshot {
    pub from_ts: u64,
    pub to_ts: u64,
    pub dropped_events: u64,
    token: DropWindowToken,
}

impl DropWindowSnapshot {
    pub const fn token(self) -> DropWindowToken {
        self.token
    }
}

/// Opaque delivery identity for a pending Gap.
///
/// It advances whenever the tracker observes additional drops. Consequently,
/// acknowledging an older report cannot clear drops discovered after it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct GapToken(u64);

/// Queue-loss metadata waiting for the control channel to accept it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PendingGap {
    pub from_ts: u64,
    pub to_ts: u64,
    pub dropped_events: u64,
    token: GapToken,
    drop_window_token: Option<DropWindowToken>,
}

impl PendingGap {
    pub const fn token(self) -> GapToken {
        self.token
    }

    /// Token to acknowledge on [`AtomicHealthCounters`] before acknowledging
    /// this report on [`HealthTracker`].
    pub const fn drop_window_token(self) -> Option<DropWindowToken> {
        self.drop_window_token
    }

    /// Whether this gap invalidates an inclusive evidence time window.
    pub const fn overlaps(self, window_from_ts: u64, window_to_ts: u64) -> bool {
        window_from_ts <= window_to_ts
            && self.from_ts <= self.to_ts
            && self.from_ts <= window_to_ts
            && window_from_ts <= self.to_ts
    }
}

/// Current health plus a Gap that must be retried until acknowledged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HealthPoll {
    pub state: EyeHealthState,
    pub counters: EyeHealthCounters,
    pub pending_gap: Option<PendingGap>,
}

/// Pure health state machine for one Eyes sensor generation.
#[derive(Debug)]
pub struct HealthTracker {
    state: EyeHealthState,
    last_counters: EyeHealthCounters,
    last_poll_ts: u64,
    last_unhealthy_growth_ts: Option<u64>,
    pending_gap: Option<PendingGap>,
    gap_sequence: u64,
    terminal_state: Option<EyeHealthState>,
}

impl HealthTracker {
    pub const fn new(started_at_ms: u64) -> Self {
        Self {
            state: EyeHealthState::Ready,
            last_counters: EyeHealthCounters {
                packet_count: 0,
                parse_errors: 0,
                queue_drops: 0,
                last_event_ts: None,
            },
            last_poll_ts: started_at_ms,
            last_unhealthy_growth_ts: None,
            pending_gap: None,
            gap_sequence: 0,
            terminal_state: None,
        }
    }

    pub const fn state(&self) -> EyeHealthState {
        self.state
    }

    /// Consumes a cumulative counter snapshot at the supplied logical time.
    pub fn poll(&mut self, now_ms: u64, snapshot: HealthCounterSnapshot) -> HealthPoll {
        self.ingest(now_ms, snapshot);
        self.current_poll()
    }

    /// Marks an unexpected receive/worker failure as terminally Blind.
    pub fn mark_unexpected_failure(
        &mut self,
        now_ms: u64,
        snapshot: HealthCounterSnapshot,
    ) -> HealthPoll {
        self.ingest(now_ms, snapshot);
        if self.terminal_state != Some(EyeHealthState::Stopped) {
            self.terminal_state = Some(EyeHealthState::Blind);
            self.state = EyeHealthState::Blind;
        }
        self.current_poll()
    }

    /// Marks intentional teardown as terminally Stopped.
    pub fn stop_intentionally(
        &mut self,
        now_ms: u64,
        snapshot: HealthCounterSnapshot,
    ) -> HealthPoll {
        self.ingest(now_ms, snapshot);
        self.terminal_state = Some(EyeHealthState::Stopped);
        self.state = EyeHealthState::Stopped;
        self.current_poll()
    }

    /// Confirms that the control channel accepted the exact pending Gap.
    ///
    /// For a producer-backed Gap, first call
    /// [`AtomicHealthCounters::acknowledge_drop_window`] with the report's
    /// `drop_window_token`. Only acknowledge this tracker token if that call
    /// succeeds.
    ///
    /// Returns false for duplicate, foreign, or stale tokens. A stale token is
    /// intentionally unable to clear a report that includes newer drops.
    pub fn acknowledge_gap(&mut self, token: GapToken) -> bool {
        let matches_current = self
            .pending_gap
            .is_some_and(|pending| pending.token == token);
        if matches_current {
            self.pending_gap = None;
        }
        matches_current
    }

    fn ingest(&mut self, now_ms: u64, snapshot: HealthCounterSnapshot) {
        let now_ms = now_ms.max(self.last_poll_ts);

        if snapshot.sensor_failed && self.terminal_state != Some(EyeHealthState::Stopped) {
            self.terminal_state = Some(EyeHealthState::Blind);
        }

        if counters_regressed(snapshot.counters, self.last_counters) {
            if self.terminal_state != Some(EyeHealthState::Stopped) {
                self.terminal_state = Some(EyeHealthState::Blind);
            }
        } else {
            let parse_errors_grew =
                snapshot.counters.parse_errors > self.last_counters.parse_errors;
            let queue_drops_grew = snapshot.counters.queue_drops > self.last_counters.queue_drops;

            if let Some(drop_window) = snapshot.drop_window {
                self.sync_exact_gap(drop_window);
            } else if queue_drops_grew && self.pending_gap.is_none() {
                let dropped = snapshot
                    .counters
                    .queue_drops
                    .saturating_sub(self.last_counters.queue_drops);
                self.replace_gap(self.last_poll_ts, now_ms, dropped, None);
            }

            if parse_errors_grew || queue_drops_grew {
                self.last_unhealthy_growth_ts = Some(now_ms);
            }
        }

        self.last_counters = snapshot.counters;
        self.last_poll_ts = now_ms;
        self.refresh_state(now_ms);
    }

    fn sync_exact_gap(&mut self, drop_window: DropWindowSnapshot) {
        let already_current = self.pending_gap.is_some_and(|pending| {
            pending.drop_window_token == Some(drop_window.token)
                && pending.from_ts == drop_window.from_ts
                && pending.to_ts == drop_window.to_ts
                && pending.dropped_events == drop_window.dropped_events
        });
        if !already_current {
            self.replace_gap(
                drop_window.from_ts,
                drop_window.to_ts,
                drop_window.dropped_events,
                Some(drop_window.token),
            );
        }
    }

    fn replace_gap(
        &mut self,
        from_ts: u64,
        to_ts: u64,
        dropped_events: u64,
        drop_window_token: Option<DropWindowToken>,
    ) {
        self.gap_sequence = next_nonzero(self.gap_sequence);
        self.pending_gap = Some(PendingGap {
            from_ts: from_ts.min(to_ts),
            to_ts: from_ts.max(to_ts),
            dropped_events,
            token: GapToken(self.gap_sequence),
            drop_window_token,
        });
    }

    fn refresh_state(&mut self, now_ms: u64) {
        if let Some(terminal) = self.terminal_state {
            self.state = terminal;
            return;
        }

        let clean_window_elapsed = self
            .last_unhealthy_growth_ts
            .is_none_or(|last_growth| now_ms.saturating_sub(last_growth) >= HEALTH_CLEAN_WINDOW_MS);
        self.state = if self.pending_gap.is_some() || !clean_window_elapsed {
            EyeHealthState::Degraded
        } else {
            EyeHealthState::Ready
        };
    }

    const fn current_poll(&self) -> HealthPoll {
        HealthPoll {
            state: self.state,
            counters: self.last_counters,
            pending_gap: self.pending_gap,
        }
    }
}

fn next_nonzero(current: u64) -> u64 {
    let next = current.wrapping_add(1);
    if next == 0 {
        1
    } else {
        next
    }
}

fn next_even_revision(current: u64) -> u64 {
    let next = current.wrapping_add(2);
    if next.is_multiple_of(2) {
        next
    } else {
        0
    }
}

fn counters_regressed(current: EyeHealthCounters, previous: EyeHealthCounters) -> bool {
    current.packet_count < previous.packet_count
        || current.parse_errors < previous.parse_errors
        || current.queue_drops < previous.queue_drops
        || option_regressed(current.last_event_ts, previous.last_event_ts)
}

fn option_regressed(current: Option<u64>, previous: Option<u64>) -> bool {
    match (current, previous) {
        (None, Some(_)) => true,
        (Some(current), Some(previous)) => current < previous,
        (None, None) | (Some(_), None) => false,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::thread;

    use super::*;

    #[test]
    fn packet_traffic_keeps_sensor_ready() {
        let counters = AtomicHealthCounters::default();
        let mut tracker = HealthTracker::new(0);

        counters.record_packet(15);
        let poll = tracker.poll(15, counters.snapshot());

        assert_eq!(poll.state, EyeHealthState::Ready);
        assert_eq!(poll.counters.packet_count, 1);
        assert_eq!(poll.counters.last_event_ts, Some(15));
        assert_eq!(poll.pending_gap, None);
    }

    #[test]
    fn parse_error_requires_a_full_clean_window() {
        let counters = AtomicHealthCounters::default();
        let mut tracker = HealthTracker::new(100);

        counters.record_parse_error(150);
        assert_eq!(
            tracker.poll(150, counters.snapshot()).state,
            EyeHealthState::Degraded
        );
        assert_eq!(
            tracker.poll(10_149, counters.snapshot()).state,
            EyeHealthState::Degraded
        );
        assert_eq!(
            tracker.poll(10_150, counters.snapshot()).state,
            EyeHealthState::Ready
        );
    }

    #[test]
    fn gap_is_retried_and_stale_ack_cannot_clear_new_drops() {
        let counters = AtomicHealthCounters::default();
        let mut tracker = HealthTracker::new(10);

        counters.record_queue_drop(20);
        let first = tracker.poll(25, counters.snapshot()).pending_gap.unwrap();
        assert_eq!(first.dropped_events, 1);

        // A failed control-channel send means no acknowledgement. The same Gap
        // remains visible on the next poll.
        assert_eq!(
            tracker.poll(30, counters.snapshot()).pending_gap,
            Some(first)
        );

        counters.record_queue_drops(2, 35);
        let expanded = tracker.poll(40, counters.snapshot()).pending_gap.unwrap();
        assert_eq!(expanded.dropped_events, 3);
        assert_eq!((expanded.from_ts, expanded.to_ts), (20, 35));
        assert_ne!(expanded.token(), first.token());
        assert!(!counters.acknowledge_drop_window(first.drop_window_token().unwrap()));
        assert!(!tracker.acknowledge_gap(first.token()));
        assert_eq!(
            tracker.poll(40, counters.snapshot()).pending_gap,
            Some(expanded)
        );

        assert!(counters.acknowledge_drop_window(expanded.drop_window_token().unwrap()));
        assert!(tracker.acknowledge_gap(expanded.token()));
        assert_eq!(tracker.poll(40, counters.snapshot()).pending_gap, None);
        assert_eq!(tracker.state(), EyeHealthState::Degraded);
        assert_eq!(
            tracker.poll(10_040, counters.snapshot()).state,
            EyeHealthState::Ready
        );
    }

    #[test]
    fn gap_only_overlaps_its_own_time_range() {
        let counters = AtomicHealthCounters::default();
        let mut tracker = HealthTracker::new(100);

        counters.record_queue_drop(150);
        let gap = tracker.poll(200, counters.snapshot()).pending_gap.unwrap();

        assert_eq!((gap.from_ts, gap.to_ts), (150, 150));
        assert!(!gap.overlaps(10, 149));
        assert!(gap.overlaps(150, 150));
        assert!(gap.overlaps(125, 175));
        assert!(!gap.overlaps(151, 250));
        assert!(!gap.overlaps(250, 201));
    }

    #[test]
    fn concurrent_queue_drops_are_not_lost() {
        const WORKERS: u64 = 4;
        const DROPS_PER_WORKER: u64 = 1_000;

        let counters = Arc::new(AtomicHealthCounters::default());
        let workers: Vec<_> = (0..WORKERS)
            .map(|worker| {
                let counters = Arc::clone(&counters);
                thread::spawn(move || {
                    for offset in 0..DROPS_PER_WORKER {
                        counters.record_queue_drop(worker * DROPS_PER_WORKER + offset);
                    }
                })
            })
            .collect();
        for worker in workers {
            worker.join().unwrap();
        }

        let snapshot = counters.snapshot();
        assert_eq!(snapshot.counters.queue_drops, WORKERS * DROPS_PER_WORKER);
        let mut tracker = HealthTracker::new(0);
        assert_eq!(
            tracker
                .poll(WORKERS * DROPS_PER_WORKER, snapshot)
                .pending_gap
                .unwrap()
                .dropped_events,
            WORKERS * DROPS_PER_WORKER
        );
    }

    #[test]
    fn unexpected_failure_is_blind_until_intentional_stop() {
        let counters = AtomicHealthCounters::default();
        let mut tracker = HealthTracker::new(0);

        assert_eq!(
            tracker
                .mark_unexpected_failure(10, counters.snapshot())
                .state,
            EyeHealthState::Blind
        );
        counters.record_packet(20);
        assert_eq!(
            tracker.poll(20, counters.snapshot()).state,
            EyeHealthState::Blind
        );
        assert_eq!(
            tracker.stop_intentionally(30, counters.snapshot()).state,
            EyeHealthState::Stopped
        );
        assert_eq!(
            tracker
                .mark_unexpected_failure(40, counters.snapshot())
                .state,
            EyeHealthState::Stopped
        );
    }

    #[test]
    fn producer_sensor_failure_is_terminal_for_the_generation() {
        let counters = AtomicHealthCounters::default();
        let mut tracker = HealthTracker::new(0);

        counters.record_sensor_failure(10);
        assert_eq!(
            tracker.poll(10, counters.snapshot()).state,
            EyeHealthState::Blind
        );
        counters.record_packet(20);
        assert_eq!(
            tracker.poll(20_000, counters.snapshot()).state,
            EyeHealthState::Blind
        );
    }

    #[test]
    fn counter_reset_without_new_generation_becomes_blind() {
        let mut tracker = HealthTracker::new(0);
        let first = HealthCounterSnapshot::new(
            EyeHealthCounters {
                packet_count: 5,
                parse_errors: 1,
                queue_drops: 0,
                last_event_ts: Some(10),
            },
            None,
            false,
        );
        tracker.poll(10, first);

        let reset = HealthCounterSnapshot::default();
        assert_eq!(tracker.poll(20, reset).state, EyeHealthState::Blind);
    }
}
