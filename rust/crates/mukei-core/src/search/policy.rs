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

// Architect review GH #34: bridge `[search]` block from `config.toml`
// into the runtime planner policy. Without this conversion the new
// SearchCfg fields would be cosmetic. The bridge crate calls
// `PlannerPolicy::from(&cfg.search)` at boot.
impl From<&crate::config::SearchCfg> for PlannerPolicy {
    fn from(cfg: &crate::config::SearchCfg) -> Self {
        Self {
            timeouts: TimeoutBudget {
                brave:  Duration::from_secs(cfg.brave_timeout_secs),
                tavily: Duration::from_secs(cfg.tavily_timeout_secs),
            },
            max_parallel_engines: cfg.max_parallel_engines.max(1),
            hits_per_engine: 5,
            enable_cache: cfg.enable_cache,
            min_results_floor: 1,
        }
    }
}

impl From<crate::config::SearchCfg> for PlannerPolicy {
    fn from(cfg: crate::config::SearchCfg) -> Self { (&cfg).into() }
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

    #[test]
    fn config_round_trips_into_policy() {
        // Architect review GH #34: every SearchCfg field MUST land in
        // the runtime policy. If a new field is added to SearchCfg,
        // this test must be updated and the conversion above amended.
        let cfg = crate::config::SearchCfg {
            brave_timeout_secs: 7,
            tavily_timeout_secs: 11,
            max_parallel_engines: 4,
            enable_cache: false,
        };
        let policy: PlannerPolicy = (&cfg).into();
        assert_eq!(policy.timeouts.brave.as_secs(), 7);
        assert_eq!(policy.timeouts.tavily.as_secs(), 11);
        assert_eq!(policy.max_parallel_engines, 4);
        assert!(!policy.enable_cache);
    }
}
