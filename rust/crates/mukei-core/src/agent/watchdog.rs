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

/// The wall-clock watchdog. Iteration / token / wall-time budgets are
/// all enforced here.
///
/// # Per-turn rearm contract (Issue #6)
///
/// `start` is the **turn** start, not the process boot. The agent loop
/// MUST call [`WatchdogHandle::rearm`] at the very top of every
/// `AgentLoop::run`. Without rearm, an AgentLoop alive for longer than
/// `max_wall_seconds` would trip the watchdog on iteration 0 of every
/// future turn.
pub struct Watchdog {
    /// Wrapped in a `Mutex` so the rearm path (called at turn start)
    /// can update `start` from any thread, including under an `Arc`
    /// shared by the `WatchdogHandle`. The hot `check` path takes a
    /// short lock, releases it before evaluating elapsed.
    start: std::sync::Mutex<Instant>,
    max_iterations: usize,
    max_tokens: u64,
    max_wall: Duration,
}

impl Watchdog {
    pub fn new(max_iterations: usize, max_tokens: u64, max_wall: Duration) -> Self {
        Self {
            start: std::sync::Mutex::new(Instant::now()),
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
        let start = *self.start.lock().expect("watchdog start mutex poisoned");
        let elapsed = start.elapsed();
        if elapsed >= self.max_wall {
            return Err(MukeiError::WatchdogExceeded { kind: "seconds" });
        }
        Ok(())
    }

    /// Reset the wall-clock start to `Instant::now()` so a fresh turn
    /// gets a full `max_wall` budget. Iteration / token budgets are
    /// reset by the agent loop's local counters at the same boundary.
    pub fn rearm(&self) {
        let mut g = self.start.lock().expect("watchdog start mutex poisoned");
        *g = Instant::now();
    }

    /// Architect review GH #46: remaining wall-clock budget. Returns
    /// `Duration::ZERO` if the budget is already exhausted. Used by
    /// `run_inference` to bound a single inference call by the same
    /// deadline the agent loop enforces — a hung inference call no
    /// longer relies on the QML-side CancellationToken alone.
    pub fn remaining_wall_clock(&self) -> Duration {
        let start = *self.start.lock().expect("watchdog start mutex poisoned");
        self.max_wall.saturating_sub(start.elapsed())
    }
}

/// Convenience: cloneable handle carried across awaits (REQ-CON-03).
#[derive(Clone)]
pub struct WatchdogHandle {
    inner: std::sync::Arc<Watchdog>,
}

impl WatchdogHandle {
    pub fn new(w: Watchdog) -> Self {
        Self {
            inner: std::sync::Arc::new(w),
        }
    }

    pub fn check(&self, iteration: usize, tokens: u64) -> Result<()> {
        self.inner.check(iteration, tokens)
    }

    /// Reset the wall-clock start. Called by `AgentLoop::run` at the
    /// top of every turn (Issue #6).
    pub fn rearm(&self) {
        self.inner.rearm();
    }

    /// Architect review GH #46: see [`Watchdog::remaining_wall_clock`].
    pub fn remaining_wall_clock(&self) -> std::time::Duration {
        self.inner.remaining_wall_clock()
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
    fn rearm_restores_wall_budget() {
        // Issue #6 regression: a long-lived AgentLoop must not have its
        // wall-clock budget exhausted by uptime alone.
        let w = Watchdog::new(100, 1_000_000, Duration::from_millis(20));
        std::thread::sleep(Duration::from_millis(40));
        // Before rearm, the budget is gone.
        assert!(w.check(0, 0).is_err());
        // Rearm — the next check starts from a fresh `now`.
        w.rearm();
        assert!(w.check(0, 0).is_ok());
    }

    #[test]
    fn handle_rearm_propagates() {
        let h = WatchdogHandle::new(Watchdog::new(100, 1_000_000, Duration::from_millis(20)));
        std::thread::sleep(Duration::from_millis(40));
        assert!(h.check(0, 0).is_err());
        h.rearm();
        assert!(h.check(0, 0).is_ok());
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

    #[test]
    fn remaining_wall_clock_shrinks_with_uptime() {
        // Architect review GH #46 regression: the wall-clock budget
        // exposed by `remaining_wall_clock` MUST track the budget the
        // `check` path enforces.
        let w = Watchdog::new(100, 1_000_000, Duration::from_millis(50));
        let before = w.remaining_wall_clock();
        std::thread::sleep(Duration::from_millis(10));
        let after = w.remaining_wall_clock();
        assert!(
            after < before,
            "remaining must shrink: {before:?} -> {after:?}"
        );
        // After the full budget elapses, remaining saturates at 0.
        std::thread::sleep(Duration::from_millis(80));
        assert_eq!(w.remaining_wall_clock(), Duration::ZERO);
    }
}
