//! Failure-isolated, byte-bounded dispatch to pluggable diagnostic sinks.

use std::collections::VecDeque;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::{Condvar, Mutex};

use super::clock::{monotonic_elapsed, ObservabilityClock};
use super::event::OperationalEvent;
use super::metrics::MetricRegistrySnapshot;
use super::privacy::{EventScope, PrivacyState, TelemetryPolicy, TelemetryPrivacyMode};

pub const DEFAULT_SINK_QUEUE_BYTES: usize = 1024 * 1024;
pub const MAX_SINK_QUEUE_BYTES: usize = 16 * 1024 * 1024;
pub const DEFAULT_SINK_SINGLE_ENVELOPE_BYTES: usize = 512 * 1024;
pub const MAX_SINK_SINGLE_ENVELOPE_BYTES: usize = 4 * 1024 * 1024;
pub const DEFAULT_SINK_DISCONNECT_AFTER_DROPS: u64 = 64;
pub const DEFAULT_SINK_SLOW_CALLBACK_THRESHOLD: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SinkError;

/// A sink receives data that has already passed structured sanitization.
/// Implementations must not assume they may access raw prompts, paths,
/// secrets or arbitrary user-controlled labels.
pub trait DiagnosticSink: Send + Sync + 'static {
    fn emit_event(
        &self,
        policy: TelemetryPolicy,
        event: &OperationalEvent,
    ) -> Result<(), SinkError>;

    fn emit_metrics(
        &self,
        _policy: TelemetryPolicy,
        _snapshot: &MetricRegistrySnapshot,
    ) -> Result<(), SinkError> {
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SinkHealthState {
    #[default]
    Healthy,
    Degraded,
    Disconnected,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SinkStatsSnapshot {
    pub queue_dropped: u64,
    pub callback_failures: u64,
    pub callback_panics: u64,
    pub disconnected_drops: u64,
    pub oversized_drops: u64,
    pub privacy_epoch_drops: u64,
    pub coalesced: u64,
    pub slow_callbacks: u64,
    pub queued_count: usize,
    pub queued_bytes: usize,
    pub health: SinkHealthState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SinkInstallError {
    WorkerSpawnFailed,
    SinkLimitExceeded,
    InvalidLimits,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SinkQueueLimits {
    pub max_count: usize,
    pub max_bytes: usize,
    pub max_single_envelope_bytes: usize,
    pub disconnect_after_drops: u64,
    pub slow_callback_threshold: Duration,
}

impl SinkQueueLimits {
    pub fn normalized(self) -> Result<Self, SinkInstallError> {
        if self.max_count == 0 || self.max_bytes == 0 || self.max_single_envelope_bytes == 0 {
            return Err(SinkInstallError::InvalidLimits);
        }
        Ok(Self {
            max_count: self.max_count,
            max_bytes: self.max_bytes.min(MAX_SINK_QUEUE_BYTES),
            max_single_envelope_bytes: self
                .max_single_envelope_bytes
                .min(MAX_SINK_SINGLE_ENVELOPE_BYTES)
                .min(self.max_bytes.min(MAX_SINK_QUEUE_BYTES)),
            disconnect_after_drops: self.disconnect_after_drops.max(1),
            slow_callback_threshold: if self.slow_callback_threshold.is_zero() {
                DEFAULT_SINK_SLOW_CALLBACK_THRESHOLD
            } else {
                self.slow_callback_threshold
            },
        })
    }
}

pub(crate) struct SinkDispatcher {
    queue: Arc<SinkQueue>,
    stats: Arc<SinkStats>,
}

impl SinkDispatcher {
    pub(crate) fn new(
        sink: Arc<dyn DiagnosticSink>,
        limits: SinkQueueLimits,
        privacy: Arc<PrivacyState>,
        clock: Arc<dyn ObservabilityClock>,
    ) -> Result<Self, SinkInstallError> {
        let limits = limits.normalized()?;
        let queue = Arc::new(SinkQueue::new(limits));
        let stats = Arc::new(SinkStats::default());
        let worker_queue = Arc::clone(&queue);
        let worker_stats = Arc::clone(&stats);

        std::thread::Builder::new()
            .name("mukei-diagnostics-sink".to_string())
            .spawn(move || {
                while let Some(envelope) = worker_queue.pop_blocking() {
                    if !privacy.permits_export(envelope.privacy_epoch, envelope.scope) {
                        worker_stats
                            .privacy_epoch_drops
                            .fetch_add(1, Ordering::Relaxed);
                        continue;
                    }

                    let started = clock.monotonic_now();
                    let result = catch_unwind(AssertUnwindSafe(|| match &envelope.payload {
                        SinkPayload::Event(event) => sink.emit_event(envelope.policy, event),
                        SinkPayload::Metrics(snapshot) => {
                            sink.emit_metrics(envelope.policy, snapshot)
                        }
                    }));
                    let elapsed = monotonic_elapsed(clock.monotonic_now(), started);
                    if elapsed >= limits.slow_callback_threshold {
                        worker_stats.slow_callbacks.fetch_add(1, Ordering::Relaxed);
                        worker_stats.mark_degraded();
                    }

                    match result {
                        Ok(Ok(())) => {}
                        Ok(Err(_)) => {
                            worker_stats
                                .callback_failures
                                .fetch_add(1, Ordering::Relaxed);
                            worker_stats.mark_degraded();
                        }
                        Err(_) => {
                            worker_stats.callback_panics.fetch_add(1, Ordering::Relaxed);
                            worker_stats.mark_degraded();
                        }
                    }
                    // Deliberately no recursive tracing/recording on sink failure.
                }
            })
            .map_err(|_| SinkInstallError::WorkerSpawnFailed)?;

        Ok(Self { queue, stats })
    }

    pub(crate) fn try_send(&self, envelope: Arc<SinkEnvelope>) {
        match self.queue.try_push(envelope, &self.stats) {
            QueuePushResult::Accepted | QueuePushResult::Coalesced => {}
            QueuePushResult::Full => {
                self.stats.queue_dropped.fetch_add(1, Ordering::Relaxed);
                self.stats.mark_degraded();
            }
            QueuePushResult::Oversized => {
                self.stats.oversized_drops.fetch_add(1, Ordering::Relaxed);
                self.stats.mark_degraded();
            }
            QueuePushResult::Disconnected => {
                self.stats
                    .disconnected_drops
                    .fetch_add(1, Ordering::Relaxed);
                self.stats.mark_disconnected();
            }
        }
    }

    pub(crate) fn stats(&self) -> SinkStatsSnapshot {
        let (queued_count, queued_bytes, disconnected) = self.queue.pressure();
        let mut snapshot = self.stats.snapshot();
        snapshot.queued_count = queued_count;
        snapshot.queued_bytes = queued_bytes;
        if disconnected {
            snapshot.health = SinkHealthState::Disconnected;
        }
        snapshot
    }
}

impl Drop for SinkDispatcher {
    fn drop(&mut self) {
        self.queue.close();
    }
}

pub(crate) struct SinkEnvelope {
    privacy_epoch: u64,
    policy: TelemetryPolicy,
    scope: EventScope,
    approx_bytes: usize,
    payload: SinkPayload,
}

impl SinkEnvelope {
    pub(crate) fn event(
        privacy_epoch: u64,
        policy: TelemetryPolicy,
        event: Arc<OperationalEvent>,
    ) -> Arc<Self> {
        Arc::new(Self {
            privacy_epoch,
            policy,
            scope: event.scope(),
            approx_bytes: event.estimated_size_bytes().saturating_add(96),
            payload: SinkPayload::Event(event),
        })
    }

    pub(crate) fn metrics(
        privacy_epoch: u64,
        policy: TelemetryPolicy,
        snapshot: Arc<MetricRegistrySnapshot>,
    ) -> Arc<Self> {
        let scope = if policy.mode() == TelemetryPrivacyMode::Extended {
            EventScope::Extended
        } else {
            EventScope::Essential
        };
        Arc::new(Self {
            privacy_epoch,
            policy,
            scope,
            approx_bytes: snapshot.estimated_size_bytes().saturating_add(96),
            payload: SinkPayload::Metrics(snapshot),
        })
    }

    fn is_replaceable_metrics(&self) -> bool {
        matches!(&self.payload, SinkPayload::Metrics(_))
    }
}

enum SinkPayload {
    Event(Arc<OperationalEvent>),
    Metrics(Arc<MetricRegistrySnapshot>),
}

struct SinkQueue {
    limits: SinkQueueLimits,
    state: Mutex<SinkQueueState>,
    ready: Condvar,
}

struct SinkQueueState {
    items: VecDeque<Arc<SinkEnvelope>>,
    bytes: usize,
    consecutive_drops: u64,
    disconnected: bool,
    closed: bool,
}

impl SinkQueue {
    fn new(limits: SinkQueueLimits) -> Self {
        Self {
            limits,
            state: Mutex::new(SinkQueueState {
                items: VecDeque::with_capacity(limits.max_count.min(256)),
                bytes: 0,
                consecutive_drops: 0,
                disconnected: false,
                closed: false,
            }),
            ready: Condvar::new(),
        }
    }

    fn try_push(&self, envelope: Arc<SinkEnvelope>, stats: &SinkStats) -> QueuePushResult {
        if envelope.approx_bytes > self.limits.max_single_envelope_bytes
            || envelope.approx_bytes > self.limits.max_bytes
        {
            return QueuePushResult::Oversized;
        }

        let mut state = self.state.lock();
        if state.disconnected || state.closed {
            return QueuePushResult::Disconnected;
        }

        // Metric snapshots are replaceable point-in-time views. Keep only the
        // newest pending one instead of charging queue growth to a slow sink.
        if envelope.is_replaceable_metrics() {
            if let Some(index) = state
                .items
                .iter()
                .rposition(|queued| queued.is_replaceable_metrics())
            {
                if let Some(previous) = state.items.remove(index) {
                    state.bytes = state.bytes.saturating_sub(previous.approx_bytes);
                }
                if state.bytes.saturating_add(envelope.approx_bytes) <= self.limits.max_bytes {
                    state.bytes = state.bytes.saturating_add(envelope.approx_bytes);
                    state.items.push_back(envelope);
                    state.consecutive_drops = 0;
                    stats.coalesced.fetch_add(1, Ordering::Relaxed);
                    self.ready.notify_one();
                    return QueuePushResult::Coalesced;
                }
            }
        }

        let count_full = state.items.len() >= self.limits.max_count;
        let bytes_full = state.bytes.saturating_add(envelope.approx_bytes) > self.limits.max_bytes;
        if count_full || bytes_full {
            state.consecutive_drops = state.consecutive_drops.saturating_add(1);
            if state.consecutive_drops >= self.limits.disconnect_after_drops {
                state.disconnected = true;
                stats.mark_disconnected();
            }
            return QueuePushResult::Full;
        }

        state.bytes = state.bytes.saturating_add(envelope.approx_bytes);
        state.items.push_back(envelope);
        state.consecutive_drops = 0;
        self.ready.notify_one();
        QueuePushResult::Accepted
    }

    fn pop_blocking(&self) -> Option<Arc<SinkEnvelope>> {
        let mut state = self.state.lock();
        loop {
            if let Some(envelope) = state.items.pop_front() {
                state.bytes = state.bytes.saturating_sub(envelope.approx_bytes);
                return Some(envelope);
            }
            if state.closed {
                return None;
            }
            self.ready.wait(&mut state);
        }
    }

    fn pressure(&self) -> (usize, usize, bool) {
        let state = self.state.lock();
        (state.items.len(), state.bytes, state.disconnected)
    }

    fn close(&self) {
        let mut state = self.state.lock();
        state.closed = true;
        self.ready.notify_all();
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum QueuePushResult {
    Accepted,
    Coalesced,
    Full,
    Oversized,
    Disconnected,
}

#[derive(Default)]
struct SinkStats {
    queue_dropped: AtomicU64,
    callback_failures: AtomicU64,
    callback_panics: AtomicU64,
    disconnected_drops: AtomicU64,
    oversized_drops: AtomicU64,
    privacy_epoch_drops: AtomicU64,
    coalesced: AtomicU64,
    slow_callbacks: AtomicU64,
    health: AtomicU8,
}

impl SinkStats {
    fn mark_degraded(&self) {
        let _ = self
            .health
            .compare_exchange(0, 1, Ordering::Relaxed, Ordering::Relaxed);
    }

    fn mark_disconnected(&self) {
        self.health.store(2, Ordering::Relaxed);
    }

    fn snapshot(&self) -> SinkStatsSnapshot {
        SinkStatsSnapshot {
            queue_dropped: self.queue_dropped.load(Ordering::Relaxed),
            callback_failures: self.callback_failures.load(Ordering::Relaxed),
            callback_panics: self.callback_panics.load(Ordering::Relaxed),
            disconnected_drops: self.disconnected_drops.load(Ordering::Relaxed),
            oversized_drops: self.oversized_drops.load(Ordering::Relaxed),
            privacy_epoch_drops: self.privacy_epoch_drops.load(Ordering::Relaxed),
            coalesced: self.coalesced.load(Ordering::Relaxed),
            slow_callbacks: self.slow_callbacks.load(Ordering::Relaxed),
            queued_count: 0,
            queued_bytes: 0,
            health: match self.health.load(Ordering::Relaxed) {
                0 => SinkHealthState::Healthy,
                1 => SinkHealthState::Degraded,
                _ => SinkHealthState::Disconnected,
            },
        }
    }
}
