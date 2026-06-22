//! Search planner policy — timeouts, parallelism, cost guardrails.
//!
//! See migration document §7 (execution rules) and §13 (timeout policy).
//!
//! # Invariants
//!
//! - Per-engine timeouts are fixed at construction. The executor MUST
//!   surface a timeout as an empty hit set (NOT an error) so the
//!   planner can continue with whatever arrived from other engines.
//! - `max_parallel_engines` ≥ 1; the default is 2 because Brave +
//!   Tavily is the canonical multi-engine combination.

use std::time::Duration;

/// Two-engine timeout pair used by the planner.
#[derive(Clone, Copy, Debug)]
pub struct TimeoutBudget {
    /// Brave per-call timeout. Default 3 s (migration §13).
    pub brave: Duration,
    /// Tavily per-call timeout. Default 5 s (migration §13).
    pub tavily: Duration,
}

impl Default for TimeoutBudget {
    fn default() -> Self {
        Self {
            brave: Duration::from_secs(PlannerPolicy::DEFAULT_BRAVE_TIMEOUT_SECS),
            tavily: Duration::from_secs(PlannerPolicy::DEFAULT_TAVILY_TIMEOUT_SECS),
        }
    }
}

/// Adaptive-planner policy. Configurable at boot from `config.toml`.
#[derive(Clone, Debug)]
pub struct PlannerPolicy {
    /// Per-engine timeout budgets.
    pub timeouts: TimeoutBudget,
    /// Maximum number of engines the planner may invoke in parallel for
    /// a single task. Hard ceiling against accidental fan-out.
    pub max_parallel_engines: usize,
    /// Hits per engine. Bounded so cache size stays predictable.
    pub hits_per_engine: usize,
    /// Whether to enable the [`crate::search::cache::SearchCache`]
    /// layer. Production = `true`; tests sometimes flip this off.
    pub enable_cache: bool,
    /// Floor on the number of results required before the planner
    /// returns. Below this it MAY broaden the engine set.
    pub min_results_floor: usize,
}

impl PlannerPolicy {
    /// Migration §13.
    pub const DEFAULT_BRAVE_TIMEOUT_SECS: u64 = 3;
    /// Migration §13.
    pub const DEFAULT_TAVILY_TIMEOUT_SECS: u64 = 5;
}

impl Default for PlannerPolicy {
    fn default() -> Self {
        Self {
            timeouts: TimeoutBudget::default(),
            max_parallel_engines: 2,
            hits_per_engine: 5,
            enable_cache: true,
            min_results_floor: 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_timeouts_match_migration() {
        let t = TimeoutBudget::default();
        assert_eq!(t.brave.as_secs(), 3);
        assert_eq!(t.tavily.as_secs(), 5);
    }

    #[test]
    fn default_policy_caps_parallelism_at_two() {
        let p = PlannerPolicy::default();
        assert_eq!(p.max_parallel_engines, 2);
        assert!(p.enable_cache);
    }
}
