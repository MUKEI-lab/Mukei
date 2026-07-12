//! Deterministic health signals and bounded aggregation.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use parking_lot::RwLock;

use crate::diagnostics::redaction::sanitize_stable_identifier;

use super::clock::{monotonic_elapsed, ObservabilityClock, SystemClock};
use super::context::MAX_COMPONENT_NAME_LEN;
use super::event::{AttributeValue, EventAttribute};
use super::privacy::FieldSensitivity;

pub const MAX_HEALTH_REASON_CODE_LEN: usize = 64;
pub const MAX_HEALTH_DETAILS: usize = 8;
pub const HEALTH_COALESCE_WINDOW: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HealthState {
    Healthy,
    Degraded,
    Unhealthy,
    Unknown,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HealthSignal {
    component: String,
    state: HealthState,
    reason_code: String,
    observed_at: DateTime<Utc>,
    expires_after: Option<Duration>,
    critical: bool,
    details: Vec<EventAttribute>,
    dropped_details: u32,
}

impl HealthSignal {
    pub fn new(
        component: impl AsRef<str>,
        state: HealthState,
        reason_code: impl AsRef<str>,
    ) -> Result<Self, HealthSignalError> {
        let component = sanitize_stable_identifier(component.as_ref(), MAX_COMPONENT_NAME_LEN)
            .ok_or(HealthSignalError::InvalidComponent)?;
        let reason_code =
            sanitize_stable_identifier(reason_code.as_ref(), MAX_HEALTH_REASON_CODE_LEN)
                .ok_or(HealthSignalError::InvalidReasonCode)?;

        Ok(Self {
            component,
            state,
            reason_code,
            observed_at: Utc::now(),
            expires_after: None,
            critical: false,
            details: Vec::with_capacity(MAX_HEALTH_DETAILS),
            dropped_details: 0,
        })
    }

    pub fn observed_at(mut self, observed_at: DateTime<Utc>) -> Self {
        self.observed_at = observed_at;
        self
    }

    pub fn expires_after(mut self, expires_after: Duration) -> Self {
        self.expires_after = Some(expires_after);
        self
    }

    pub fn critical(mut self, critical: bool) -> Self {
        self.critical = critical;
        self
    }

    pub fn detail(
        mut self,
        key: impl AsRef<str>,
        value: AttributeValue,
        sensitivity: FieldSensitivity,
    ) -> Self {
        if self.details.len() >= MAX_HEALTH_DETAILS {
            self.dropped_details = self.dropped_details.saturating_add(1);
            return self;
        }
        match EventAttribute::try_new(key, value, sensitivity) {
            Ok(detail) => self.details.push(detail),
            Err(_) => self.dropped_details = self.dropped_details.saturating_add(1),
        }
        self
    }

    pub fn component(&self) -> &str {
        &self.component
    }

    pub const fn state(&self) -> HealthState {
        self.state
    }

    pub fn reason_code(&self) -> &str {
        &self.reason_code
    }

    pub fn effective_state(&self, now: DateTime<Utc>) -> HealthState {
        if self.is_stale(now) {
            HealthState::Unknown
        } else {
            self.state
        }
    }

    pub fn is_stale(&self, now: DateTime<Utc>) -> bool {
        let Some(expiry) = self.expires_after else {
            return false;
        };
        let Ok(expiry) = chrono::Duration::from_std(expiry) else {
            return false;
        };
        now >= self.observed_at + expiry
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HealthSignalError {
    InvalidComponent,
    InvalidReasonCode,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HealthSignalSnapshot {
    pub component: String,
    pub state: HealthState,
    pub reason_code: String,
    pub observed_at: DateTime<Utc>,
    pub expires_after: Option<Duration>,
    pub stale: bool,
    pub critical: bool,
    pub details: Vec<EventAttribute>,
    pub dropped_details: u32,
    pub previous_state: Option<HealthState>,
    pub transition_count: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HealthSummary {
    pub captured_at: DateTime<Utc>,
    pub overall: HealthState,
    pub signals: Vec<HealthSignalSnapshot>,
    pub cardinality_overflow_count: u64,
    pub coalesced_count: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HealthPublishStatus {
    Published,
    SeriesLimitExceeded,
    PolicyRejected,
}

#[derive(Clone)]
pub struct HealthRegistry {
    inner: Arc<HealthRegistryInner>,
}

struct HealthRegistryInner {
    signals: RwLock<HashMap<String, StoredHealthSignal>>,
    max_signals: usize,
    clock: Arc<dyn ObservabilityClock>,
    cardinality_overflow_count: AtomicU64,
    coalesced_count: AtomicU64,
}

impl HealthRegistry {
    pub fn new(max_signals: usize) -> Self {
        Self::with_clock(max_signals, SystemClock::shared())
    }

    pub fn with_clock(max_signals: usize, clock: Arc<dyn ObservabilityClock>) -> Self {
        Self {
            inner: Arc::new(HealthRegistryInner {
                signals: RwLock::new(HashMap::new()),
                max_signals,
                clock,
                cardinality_overflow_count: AtomicU64::new(0),
                coalesced_count: AtomicU64::new(0),
            }),
        }
    }

    pub fn publish(&self, signal: HealthSignal) -> HealthPublishStatus {
        let now = self.inner.clock.monotonic_now();
        let mut signals = self.inner.signals.write();
        if let Some(existing) = signals.get_mut(signal.component()) {
            if existing.is_equivalent(&signal)
                && monotonic_elapsed(now, existing.last_update_mono) <= HEALTH_COALESCE_WINDOW
            {
                existing.signal = signal;
                existing.observed_mono = now;
                existing.last_update_mono = now;
                self.inner.coalesced_count.fetch_add(1, Ordering::Relaxed);
                return HealthPublishStatus::Published;
            }

            let previous_state = if existing.signal.state != signal.state {
                Some(existing.signal.state)
            } else {
                existing.previous_state
            };
            let transition_count = if existing.signal.state != signal.state {
                existing.transition_count.saturating_add(1)
            } else {
                existing.transition_count
            };
            *existing = StoredHealthSignal {
                signal,
                observed_mono: now,
                last_update_mono: now,
                previous_state,
                transition_count,
            };
            return HealthPublishStatus::Published;
        }

        if signals.len() >= self.inner.max_signals {
            self.inner
                .cardinality_overflow_count
                .fetch_add(1, Ordering::Relaxed);
            return HealthPublishStatus::SeriesLimitExceeded;
        }

        let component = signal.component().to_string();
        signals.insert(component, StoredHealthSignal::new(signal, now));
        HealthPublishStatus::Published
    }

    pub fn snapshot(&self) -> HealthSummary {
        self.snapshot_at(self.inner.clock.wall_now())
    }

    pub fn snapshot_at(&self, now: DateTime<Utc>) -> HealthSummary {
        let monotonic_now = self.inner.clock.monotonic_now();
        let signals = self.inner.signals.read();
        let mut snapshots = signals
            .values()
            .map(|signal| signal.snapshot(now, monotonic_now))
            .collect::<Vec<_>>();
        snapshots.sort_by(|left, right| left.component.cmp(&right.component));
        let overall = aggregate_health(&snapshots);

        HealthSummary {
            captured_at: now,
            overall,
            signals: snapshots,
            cardinality_overflow_count: self
                .inner
                .cardinality_overflow_count
                .load(Ordering::Relaxed),
            coalesced_count: self.inner.coalesced_count.load(Ordering::Relaxed),
        }
    }

    pub fn clear(&self) {
        self.inner.signals.write().clear();
        self.inner
            .cardinality_overflow_count
            .store(0, Ordering::Relaxed);
        self.inner.coalesced_count.store(0, Ordering::Relaxed);
    }

    pub fn cardinality_overflow_count(&self) -> u64 {
        self.inner
            .cardinality_overflow_count
            .load(Ordering::Relaxed)
    }

    pub fn coalesced_count(&self) -> u64 {
        self.inner.coalesced_count.load(Ordering::Relaxed)
    }
}

struct StoredHealthSignal {
    signal: HealthSignal,
    observed_mono: Duration,
    last_update_mono: Duration,
    previous_state: Option<HealthState>,
    transition_count: u64,
}

impl StoredHealthSignal {
    fn new(signal: HealthSignal, now: Duration) -> Self {
        Self {
            signal,
            observed_mono: now,
            last_update_mono: now,
            previous_state: None,
            transition_count: 0,
        }
    }

    fn is_equivalent(&self, other: &HealthSignal) -> bool {
        self.signal.state == other.state
            && self.signal.reason_code == other.reason_code
            && self.signal.critical == other.critical
            && self.signal.details == other.details
            && self.signal.expires_after == other.expires_after
    }

    fn snapshot(&self, _wall_now: DateTime<Utc>, monotonic_now: Duration) -> HealthSignalSnapshot {
        let stale = self
            .signal
            .expires_after
            .map(|expiry| monotonic_elapsed(monotonic_now, self.observed_mono) >= expiry)
            .unwrap_or(false);
        HealthSignalSnapshot {
            component: self.signal.component.clone(),
            state: if stale {
                HealthState::Unknown
            } else {
                self.signal.state
            },
            reason_code: self.signal.reason_code.clone(),
            observed_at: self.signal.observed_at,
            expires_after: self.signal.expires_after,
            stale,
            critical: self.signal.critical,
            details: self.signal.details.clone(),
            dropped_details: self.signal.dropped_details,
            previous_state: self.previous_state,
            transition_count: self.transition_count,
        }
    }
}

impl Default for HealthRegistry {
    fn default() -> Self {
        Self::new(64)
    }
}

fn aggregate_health(signals: &[HealthSignalSnapshot]) -> HealthState {
    if signals.is_empty() {
        return HealthState::Unknown;
    }

    if signals
        .iter()
        .any(|signal| signal.critical && signal.state == HealthState::Unhealthy)
    {
        return HealthState::Unhealthy;
    }

    if signals.iter().any(|signal| {
        signal.state == HealthState::Degraded
            || (!signal.critical && signal.state == HealthState::Unhealthy)
    }) {
        return HealthState::Degraded;
    }

    if signals
        .iter()
        .any(|signal| signal.critical && signal.state == HealthState::Unknown)
    {
        return HealthState::Unknown;
    }

    if signals
        .iter()
        .any(|signal| signal.state == HealthState::Healthy)
    {
        HealthState::Healthy
    } else {
        HealthState::Unknown
    }
}
