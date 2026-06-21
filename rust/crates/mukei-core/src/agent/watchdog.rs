//! `mukei_core::agent::watchdog` — TRD §2.6.
//!
//! Hardware-enforced watchdog that the ReAct loop MUST consult on every
//! iteration. Triggers escalate to `MukeiError::WatchdogExceeded` which
//! the agent core turns into a typed "Task Timeout" surfaced to QML.
//!
//! Three budgets:
//!  - *iterations*  — max number of tool calls before abort.
//!  - *token_budget* — accumulated tokens across the loop.
//!  - *wall_seconds* — wall-clock budget from `start()` to `check()`.

use std::time::{Duration, Instant};

use crate::error::{MukeiError, Result};

pub struct Watchdog {
    start: Instant,
    max_iterations: usize,
    max_tokens:     u64,
    max_wall:       Duration,
}

impl Watchdog {
    pub fn new(max_iterations: usize, max_tokens: u64, max_wall: Duration) -> Self {
        Self {
            start: Instant::now(),
            max_iterations,
            max_tokens,
            max_wall,
        }
    }

    /// Called from the agent loop on every iteration. Returns `Err`
    /// when any budget is exhausted.
    pub fn check(&self, iteration: usize, tokens_so_far: u64) -> Result<()> {
        if iteration >= self.max_iterations {
            return Err(MukeiError::WatchdogExceeded { kind: "iterations" });
        }
        if tokens_so_far >= self.max_tokens {
            return Err(MukeiError::WatchdogExceeded { kind: "tokens" });
        }
        let elapsed = self.start.elapsed();
        if elapsed >= self.max_wall {
            return Err(MukeiError::WatchdogExceeded { kind: "seconds" });
        }
        Ok(())
    }
}

/// Convenience: cloneable handle carried across awaits (REQ-CON-03).
#[derive(Clone)]
pub struct WatchdogHandle {
    inner: std::sync::Arc<Watchdog>,
}

impl WatchdogHandle {
    pub fn new(w: Watchdog) -> Self {
        Self { inner: std::sync::Arc::new(w) }
    }

    pub fn check(&self, iteration: usize, tokens: u64) -> Result<()> {
        self.inner.check(iteration, tokens)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iteration_limit_triggers() {
        let w = Watchdog::new(3, 1_000_000, Duration::from_secs(60));
        assert!(w.check(2, 0).is_ok());
        assert!(matches!(
            w.check(3, 0).unwrap_err(),
            MukeiError::WatchdogExceeded { kind: "iterations" }
        ));
    }

    #[test]
    fn token_limit_triggers() {
        let w = Watchdog::new(100, 5, Duration::from_secs(60));
        assert!(w.check(0, 5).is_err());
    }

    #[test]
    fn wall_limit_triggers() {
        let w = Watchdog::new(100, 1_000_000, Duration::from_millis(0));
        // first check after zero ms sleeps — we must NOT trust elapsed
        // here; instead check after a forced elapsed.
        std::thread::sleep(Duration::from_millis(2));
        assert!(w.check(0, 0).is_err());
    }

    #[test]
    fn handle_is_clone_send_sync() {
        fn assert<T: Send + Sync>() {}
        assert::<WatchdogHandle>();
        let h = WatchdogHandle::new(Watchdog::new(1, 1, Duration::from_secs(1)));
        let h2 = h.clone();
        assert!(h.check(0, 0).is_ok());
        assert!(h2.check(0, 0).is_ok());
    }
}
