//! Bounded SLO-oriented local aggregation primitives.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use parking_lot::RwLock;

use crate::diagnostics::redaction::sanitize_stable_identifier;

use super::clock::{monotonic_elapsed, ObservabilityClock, SystemClock};
use super::context::{MAX_COMPONENT_NAME_LEN, MAX_CONTEXT_DIMENSION_LEN};
use super::event::OutcomeClass;
use super::metrics::{DistributionSnapshot, DistributionSpec, DistributionState};

pub const MIN_SLO_OPERATION_SAMPLES: u64 = 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SloSampleState {
    InsufficientData,
    Sufficient,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SloIndicatorKind {
    OperationSuccessRatio,
    ErrorRatio,
    LatencyDistribution,
    CancellationRatio,
    RetryExhaustion,
    RecoveryOccurrence,
    QueueBackpressure,
    StaleStateDuration,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct SloDimensions {
    component: Option<String>,
    operation_kind: Option<String>,
    backend_kind: Option<String>,
    feature_area: Option<String>,
}

impl SloDimensions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_component(mut self, value: impl AsRef<str>) -> Self {
        self.component = sanitize_stable_identifier(value.as_ref(), MAX_COMPONENT_NAME_LEN);
        self
    }

    pub fn with_operation_kind(mut self, value: impl AsRef<str>) -> Self {
        self.operation_kind = stable_dimension(value.as_ref());
        self
    }

    pub fn with_backend_kind(mut self, value: impl AsRef<str>) -> Self {
        self.backend_kind = stable_dimension(value.as_ref());
        self
    }

    pub fn with_feature_area(mut self, value: impl AsRef<str>) -> Self {
        self.feature_area = stable_dimension(value.as_ref());
        self
    }

    pub fn component(&self) -> Option<&str> {
        self.component.as_deref()
    }

    pub fn operation_kind(&self) -> Option<&str> {
        self.operation_kind.as_deref()
    }

    pub fn backend_kind(&self) -> Option<&str> {
        self.backend_kind.as_deref()
    }

    pub fn feature_area(&self) -> Option<&str> {
        self.feature_area.as_deref()
    }
}

#[derive(Clone, Debug)]
pub enum SloObservation {
    OperationOutcome(OutcomeClass),
    Latency(Duration),
    RetryExhausted,
    RecoveryOccurred,
    QueuePressure(f64),
    StaleState(Duration),
}

#[derive(Clone, Debug, PartialEq)]
pub struct SloSummarySnapshot {
    pub total_operations: u64,
    pub successful_operations: u64,
    pub error_operations: u64,
    pub cancellations: u64,
    pub retry_exhaustions: u64,
    pub recoveries: u64,
    pub queue_pressure_observations: u64,
    pub queue_pressure_last: Option<f64>,
    pub queue_pressure_max: Option<f64>,
    pub latency: DistributionSnapshot,
    pub stale_state_duration: DistributionSnapshot,
}

impl SloSummarySnapshot {
    pub fn sample_state(&self) -> SloSampleState {
        if self.total_operations < MIN_SLO_OPERATION_SAMPLES {
            SloSampleState::InsufficientData
        } else {
            SloSampleState::Sufficient
        }
    }

    pub fn success_ratio(&self) -> Option<f64> {
        self.sufficient_ratio(self.successful_operations)
    }

    pub fn error_ratio(&self) -> Option<f64> {
        self.sufficient_ratio(self.error_operations)
    }

    pub fn cancellation_ratio(&self) -> Option<f64> {
        self.sufficient_ratio(self.cancellations)
    }

    pub fn raw_success_ratio(&self) -> Option<f64> {
        ratio(self.successful_operations, self.total_operations)
    }

    fn sufficient_ratio(&self, numerator: u64) -> Option<f64> {
        if self.sample_state() == SloSampleState::InsufficientData {
            None
        } else {
            ratio(numerator, self.total_operations)
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SloSeriesSnapshot {
    pub dimensions: SloDimensions,
    pub current: SloSummarySnapshot,
    pub previous: SloSummarySnapshot,
    pub lifetime: SloSummarySnapshot,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SloRegistrySnapshot {
    pub captured_at: DateTime<Utc>,
    pub interval: Duration,
    pub series: Vec<SloSeriesSnapshot>,
    pub cardinality_overflow_count: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SloRecordStatus {
    Recorded,
    SeriesLimitExceeded,
    InvalidValue,
    PolicyRejected,
}

#[derive(Clone)]
pub struct SloRegistry {
    inner: Arc<SloRegistryInner>,
}

struct SloRegistryInner {
    state: RwLock<HashMap<SloDimensions, WindowedSloSummary>>,
    max_series: usize,
    interval: Duration,
    clock: Arc<dyn ObservabilityClock>,
    cardinality_overflow_count: AtomicU64,
}

impl SloRegistry {
    pub fn new(max_series: usize, interval: Duration) -> Self {
        Self::with_clock(max_series, interval, SystemClock::shared())
    }

    pub fn with_clock(
        max_series: usize,
        interval: Duration,
        clock: Arc<dyn ObservabilityClock>,
    ) -> Self {
        Self {
            inner: Arc::new(SloRegistryInner {
                state: RwLock::new(HashMap::new()),
                max_series,
                interval: normalized_interval(interval),
                clock,
                cardinality_overflow_count: AtomicU64::new(0),
            }),
        }
    }

    pub fn record(
        &self,
        dimensions: SloDimensions,
        observation: SloObservation,
    ) -> SloRecordStatus {
        if !observation_is_valid(&observation) {
            return SloRecordStatus::InvalidValue;
        }

        let now = self.inner.clock.monotonic_now();
        let mut state = self.inner.state.write();
        if let Some(summary) = state.get_mut(&dimensions) {
            summary.rotate(now.clone(), self.inner.interval);
            summary.apply(observation);
            return SloRecordStatus::Recorded;
        }

        if state.len() >= self.inner.max_series {
            self.inner
                .cardinality_overflow_count
                .fetch_add(1, Ordering::Relaxed);
            return SloRecordStatus::SeriesLimitExceeded;
        }

        let mut summary = WindowedSloSummary::new(now);
        summary.apply(observation);
        state.insert(dimensions, summary);
        SloRecordStatus::Recorded
    }

    pub fn record_operation(
        &self,
        dimensions: SloDimensions,
        outcome: OutcomeClass,
        latency: Option<Duration>,
    ) -> SloRecordStatus {
        let now = self.inner.clock.monotonic_now();
        let mut state = self.inner.state.write();

        if !state.contains_key(&dimensions) && state.len() >= self.inner.max_series {
            self.inner
                .cardinality_overflow_count
                .fetch_add(1, Ordering::Relaxed);
            return SloRecordStatus::SeriesLimitExceeded;
        }

        let summary = state
            .entry(dimensions)
            .or_insert_with(|| WindowedSloSummary::new(now.clone()));
        summary.rotate(now.clone(), self.inner.interval);
        summary.apply(SloObservation::OperationOutcome(outcome));
        if let Some(latency) = latency {
            summary.apply(SloObservation::Latency(latency));
        }
        SloRecordStatus::Recorded
    }

    pub fn snapshot(&self) -> SloRegistrySnapshot {
        self.snapshot_at(self.inner.clock.wall_now())
    }

    pub fn snapshot_at(&self, now: DateTime<Utc>) -> SloRegistrySnapshot {
        let monotonic_now = self.inner.clock.monotonic_now();
        let mut state = self.inner.state.write();
        for summary in state.values_mut() {
            summary.rotate(monotonic_now, self.inner.interval);
        }
        let mut series = state
            .iter()
            .map(|(dimensions, summary)| SloSeriesSnapshot {
                dimensions: dimensions.clone(),
                current: summary.current.snapshot(),
                previous: summary.previous.snapshot(),
                lifetime: summary.lifetime.snapshot(),
            })
            .collect::<Vec<_>>();
        series.sort_by(|left, right| format!("{:?}", left.dimensions).cmp(&format!("{:?}", right.dimensions)));

        SloRegistrySnapshot {
            captured_at: now.clone(),
            interval: self.inner.interval,
            series,
            cardinality_overflow_count: self
                .inner
                .cardinality_overflow_count
                .load(Ordering::Relaxed),
        }
    }

    pub fn reset_intervals(&self) {
        let now = self.inner.clock.monotonic_now();
        let mut state = self.inner.state.write();
        for summary in state.values_mut() {
            summary.current = SloSummaryState::new();
            summary.previous = SloSummaryState::new();
            summary.current_started_at = now;
        }
    }

    pub fn clear(&self) {
        self.inner.state.write().clear();
        self.inner
            .cardinality_overflow_count
            .store(0, Ordering::Relaxed);
    }

    pub fn cardinality_overflow_count(&self) -> u64 {
        self.inner
            .cardinality_overflow_count
            .load(Ordering::Relaxed)
    }
}

impl Default for SloRegistry {
    fn default() -> Self {
        Self::new(128, Duration::from_secs(60))
    }
}

struct WindowedSloSummary {
    current_started_at: Duration,
    current: SloSummaryState,
    previous: SloSummaryState,
    lifetime: SloSummaryState,
}

impl WindowedSloSummary {
    fn new(now: Duration) -> Self {
        Self {
            current_started_at: now,
            current: SloSummaryState::new(),
            previous: SloSummaryState::new(),
            lifetime: SloSummaryState::new(),
        }
    }

    fn apply(&mut self, observation: SloObservation) {
        self.current.apply(&observation);
        self.lifetime.apply(&observation);
    }

    fn rotate(&mut self, now: Duration, interval: Duration) {
        let elapsed = monotonic_elapsed(now, self.current_started_at);
        if elapsed < interval {
            return;
        }

        let two_intervals = interval.checked_mul(2).unwrap_or(Duration::MAX);
        self.previous = if elapsed >= two_intervals {
            SloSummaryState::new()
        } else {
            self.current.clone()
        };
        self.current = SloSummaryState::new();
        self.current_started_at = now;
    }
}

#[derive(Clone)]
struct SloSummaryState {
    total_operations: u64,
    successful_operations: u64,
    error_operations: u64,
    cancellations: u64,
    retry_exhaustions: u64,
    recoveries: u64,
    queue_pressure_observations: u64,
    queue_pressure_last: Option<f64>,
    queue_pressure_max: Option<f64>,
    latency: DistributionState,
    stale_state_duration: DistributionState,
}

impl SloSummaryState {
    fn new() -> Self {
        Self {
            total_operations: 0,
            successful_operations: 0,
            error_operations: 0,
            cancellations: 0,
            retry_exhaustions: 0,
            recoveries: 0,
            queue_pressure_observations: 0,
            queue_pressure_last: None,
            queue_pressure_max: None,
            latency: DistributionState::new(DistributionSpec::latency_millis()),
            stale_state_duration: DistributionState::new(DistributionSpec::latency_millis()),
        }
    }

    fn apply(&mut self, observation: &SloObservation) {
        match observation {
            SloObservation::OperationOutcome(outcome) => {
                self.total_operations = self.total_operations.saturating_add(1);
                if outcome.is_success_like() {
                    self.successful_operations = self.successful_operations.saturating_add(1);
                } else if *outcome == OutcomeClass::Cancelled {
                    self.cancellations = self.cancellations.saturating_add(1);
                } else {
                    self.error_operations = self.error_operations.saturating_add(1);
                }
            }
            SloObservation::Latency(duration) => {
                self.latency.observe(duration_to_millis(*duration));
            }
            SloObservation::RetryExhausted => {
                self.retry_exhaustions = self.retry_exhaustions.saturating_add(1);
            }
            SloObservation::RecoveryOccurred => {
                self.recoveries = self.recoveries.saturating_add(1);
            }
            SloObservation::QueuePressure(value) => {
                self.queue_pressure_observations =
                    self.queue_pressure_observations.saturating_add(1);
                self.queue_pressure_last = Some(*value);
                self.queue_pressure_max = Some(
                    self.queue_pressure_max
                        .map_or(*value, |current| current.max(*value)),
                );
            }
            SloObservation::StaleState(duration) => {
                self.stale_state_duration
                    .observe(duration_to_millis(*duration));
            }
        }
    }

    fn snapshot(&self) -> SloSummarySnapshot {
        SloSummarySnapshot {
            total_operations: self.total_operations,
            successful_operations: self.successful_operations,
            error_operations: self.error_operations,
            cancellations: self.cancellations,
            retry_exhaustions: self.retry_exhaustions,
            recoveries: self.recoveries,
            queue_pressure_observations: self.queue_pressure_observations,
            queue_pressure_last: self.queue_pressure_last,
            queue_pressure_max: self.queue_pressure_max,
            latency: self.latency.snapshot(),
            stale_state_duration: self.stale_state_duration.snapshot(),
        }
    }
}

fn observation_is_valid(observation: &SloObservation) -> bool {
    match observation {
        SloObservation::QueuePressure(value) => value.is_finite() && *value >= 0.0,
        _ => true,
    }
}

fn duration_to_millis(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1_000.0
}

fn stable_dimension(value: &str) -> Option<String> {
    sanitize_stable_identifier(value, MAX_CONTEXT_DIMENSION_LEN)
}

fn normalized_interval(interval: Duration) -> Duration {
    if interval.is_zero() {
        Duration::from_secs(60)
    } else {
        interval
    }
}

fn ratio(numerator: u64, denominator: u64) -> Option<f64> {
    if denominator == 0 {
        None
    } else {
        Some(numerator as f64 / denominator as f64)
    }
}
