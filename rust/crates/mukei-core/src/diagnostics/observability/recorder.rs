//! Owned, thread-safe observability recorder and privacy-safe snapshots.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use parking_lot::{Mutex, RwLock};

use crate::diagnostics::redaction::{sanitize_stable_identifier, sanitize_telemetry_text};

use super::clock::{ObservabilityClock, SystemClock};
use super::event::OperationalEvent;
use super::health::{HealthPublishStatus, HealthRegistry, HealthSignal, HealthSummary};
use super::metrics::{
    DistributionSpec, MetricDimensions, MetricRecordStatus, MetricRegistry, MetricRegistrySnapshot,
};
use super::privacy::{EventScope, PrivacyState, TelemetryPolicy, TelemetryPrivacyMode};
use super::sink::{
    DiagnosticSink, SinkDispatcher, SinkEnvelope, SinkHealthState, SinkInstallError,
    SinkQueueLimits, SinkStatsSnapshot, DEFAULT_SINK_DISCONNECT_AFTER_DROPS,
    DEFAULT_SINK_QUEUE_BYTES, DEFAULT_SINK_SINGLE_ENVELOPE_BYTES,
    DEFAULT_SINK_SLOW_CALLBACK_THRESHOLD,
};
use super::slo::{
    SloDimensions, SloObservation, SloRecordStatus, SloRegistry, SloRegistrySnapshot,
};

pub const MAX_EVENT_BUFFER_CAPACITY: usize = 4_096;
pub const MAX_METRIC_SERIES_CAPACITY: usize = 4_096;
pub const MAX_HEALTH_SIGNAL_CAPACITY: usize = 512;
pub const MAX_SLO_SERIES_CAPACITY: usize = 1_024;
pub const MAX_SINK_QUEUE_CAPACITY: usize = 1_024;
pub const MAX_DIAGNOSTIC_SINKS: usize = 8;
pub const MAX_EVENT_BUFFER_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_SINGLE_EVENT_BYTES: usize = 256 * 1024;

#[derive(Clone, Debug, PartialEq)]
pub struct ProcessMetadata {
    pub session_started_at: DateTime<Utc>,
    pub app_version: Option<String>,
    pub platform_family: Option<String>,
    pub runtime_flavor: Option<String>,
    pub build_label: Option<String>,
}

impl ProcessMetadata {
    pub fn new() -> Self {
        Self {
            session_started_at: Utc::now(),
            app_version: sanitize_stable_identifier(env!("CARGO_PKG_VERSION"), 48),
            platform_family: sanitize_stable_identifier(std::env::consts::OS, 32),
            runtime_flavor: None,
            build_label: None,
        }
    }

    pub fn with_app_version(mut self, value: impl AsRef<str>) -> Self {
        self.app_version = sanitize_stable_identifier(value.as_ref(), 48);
        self
    }

    pub fn with_platform_family(mut self, value: impl AsRef<str>) -> Self {
        self.platform_family = sanitize_stable_identifier(value.as_ref(), 32);
        self
    }

    pub fn with_runtime_flavor(mut self, value: impl AsRef<str>) -> Self {
        self.runtime_flavor = sanitize_stable_identifier(value.as_ref(), 48);
        self
    }

    pub fn with_build_label(mut self, value: impl AsRef<str>) -> Self {
        self.build_label = Some(sanitize_telemetry_text(value.as_ref(), 64).into_string());
        self
    }
}

impl Default for ProcessMetadata {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug)]
pub struct ObservabilityConfig {
    pub policy: TelemetryPolicy,
    pub event_buffer_capacity: usize,
    pub max_metric_series: usize,
    pub max_health_signals: usize,
    pub max_slo_series: usize,
    pub interval: Duration,
    pub default_sink_queue_capacity: usize,
    pub process_metadata: ProcessMetadata,
}

impl Default for ObservabilityConfig {
    fn default() -> Self {
        Self {
            policy: TelemetryPolicy::essential(),
            event_buffer_capacity: 256,
            max_metric_series: 256,
            max_health_signals: 64,
            max_slo_series: 128,
            interval: Duration::from_secs(60),
            default_sink_queue_capacity: 128,
            process_metadata: ProcessMetadata::default(),
        }
    }
}

/// Byte budgets are separate from the compatibility configuration so existing
/// callers that construct `ObservabilityConfig` literals do not need to add
/// fields merely to obtain safe defaults.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ObservabilityMemoryLimits {
    pub event_buffer_bytes: usize,
    pub max_single_event_bytes: usize,
    pub critical_event_reserve_bytes: usize,
    pub default_sink_queue_bytes: usize,
    pub default_sink_single_envelope_bytes: usize,
    pub sink_disconnect_after_drops: u64,
    pub sink_slow_callback_threshold: Duration,
}

impl Default for ObservabilityMemoryLimits {
    fn default() -> Self {
        Self {
            event_buffer_bytes: 512 * 1024,
            max_single_event_bytes: 32 * 1024,
            critical_event_reserve_bytes: 64 * 1024,
            default_sink_queue_bytes: DEFAULT_SINK_QUEUE_BYTES,
            default_sink_single_envelope_bytes: DEFAULT_SINK_SINGLE_ENVELOPE_BYTES,
            sink_disconnect_after_drops: DEFAULT_SINK_DISCONNECT_AFTER_DROPS,
            sink_slow_callback_threshold: DEFAULT_SINK_SLOW_CALLBACK_THRESHOLD,
        }
    }
}

impl ObservabilityMemoryLimits {
    fn normalized(self) -> Self {
        let event_buffer_bytes = self.event_buffer_bytes.clamp(1, MAX_EVENT_BUFFER_BYTES);
        let max_single_event_bytes = self
            .max_single_event_bytes
            .clamp(1, MAX_SINGLE_EVENT_BYTES.min(event_buffer_bytes));
        let critical_event_reserve_bytes = self
            .critical_event_reserve_bytes
            .min(event_buffer_bytes / 2);
        Self {
            event_buffer_bytes,
            max_single_event_bytes,
            critical_event_reserve_bytes,
            default_sink_queue_bytes: self.default_sink_queue_bytes.max(1),
            default_sink_single_envelope_bytes: self.default_sink_single_envelope_bytes.max(1),
            sink_disconnect_after_drops: self.sink_disconnect_after_drops.max(1),
            sink_slow_callback_threshold: if self.sink_slow_callback_threshold.is_zero() {
                DEFAULT_SINK_SLOW_CALLBACK_THRESHOLD
            } else {
                self.sink_slow_callback_threshold
            },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventRecordStatus {
    Recorded,
    PolicyRejected,
    OversizedRejected,
    CapacityRejected,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RecorderStatsSnapshot {
    pub events_recorded: u64,
    pub events_dropped_policy: u64,
    pub event_attributes_dropped: u64,
    pub metric_observations_dropped_policy: u64,
    pub health_signals_dropped_policy: u64,
    pub slo_observations_dropped_policy: u64,
    pub event_buffer_evictions: u64,
    pub event_oversized_drops: u64,
    pub event_capacity_drops: u64,
    pub event_buffer_bytes: usize,
    pub metric_cardinality_overflow: u64,
    pub health_cardinality_overflow: u64,
    pub health_signals_coalesced: u64,
    pub slo_cardinality_overflow: u64,
    pub sink_queue_drops: u64,
    pub sink_callback_failures: u64,
    pub sink_callback_panics: u64,
    pub sink_disconnected_drops: u64,
    pub sink_oversized_drops: u64,
    pub sink_privacy_epoch_drops: u64,
    pub sink_coalesced: u64,
    pub sink_slow_callbacks: u64,
    pub sink_health: SinkHealthState,
    pub sink_queued_count: usize,
    pub sink_queued_bytes: usize,
}

#[derive(Clone, Debug)]
pub struct DiagnosticSnapshot {
    pub captured_at: DateTime<Utc>,
    pub policy: TelemetryPolicy,
    pub privacy_epoch: u64,
    pub process: ProcessMetadata,
    pub health: HealthSummary,
    pub recent_events: Vec<OperationalEvent>,
    pub metrics: MetricRegistrySnapshot,
    pub slo: SloRegistrySnapshot,
    pub recorder_stats: RecorderStatsSnapshot,
}

#[derive(Clone)]
pub struct ObservabilityRecorder {
    inner: Arc<RecorderInner>,
}

struct RecorderInner {
    privacy: Arc<PrivacyState>,
    policy_update_gate: RwLock<()>,
    events: EventRingBuffer,
    metrics: MetricRegistry,
    health: HealthRegistry,
    slo: SloRegistry,
    sinks: RwLock<Vec<SinkDispatcher>>,
    default_sink_queue_capacity: usize,
    memory_limits: ObservabilityMemoryLimits,
    process_metadata: ProcessMetadata,
    clock: Arc<dyn ObservabilityClock>,
    stats: RecorderStats,
}

impl ObservabilityRecorder {
    pub fn new(config: ObservabilityConfig) -> Self {
        Self::with_clock_and_limits(
            config,
            SystemClock::shared(),
            ObservabilityMemoryLimits::default(),
        )
    }

    pub fn with_clock(config: ObservabilityConfig, clock: Arc<dyn ObservabilityClock>) -> Self {
        Self::with_clock_and_limits(config, clock, ObservabilityMemoryLimits::default())
    }

    pub fn with_limits(config: ObservabilityConfig, limits: ObservabilityMemoryLimits) -> Self {
        Self::with_clock_and_limits(config, SystemClock::shared(), limits)
    }

    pub fn with_clock_and_limits(
        config: ObservabilityConfig,
        clock: Arc<dyn ObservabilityClock>,
        limits: ObservabilityMemoryLimits,
    ) -> Self {
        let event_buffer_capacity = config.event_buffer_capacity.min(MAX_EVENT_BUFFER_CAPACITY);
        let max_metric_series = config.max_metric_series.min(MAX_METRIC_SERIES_CAPACITY);
        let max_health_signals = config.max_health_signals.min(MAX_HEALTH_SIGNAL_CAPACITY);
        let max_slo_series = config.max_slo_series.min(MAX_SLO_SERIES_CAPACITY);
        let default_sink_queue_capacity = config
            .default_sink_queue_capacity
            .clamp(1, MAX_SINK_QUEUE_CAPACITY);
        let memory_limits = limits.normalized();
        let privacy = Arc::new(PrivacyState::new(config.policy));

        Self {
            inner: Arc::new(RecorderInner {
                privacy,
                policy_update_gate: RwLock::new(()),
                events: EventRingBuffer::new(
                    event_buffer_capacity,
                    memory_limits.event_buffer_bytes,
                    memory_limits.max_single_event_bytes,
                    memory_limits.critical_event_reserve_bytes,
                ),
                metrics: MetricRegistry::with_clock(
                    max_metric_series,
                    config.interval,
                    Arc::clone(&clock),
                ),
                health: HealthRegistry::with_clock(max_health_signals, Arc::clone(&clock)),
                slo: SloRegistry::with_clock(max_slo_series, config.interval, Arc::clone(&clock)),
                sinks: RwLock::new(Vec::new()),
                default_sink_queue_capacity,
                memory_limits,
                process_metadata: config.process_metadata,
                clock,
                stats: RecorderStats::default(),
            }),
        }
    }

    pub fn policy(&self) -> TelemetryPolicy {
        self.inner.privacy.snapshot().policy
    }

    pub fn privacy_epoch(&self) -> u64 {
        self.inner.privacy.snapshot().epoch
    }

    /// Update the in-memory policy. Every material change increments the
    /// privacy epoch before old queued sink work can be emitted.
    pub fn set_policy(&self, policy: TelemetryPolicy) {
        let _gate = self.inner.policy_update_gate.write();
        let (previous, current) = self.inner.privacy.update(policy);
        if previous == current {
            return;
        }

        match (previous.policy.mode(), current.policy.mode()) {
            (_, TelemetryPrivacyMode::Disabled) => {
                self.inner.events.clear();
                self.inner.metrics.clear();
                self.inner.health.clear();
                self.inner.slo.clear();
            }
            (TelemetryPrivacyMode::Extended, TelemetryPrivacyMode::Essential) => {
                self.inner.events.retain_scope(EventScope::Essential);
                // Registries do not retain scope per series; clearing on a
                // narrowing transition prevents extended observations from
                // surviving into the narrower local view.
                self.inner.metrics.clear();
                self.inner.health.clear();
                self.inner.slo.clear();
            }
            _ => {}
        }
    }

    pub fn add_sink(&self, sink: Arc<dyn DiagnosticSink>) -> Result<(), SinkInstallError> {
        self.add_sink_with_capacity(sink, self.inner.default_sink_queue_capacity)
    }

    pub fn add_sink_with_capacity(
        &self,
        sink: Arc<dyn DiagnosticSink>,
        queue_capacity: usize,
    ) -> Result<(), SinkInstallError> {
        let limits = SinkQueueLimits {
            max_count: queue_capacity.clamp(1, MAX_SINK_QUEUE_CAPACITY),
            max_bytes: self.inner.memory_limits.default_sink_queue_bytes,
            max_single_envelope_bytes: self.inner.memory_limits.default_sink_single_envelope_bytes,
            disconnect_after_drops: self.inner.memory_limits.sink_disconnect_after_drops,
            slow_callback_threshold: self.inner.memory_limits.sink_slow_callback_threshold,
        };
        self.add_sink_with_limits(sink, limits)
    }

    pub fn add_sink_with_limits(
        &self,
        sink: Arc<dyn DiagnosticSink>,
        mut limits: SinkQueueLimits,
    ) -> Result<(), SinkInstallError> {
        limits.max_count = limits.max_count.clamp(1, MAX_SINK_QUEUE_CAPACITY);
        let mut sinks = self.inner.sinks.write();
        if sinks.len() >= MAX_DIAGNOSTIC_SINKS {
            return Err(SinkInstallError::SinkLimitExceeded);
        }
        let dispatcher = SinkDispatcher::new(
            sink,
            limits,
            Arc::clone(&self.inner.privacy),
            Arc::clone(&self.inner.clock),
        )?;
        sinks.push(dispatcher);
        Ok(())
    }

    pub fn record_event(&self, event: OperationalEvent) -> EventRecordStatus {
        self.inner
            .stats
            .event_attributes_dropped
            .fetch_add(u64::from(event.dropped_attributes()), Ordering::Relaxed);

        let _gate = self.inner.policy_update_gate.read();
        let privacy = self.inner.privacy.snapshot();
        if !privacy.policy.allows_event(event.scope()) {
            self.inner
                .stats
                .events_dropped_policy
                .fetch_add(1, Ordering::Relaxed);
            return EventRecordStatus::PolicyRejected;
        }

        let event = Arc::new(event);
        match self.inner.events.push(Arc::clone(&event)) {
            EventBufferPush::Stored { evicted } => {
                if evicted > 0 {
                    self.inner
                        .stats
                        .event_buffer_evictions
                        .fetch_add(evicted, Ordering::Relaxed);
                }
            }
            EventBufferPush::Oversized => {
                self.inner
                    .stats
                    .event_oversized_drops
                    .fetch_add(1, Ordering::Relaxed);
                return EventRecordStatus::OversizedRejected;
            }
            EventBufferPush::CapacityRejected => {
                self.inner
                    .stats
                    .event_capacity_drops
                    .fetch_add(1, Ordering::Relaxed);
                return EventRecordStatus::CapacityRejected;
            }
        }

        self.inner
            .stats
            .events_recorded
            .fetch_add(1, Ordering::Relaxed);

        if privacy.policy.allows_export(event.scope()) {
            let envelope = SinkEnvelope::event(privacy.epoch, privacy.policy, event);
            for sink in self.inner.sinks.read().iter() {
                sink.try_send(Arc::clone(&envelope));
            }
        }
        EventRecordStatus::Recorded
    }

    pub fn increment_counter(
        &self,
        scope: EventScope,
        name: impl AsRef<str>,
        dimensions: MetricDimensions,
        delta: u64,
    ) -> MetricRecordStatus {
        let _gate = self.inner.policy_update_gate.read();
        if !self.inner.privacy.snapshot().policy.allows_event(scope) {
            self.inner
                .stats
                .metric_observations_dropped_policy
                .fetch_add(1, Ordering::Relaxed);
            return MetricRecordStatus::PolicyRejected;
        }
        self.inner
            .metrics
            .increment_counter(name, dimensions, delta)
    }

    pub fn set_gauge(
        &self,
        scope: EventScope,
        name: impl AsRef<str>,
        dimensions: MetricDimensions,
        value: f64,
    ) -> MetricRecordStatus {
        let _gate = self.inner.policy_update_gate.read();
        if !self.inner.privacy.snapshot().policy.allows_event(scope) {
            self.inner
                .stats
                .metric_observations_dropped_policy
                .fetch_add(1, Ordering::Relaxed);
            return MetricRecordStatus::PolicyRejected;
        }
        self.inner.metrics.set_gauge(name, dimensions, value)
    }

    pub fn observe_distribution(
        &self,
        scope: EventScope,
        name: impl AsRef<str>,
        dimensions: MetricDimensions,
        value: f64,
        spec: DistributionSpec,
    ) -> MetricRecordStatus {
        let _gate = self.inner.policy_update_gate.read();
        if !self.inner.privacy.snapshot().policy.allows_event(scope) {
            self.inner
                .stats
                .metric_observations_dropped_policy
                .fetch_add(1, Ordering::Relaxed);
            return MetricRecordStatus::PolicyRejected;
        }
        self.inner
            .metrics
            .observe_distribution(name, dimensions, value, spec)
    }

    pub fn publish_health(&self, scope: EventScope, signal: HealthSignal) -> HealthPublishStatus {
        let _gate = self.inner.policy_update_gate.read();
        if !self.inner.privacy.snapshot().policy.allows_event(scope) {
            self.inner
                .stats
                .health_signals_dropped_policy
                .fetch_add(1, Ordering::Relaxed);
            return HealthPublishStatus::PolicyRejected;
        }
        self.inner.health.publish(signal)
    }

    pub fn record_slo(
        &self,
        scope: EventScope,
        dimensions: SloDimensions,
        observation: SloObservation,
    ) -> SloRecordStatus {
        let _gate = self.inner.policy_update_gate.read();
        if !self.inner.privacy.snapshot().policy.allows_event(scope) {
            self.inner
                .stats
                .slo_observations_dropped_policy
                .fetch_add(1, Ordering::Relaxed);
            return SloRecordStatus::PolicyRejected;
        }
        self.inner.slo.record(dimensions, observation)
    }

    pub fn record_operation_slo(
        &self,
        scope: EventScope,
        dimensions: SloDimensions,
        outcome: super::event::OutcomeClass,
        latency: Option<Duration>,
    ) -> SloRecordStatus {
        let _gate = self.inner.policy_update_gate.read();
        if !self.inner.privacy.snapshot().policy.allows_event(scope) {
            self.inner
                .stats
                .slo_observations_dropped_policy
                .fetch_add(1, Ordering::Relaxed);
            return SloRecordStatus::PolicyRejected;
        }
        self.inner
            .slo
            .record_operation(dimensions, outcome, latency)
    }

    /// Enqueue one immutable metric snapshot shared by all installed sinks.
    /// Producer progress never waits on sink I/O.
    pub fn dispatch_metric_snapshot(&self) {
        let _gate = self.inner.policy_update_gate.read();
        let privacy = self.inner.privacy.snapshot();
        if !privacy.policy.export_allowed() {
            return;
        }
        let snapshot = Arc::new(self.inner.metrics.snapshot());
        let envelope = SinkEnvelope::metrics(privacy.epoch, privacy.policy, snapshot);
        for sink in self.inner.sinks.read().iter() {
            sink.try_send(Arc::clone(&envelope));
        }
    }

    pub fn snapshot(&self) -> DiagnosticSnapshot {
        let _gate = self.inner.policy_update_gate.read();
        let captured_at = self.inner.clock.wall_now();
        let privacy = self.inner.privacy.snapshot();
        let metrics = self.inner.metrics.snapshot_at(captured_at);
        let health = self.inner.health.snapshot_at(captured_at);
        let slo = self.inner.slo.snapshot_at(captured_at);
        let recent_events = self.inner.events.snapshot();
        let recorder_stats = self.stats_snapshot();

        DiagnosticSnapshot {
            captured_at,
            policy: privacy.policy,
            privacy_epoch: privacy.epoch,
            process: self.inner.process_metadata.clone(),
            health,
            recent_events,
            metrics,
            slo,
            recorder_stats,
        }
    }

    pub fn stats_snapshot(&self) -> RecorderStatsSnapshot {
        let sink_stats = self
            .inner
            .sinks
            .read()
            .iter()
            .map(SinkDispatcher::stats)
            .fold(SinkStatsSnapshot::default(), |mut total, current| {
                total.queue_dropped = total.queue_dropped.saturating_add(current.queue_dropped);
                total.callback_failures = total
                    .callback_failures
                    .saturating_add(current.callback_failures);
                total.callback_panics = total
                    .callback_panics
                    .saturating_add(current.callback_panics);
                total.disconnected_drops = total
                    .disconnected_drops
                    .saturating_add(current.disconnected_drops);
                total.oversized_drops = total
                    .oversized_drops
                    .saturating_add(current.oversized_drops);
                total.privacy_epoch_drops = total
                    .privacy_epoch_drops
                    .saturating_add(current.privacy_epoch_drops);
                total.coalesced = total.coalesced.saturating_add(current.coalesced);
                total.slow_callbacks = total.slow_callbacks.saturating_add(current.slow_callbacks);
                total.queued_count = total.queued_count.saturating_add(current.queued_count);
                total.queued_bytes = total.queued_bytes.saturating_add(current.queued_bytes);
                total.health = worst_sink_health(total.health, current.health);
                total
            });

        RecorderStatsSnapshot {
            events_recorded: self.inner.stats.events_recorded.load(Ordering::Relaxed),
            events_dropped_policy: self
                .inner
                .stats
                .events_dropped_policy
                .load(Ordering::Relaxed),
            event_attributes_dropped: self
                .inner
                .stats
                .event_attributes_dropped
                .load(Ordering::Relaxed),
            metric_observations_dropped_policy: self
                .inner
                .stats
                .metric_observations_dropped_policy
                .load(Ordering::Relaxed),
            health_signals_dropped_policy: self
                .inner
                .stats
                .health_signals_dropped_policy
                .load(Ordering::Relaxed),
            slo_observations_dropped_policy: self
                .inner
                .stats
                .slo_observations_dropped_policy
                .load(Ordering::Relaxed),
            event_buffer_evictions: self
                .inner
                .stats
                .event_buffer_evictions
                .load(Ordering::Relaxed),
            event_oversized_drops: self
                .inner
                .stats
                .event_oversized_drops
                .load(Ordering::Relaxed),
            event_capacity_drops: self
                .inner
                .stats
                .event_capacity_drops
                .load(Ordering::Relaxed),
            event_buffer_bytes: self.inner.events.bytes(),
            metric_cardinality_overflow: self.inner.metrics.cardinality_overflow_count(),
            health_cardinality_overflow: self.inner.health.cardinality_overflow_count(),
            health_signals_coalesced: self.inner.health.coalesced_count(),
            slo_cardinality_overflow: self.inner.slo.cardinality_overflow_count(),
            sink_queue_drops: sink_stats.queue_dropped,
            sink_callback_failures: sink_stats.callback_failures,
            sink_callback_panics: sink_stats.callback_panics,
            sink_disconnected_drops: sink_stats.disconnected_drops,
            sink_oversized_drops: sink_stats.oversized_drops,
            sink_privacy_epoch_drops: sink_stats.privacy_epoch_drops,
            sink_coalesced: sink_stats.coalesced,
            sink_slow_callbacks: sink_stats.slow_callbacks,
            sink_health: sink_stats.health,
            sink_queued_count: sink_stats.queued_count,
            sink_queued_bytes: sink_stats.queued_bytes,
        }
    }

    pub fn reset_interval_summaries(&self) {
        let _gate = self.inner.policy_update_gate.read();
        self.inner.metrics.reset_intervals();
        self.inner.slo.reset_intervals();
    }
}

impl Default for ObservabilityRecorder {
    fn default() -> Self {
        Self::new(ObservabilityConfig::default())
    }
}

fn worst_sink_health(left: SinkHealthState, right: SinkHealthState) -> SinkHealthState {
    match (left, right) {
        (SinkHealthState::Disconnected, _) | (_, SinkHealthState::Disconnected) => {
            SinkHealthState::Disconnected
        }
        (SinkHealthState::Degraded, _) | (_, SinkHealthState::Degraded) => {
            SinkHealthState::Degraded
        }
        _ => SinkHealthState::Healthy,
    }
}

struct EventRingBuffer {
    capacity: usize,
    max_bytes: usize,
    max_single_event_bytes: usize,
    critical_reserve_bytes: usize,
    state: Mutex<EventBufferState>,
}

struct EventBufferState {
    events: VecDeque<BufferedEvent>,
    bytes: usize,
    critical_bytes: usize,
}

struct BufferedEvent {
    event: Arc<OperationalEvent>,
    size: usize,
    critical: bool,
}

enum EventBufferPush {
    Stored { evicted: u64 },
    Oversized,
    CapacityRejected,
}

impl EventRingBuffer {
    fn new(
        capacity: usize,
        max_bytes: usize,
        max_single_event_bytes: usize,
        critical_reserve_bytes: usize,
    ) -> Self {
        Self {
            capacity,
            max_bytes,
            max_single_event_bytes,
            critical_reserve_bytes,
            state: Mutex::new(EventBufferState {
                events: VecDeque::with_capacity(capacity.min(256)),
                bytes: 0,
                critical_bytes: 0,
            }),
        }
    }

    fn push(&self, event: Arc<OperationalEvent>) -> EventBufferPush {
        if self.capacity == 0 {
            return EventBufferPush::CapacityRejected;
        }
        let size = event.estimated_size_bytes();
        if size > self.max_single_event_bytes || size > self.max_bytes {
            return EventBufferPush::Oversized;
        }
        let critical = event.is_critical();
        let mut state = self.state.lock();
        let mut evicted = 0u64;

        if !critical {
            let noncritical_budget = self.max_bytes.saturating_sub(self.critical_reserve_bytes);
            if size > noncritical_budget {
                return EventBufferPush::CapacityRejected;
            }
            while state
                .bytes
                .saturating_sub(state.critical_bytes)
                .saturating_add(size)
                > noncritical_budget
            {
                if !remove_oldest_matching(&mut state, |item| !item.critical) {
                    return EventBufferPush::CapacityRejected;
                }
                evicted = evicted.saturating_add(1);
            }
        }

        while state.events.len() >= self.capacity
            || state.bytes.saturating_add(size) > self.max_bytes
        {
            let removed = if critical {
                remove_oldest_matching(&mut state, |item| !item.critical)
                    || remove_oldest_matching(&mut state, |_| true)
            } else {
                remove_oldest_matching(&mut state, |item| !item.critical)
            };
            if !removed {
                return EventBufferPush::CapacityRejected;
            }
            evicted = evicted.saturating_add(1);
        }

        state.bytes = state.bytes.saturating_add(size);
        if critical {
            state.critical_bytes = state.critical_bytes.saturating_add(size);
        }
        state.events.push_back(BufferedEvent {
            event,
            size,
            critical,
        });
        EventBufferPush::Stored { evicted }
    }

    fn snapshot(&self) -> Vec<OperationalEvent> {
        self.state
            .lock()
            .events
            .iter()
            .map(|buffered| (*buffered.event).clone())
            .collect()
    }

    fn retain_scope(&self, scope: EventScope) {
        let mut state = self.state.lock();
        let mut retained = VecDeque::with_capacity(state.events.len());
        while let Some(item) = state.events.pop_front() {
            if item.event.scope() == scope {
                retained.push_back(item);
            }
        }
        state.events = retained;
        recalculate_event_bytes(&mut state);
    }

    fn clear(&self) {
        let mut state = self.state.lock();
        state.events.clear();
        state.bytes = 0;
        state.critical_bytes = 0;
    }

    fn bytes(&self) -> usize {
        self.state.lock().bytes
    }
}

fn remove_oldest_matching(
    state: &mut EventBufferState,
    predicate: impl Fn(&BufferedEvent) -> bool,
) -> bool {
    let Some(index) = state.events.iter().position(predicate) else {
        return false;
    };
    if let Some(item) = state.events.remove(index) {
        state.bytes = state.bytes.saturating_sub(item.size);
        if item.critical {
            state.critical_bytes = state.critical_bytes.saturating_sub(item.size);
        }
        true
    } else {
        false
    }
}

fn recalculate_event_bytes(state: &mut EventBufferState) {
    state.bytes = state
        .events
        .iter()
        .map(|item| item.size)
        .fold(0usize, usize::saturating_add);
    state.critical_bytes = state
        .events
        .iter()
        .filter(|item| item.critical)
        .map(|item| item.size)
        .fold(0usize, usize::saturating_add);
}

#[derive(Default)]
struct RecorderStats {
    events_recorded: AtomicU64,
    events_dropped_policy: AtomicU64,
    event_attributes_dropped: AtomicU64,
    metric_observations_dropped_policy: AtomicU64,
    health_signals_dropped_policy: AtomicU64,
    slo_observations_dropped_policy: AtomicU64,
    event_buffer_evictions: AtomicU64,
    event_oversized_drops: AtomicU64,
    event_capacity_drops: AtomicU64,
}
