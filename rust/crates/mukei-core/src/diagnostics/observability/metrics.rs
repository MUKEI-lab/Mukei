//! Bounded metric registry with current, previous and lifetime summaries.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use parking_lot::RwLock;

use crate::diagnostics::redaction::sanitize_stable_identifier;

use super::clock::{monotonic_elapsed, ObservabilityClock, SystemClock};
use super::context::{MAX_COMPONENT_NAME_LEN, MAX_CONTEXT_DIMENSION_LEN};
use super::event::OutcomeClass;

pub const MAX_METRIC_NAME_LEN: usize = 96;
pub const MAX_DISTRIBUTION_BUCKETS: usize = 32;
pub const MAX_METRIC_LABELS: usize = 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MetricKind {
    Counter,
    Gauge,
    Distribution,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct MetricDimensions {
    component: Option<String>,
    operation_kind: Option<String>,
    backend_kind: Option<String>,
    feature_area: Option<String>,
    result_class: Option<String>,
}

impl MetricDimensions {
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

    pub fn with_result_class(mut self, outcome: OutcomeClass) -> Self {
        self.result_class = Some(outcome.as_str().to_string());
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

    pub fn result_class(&self) -> Option<&str> {
        self.result_class.as_deref()
    }

    pub fn label_count(&self) -> usize {
        [
            self.component.as_ref(),
            self.operation_kind.as_ref(),
            self.backend_kind.as_ref(),
            self.feature_area.as_ref(),
            self.result_class.as_ref(),
        ]
        .into_iter()
        .flatten()
        .count()
        .min(MAX_METRIC_LABELS)
    }

    pub(crate) fn estimated_size_bytes(&self) -> usize {
        [
            self.component.as_ref(),
            self.operation_kind.as_ref(),
            self.backend_kind.as_ref(),
            self.feature_area.as_ref(),
            self.result_class.as_ref(),
        ]
        .into_iter()
        .flatten()
        .map(|value| value.len().saturating_add(16))
        .fold(0usize, usize::saturating_add)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct MetricKey {
    name: String,
    dimensions: MetricDimensions,
}

impl MetricKey {
    pub fn new(
        name: impl AsRef<str>,
        dimensions: MetricDimensions,
    ) -> Result<Self, MetricRecordStatus> {
        let name = sanitize_stable_identifier(name.as_ref(), MAX_METRIC_NAME_LEN)
            .ok_or(MetricRecordStatus::InvalidIdentity)?;
        Ok(Self { name, dimensions })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn dimensions(&self) -> &MetricDimensions {
        &self.dimensions
    }

    pub(crate) fn estimated_size_bytes(&self) -> usize {
        self.name
            .len()
            .saturating_add(self.dimensions.estimated_size_bytes())
            .saturating_add(32)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct DistributionSpec {
    bounds: Vec<f64>,
}

impl DistributionSpec {
    pub fn new(bounds: impl IntoIterator<Item = f64>) -> Result<Self, DistributionSpecError> {
        let mut bounds = bounds.into_iter().collect::<Vec<_>>();
        if bounds.is_empty() || bounds.len() > MAX_DISTRIBUTION_BUCKETS {
            return Err(DistributionSpecError::InvalidBucketCount);
        }
        if bounds
            .iter()
            .any(|value| !value.is_finite() || *value < 0.0)
        {
            return Err(DistributionSpecError::InvalidBound);
        }
        bounds.sort_by(|left, right| left.total_cmp(right));
        bounds.dedup_by(|left, right| left.total_cmp(right).is_eq());
        if bounds.is_empty() || bounds.len() > MAX_DISTRIBUTION_BUCKETS {
            return Err(DistributionSpecError::InvalidBucketCount);
        }
        Ok(Self { bounds })
    }

    pub fn latency_millis() -> Self {
        Self {
            bounds: vec![
                1.0, 5.0, 10.0, 25.0, 50.0, 100.0, 250.0, 500.0, 1_000.0, 2_500.0, 5_000.0,
                10_000.0, 30_000.0,
            ],
        }
    }

    pub fn size_units() -> Self {
        Self {
            bounds: vec![
                1.0,
                4.0,
                16.0,
                64.0,
                256.0,
                1_024.0,
                4_096.0,
                16_384.0,
                65_536.0,
                262_144.0,
                1_048_576.0,
            ],
        }
    }

    pub fn bounds(&self) -> &[f64] {
        &self.bounds
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DistributionSpecError {
    InvalidBucketCount,
    InvalidBound,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DistributionSnapshot {
    pub count: u64,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub sum: f64,
    pub buckets: Vec<BucketSnapshot>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BucketSnapshot {
    /// Inclusive upper bound. `None` is the final +infinity bucket.
    pub upper_bound: Option<f64>,
    pub count: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum MetricValueSnapshot {
    Counter(u64),
    Gauge(Option<f64>),
    Distribution(DistributionSnapshot),
}

#[derive(Clone, Debug, PartialEq)]
pub struct MetricSeriesSnapshot {
    pub key: MetricKey,
    pub kind: MetricKind,
    pub current: MetricValueSnapshot,
    pub previous: MetricValueSnapshot,
    pub lifetime: MetricValueSnapshot,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MetricRegistrySnapshot {
    pub captured_at: DateTime<Utc>,
    pub interval: Duration,
    pub series: Vec<MetricSeriesSnapshot>,
    pub cardinality_overflow_count: u64,
}

impl MetricRegistrySnapshot {
    pub(crate) fn estimated_size_bytes(&self) -> usize {
        self.series
            .iter()
            .map(|series| {
                series
                    .key
                    .estimated_size_bytes()
                    .saturating_add(metric_value_size(&series.current))
                    .saturating_add(metric_value_size(&series.previous))
                    .saturating_add(metric_value_size(&series.lifetime))
                    .saturating_add(64)
            })
            .fold(128usize, usize::saturating_add)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MetricRecordStatus {
    Recorded,
    SeriesLimitExceeded,
    TypeMismatch,
    InvalidIdentity,
    InvalidValue,
    PolicyRejected,
}

#[derive(Clone)]
pub struct MetricRegistry {
    inner: Arc<MetricRegistryInner>,
}

struct MetricRegistryInner {
    state: RwLock<HashMap<MetricKey, WindowedMetric>>,
    max_series: usize,
    interval: Duration,
    clock: Arc<dyn ObservabilityClock>,
    cardinality_overflow_count: AtomicU64,
}

impl MetricRegistry {
    pub fn new(max_series: usize, interval: Duration) -> Self {
        Self::with_clock(max_series, interval, SystemClock::shared())
    }

    pub fn with_clock(
        max_series: usize,
        interval: Duration,
        clock: Arc<dyn ObservabilityClock>,
    ) -> Self {
        let interval = normalized_interval(interval);
        Self {
            inner: Arc::new(MetricRegistryInner {
                state: RwLock::new(HashMap::new()),
                max_series,
                interval,
                clock,
                cardinality_overflow_count: AtomicU64::new(0),
            }),
        }
    }

    pub fn increment_counter(
        &self,
        name: impl AsRef<str>,
        dimensions: MetricDimensions,
        delta: u64,
    ) -> MetricRecordStatus {
        let key = match MetricKey::new(name, dimensions) {
            Ok(key) => key,
            Err(status) => return status,
        };
        self.record(
            key,
            MetricUpdate::Counter(delta),
            self.inner.clock.monotonic_now(),
        )
    }

    pub fn set_gauge(
        &self,
        name: impl AsRef<str>,
        dimensions: MetricDimensions,
        value: f64,
    ) -> MetricRecordStatus {
        if !value.is_finite() {
            return MetricRecordStatus::InvalidValue;
        }
        let key = match MetricKey::new(name, dimensions) {
            Ok(key) => key,
            Err(status) => return status,
        };
        self.record(
            key,
            MetricUpdate::Gauge(value),
            self.inner.clock.monotonic_now(),
        )
    }

    pub fn observe_distribution(
        &self,
        name: impl AsRef<str>,
        dimensions: MetricDimensions,
        value: f64,
        spec: DistributionSpec,
    ) -> MetricRecordStatus {
        if !value.is_finite() || value < 0.0 {
            return MetricRecordStatus::InvalidValue;
        }
        let key = match MetricKey::new(name, dimensions) {
            Ok(key) => key,
            Err(status) => return status,
        };
        self.record(
            key,
            MetricUpdate::Distribution(value, spec),
            self.inner.clock.monotonic_now(),
        )
    }

    pub fn snapshot(&self) -> MetricRegistrySnapshot {
        self.snapshot_at(self.inner.clock.wall_now())
    }

    pub fn snapshot_at(&self, now: DateTime<Utc>) -> MetricRegistrySnapshot {
        let monotonic_now = self.inner.clock.monotonic_now();
        let mut state = self.inner.state.write();
        for metric in state.values_mut() {
            metric.rotate(monotonic_now, self.inner.interval);
        }

        let mut series = state
            .iter()
            .map(|(key, metric)| metric.snapshot(key.clone()))
            .collect::<Vec<_>>();
        series.sort_by(|left, right| metric_sort_key(&left.key).cmp(&metric_sort_key(&right.key)));

        MetricRegistrySnapshot {
            captured_at: now,
            interval: self.inner.interval,
            series,
            cardinality_overflow_count: self
                .inner
                .cardinality_overflow_count
                .load(Ordering::Relaxed),
        }
    }

    /// Clear current and previous interval data while keeping lifetime totals.
    pub fn reset_intervals(&self) {
        let now = self.inner.clock.monotonic_now();
        let mut state = self.inner.state.write();
        for metric in state.values_mut() {
            metric.reset_intervals(now);
        }
    }

    /// Drop every metric series and reset cardinality-overflow accounting.
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

    fn record(&self, key: MetricKey, update: MetricUpdate, now: Duration) -> MetricRecordStatus {
        let mut state = self.inner.state.write();

        if let Some(metric) = state.get_mut(&key) {
            metric.rotate(now, self.inner.interval);
            return metric.apply(update);
        }

        if state.len() >= self.inner.max_series {
            self.inner
                .cardinality_overflow_count
                .fetch_add(1, Ordering::Relaxed);
            return MetricRecordStatus::SeriesLimitExceeded;
        }

        let mut metric = WindowedMetric::new(&update, now);
        let status = metric.apply(update);
        if status == MetricRecordStatus::Recorded {
            state.insert(key, metric);
        }
        status
    }
}

impl Default for MetricRegistry {
    fn default() -> Self {
        Self::new(256, Duration::from_secs(60))
    }
}

enum MetricUpdate {
    Counter(u64),
    Gauge(f64),
    Distribution(f64, DistributionSpec),
}

struct WindowedMetric {
    kind: MetricKind,
    current_started_at: Duration,
    current: MetricValueState,
    previous: MetricValueState,
    lifetime: MetricValueState,
}

impl WindowedMetric {
    fn new(update: &MetricUpdate, now: Duration) -> Self {
        let (kind, empty) = match update {
            MetricUpdate::Counter(_) => (MetricKind::Counter, MetricValueState::Counter(0)),
            MetricUpdate::Gauge(_) => (MetricKind::Gauge, MetricValueState::Gauge(None)),
            MetricUpdate::Distribution(_, spec) => (
                MetricKind::Distribution,
                MetricValueState::Distribution(DistributionState::new(spec.clone())),
            ),
        };
        Self {
            kind,
            current_started_at: now,
            current: empty.clone(),
            previous: empty.clone(),
            lifetime: empty,
        }
    }

    fn apply(&mut self, update: MetricUpdate) -> MetricRecordStatus {
        match (&mut self.current, &mut self.lifetime, update) {
            (
                MetricValueState::Counter(current),
                MetricValueState::Counter(lifetime),
                MetricUpdate::Counter(delta),
            ) => {
                *current = current.saturating_add(delta);
                *lifetime = lifetime.saturating_add(delta);
                MetricRecordStatus::Recorded
            }
            (
                MetricValueState::Gauge(current),
                MetricValueState::Gauge(lifetime),
                MetricUpdate::Gauge(value),
            ) => {
                *current = Some(value);
                *lifetime = Some(value);
                MetricRecordStatus::Recorded
            }
            (
                MetricValueState::Distribution(current),
                MetricValueState::Distribution(lifetime),
                MetricUpdate::Distribution(value, spec),
            ) if current.spec == spec && lifetime.spec == spec => {
                current.observe(value);
                lifetime.observe(value);
                MetricRecordStatus::Recorded
            }
            _ => MetricRecordStatus::TypeMismatch,
        }
    }

    fn rotate(&mut self, now: Duration, interval: Duration) {
        let elapsed = monotonic_elapsed(now, self.current_started_at);
        if elapsed < interval {
            return;
        }

        let two_intervals = interval.checked_mul(2).unwrap_or(Duration::MAX);
        self.previous = if elapsed >= two_intervals {
            self.current.empty_like()
        } else {
            self.current.clone()
        };
        self.current = self.current.empty_like();
        self.current_started_at = now;
    }

    fn reset_intervals(&mut self, now: Duration) {
        self.current = self.current.empty_like();
        self.previous = self.previous.empty_like();
        self.current_started_at = now;
    }

    fn snapshot(&self, key: MetricKey) -> MetricSeriesSnapshot {
        MetricSeriesSnapshot {
            key,
            kind: self.kind,
            current: self.current.snapshot(),
            previous: self.previous.snapshot(),
            lifetime: self.lifetime.snapshot(),
        }
    }
}

#[derive(Clone)]
enum MetricValueState {
    Counter(u64),
    Gauge(Option<f64>),
    Distribution(DistributionState),
}

impl MetricValueState {
    fn empty_like(&self) -> Self {
        match self {
            Self::Counter(_) => Self::Counter(0),
            Self::Gauge(_) => Self::Gauge(None),
            Self::Distribution(state) => {
                Self::Distribution(DistributionState::new(state.spec.clone()))
            }
        }
    }

    fn snapshot(&self) -> MetricValueSnapshot {
        match self {
            Self::Counter(value) => MetricValueSnapshot::Counter(*value),
            Self::Gauge(value) => MetricValueSnapshot::Gauge(*value),
            Self::Distribution(state) => MetricValueSnapshot::Distribution(state.snapshot()),
        }
    }
}

#[derive(Clone)]
pub(crate) struct DistributionState {
    pub(crate) spec: DistributionSpec,
    count: u64,
    min: Option<f64>,
    max: Option<f64>,
    sum: f64,
    bucket_counts: Vec<u64>,
}

impl DistributionState {
    pub(crate) fn new(spec: DistributionSpec) -> Self {
        let bucket_count = spec.bounds.len().saturating_add(1);
        Self {
            spec,
            count: 0,
            min: None,
            max: None,
            sum: 0.0,
            bucket_counts: vec![0; bucket_count],
        }
    }

    pub(crate) fn observe(&mut self, value: f64) {
        if !value.is_finite() || value < 0.0 {
            return;
        }
        self.count = self.count.saturating_add(1);
        self.min = Some(self.min.map_or(value, |current| current.min(value)));
        self.max = Some(self.max.map_or(value, |current| current.max(value)));
        self.sum = finite_saturating_add(self.sum, value);

        let index = self
            .spec
            .bounds
            .iter()
            .position(|bound| value <= *bound)
            .unwrap_or(self.spec.bounds.len());
        self.bucket_counts[index] = self.bucket_counts[index].saturating_add(1);
    }

    pub(crate) fn snapshot(&self) -> DistributionSnapshot {
        let mut buckets = self
            .spec
            .bounds
            .iter()
            .copied()
            .zip(self.bucket_counts.iter().copied())
            .map(|(upper_bound, count)| BucketSnapshot {
                upper_bound: Some(upper_bound),
                count,
            })
            .collect::<Vec<_>>();
        buckets.push(BucketSnapshot {
            upper_bound: None,
            count: *self.bucket_counts.last().unwrap_or(&0),
        });

        DistributionSnapshot {
            count: self.count,
            min: self.min,
            max: self.max,
            sum: self.sum,
            buckets,
        }
    }
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

fn finite_saturating_add(left: f64, right: f64) -> f64 {
    let sum = left + right;
    if sum.is_finite() {
        sum
    } else {
        f64::MAX
    }
}

fn metric_sort_key(key: &MetricKey) -> (String, u64) {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    key.dimensions.hash(&mut hasher);
    (key.name.clone(), hasher.finish())
}

fn metric_value_size(value: &MetricValueSnapshot) -> usize {
    match value {
        MetricValueSnapshot::Counter(_) | MetricValueSnapshot::Gauge(_) => 24,
        MetricValueSnapshot::Distribution(distribution) => {
            64usize.saturating_add(distribution.buckets.len().saturating_mul(24))
        }
    }
}
