//! Injectable clock primitives for observability.
//!
//! Wall time is retained only for human-readable timestamps. All ageing,
//! window rotation, lag and elapsed-duration decisions use the monotonic
//! timeline returned by [`ObservabilityClock::monotonic_now`].

use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};

/// Clock abstraction used by metrics, health, SLO and sink lag accounting.
///
/// Implementations must return a non-decreasing monotonic value. Production
/// uses [`SystemClock`]; tests can inject deterministic clocks without
/// changing process wall time.
pub trait ObservabilityClock: Send + Sync + 'static {
    fn monotonic_now(&self) -> Duration;
    fn wall_now(&self) -> DateTime<Utc>;
}

/// Production clock backed by `Instant` for elapsed time and UTC for display.
#[derive(Debug)]
pub struct SystemClock {
    origin: Instant,
}

impl SystemClock {
    pub fn new() -> Self {
        Self {
            origin: Instant::now(),
        }
    }

    pub fn shared() -> Arc<dyn ObservabilityClock> {
        Arc::new(Self::new())
    }
}

impl Default for SystemClock {
    fn default() -> Self {
        Self::new()
    }
}

impl ObservabilityClock for SystemClock {
    fn monotonic_now(&self) -> Duration {
        self.origin.elapsed()
    }

    fn wall_now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

pub(crate) fn monotonic_elapsed(now: Duration, started: Duration) -> Duration {
    now.saturating_sub(started)
}
