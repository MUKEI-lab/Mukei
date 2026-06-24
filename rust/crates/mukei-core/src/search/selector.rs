//! Engine selector — migration §6.
//!
//! # Invariants
//!
//! - The selector is **pure** (no I/O, no allocation beyond the
//!   returned vector). All decisions are derived from the
//!   [`TaskKind`] alone.
//! - Multi-engine selection MUST respect the order: primary first,
//!   secondary second. The executor uses that order for fallback.
//! - Adding a new engine kind requires editing this file AND the
//!   `engines/` module at the same time.

use crate::search::engines::SearchEngineKind;
use crate::search::intent::TaskKind;

/// Engine-selection policy. Stateless — the planner instantiates one
/// per call.
pub struct SearchSelector;

impl SearchSelector {
    /// Return the ordered list of engines to consult for a single task.
    /// The first entry is the primary; subsequent entries are
    /// fallbacks the executor consults only when the primary yields
    /// `< min_results_floor` hits.
    pub fn select(kind: TaskKind) -> Vec<SearchEngineKind> {
        match kind {
            // Migration §6: simple facts → Brave only.
            TaskKind::Fact | TaskKind::News | TaskKind::Local | TaskKind::Shopping => {
                vec![SearchEngineKind::Brave]
            }
            // Research / comparison / academic → Tavily first.
            TaskKind::Research | TaskKind::Compare | TaskKind::Academic => {
                vec![SearchEngineKind::Tavily, SearchEngineKind::Brave]
            }
            // Multi-step is split BEFORE selection; if a task lands here
            // it means the splitter could not break it down further, so
            // we treat it as research and consult both engines.
            TaskKind::MultiStep => vec![SearchEngineKind::Tavily, SearchEngineKind::Brave],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fact_routes_to_brave_only() {
        let engines = SearchSelector::select(TaskKind::Fact);
        assert_eq!(engines, vec![SearchEngineKind::Brave]);
    }

    #[test]
    fn news_local_shopping_route_to_brave_only() {
        for kind in [TaskKind::News, TaskKind::Local, TaskKind::Shopping] {
            let engines = SearchSelector::select(kind);
            assert_eq!(engines, vec![SearchEngineKind::Brave]);
        }
    }

    #[test]
    fn research_starts_with_tavily() {
        let engines = SearchSelector::select(TaskKind::Research);
        assert_eq!(engines[0], SearchEngineKind::Tavily);
        assert!(engines.contains(&SearchEngineKind::Brave));
    }

    #[test]
    fn compare_starts_with_tavily() {
        let engines = SearchSelector::select(TaskKind::Compare);
        assert_eq!(engines[0], SearchEngineKind::Tavily);
    }

    #[test]
    fn no_kind_returns_empty() {
        for kind in [
            TaskKind::Fact,
            TaskKind::Research,
            TaskKind::Compare,
            TaskKind::News,
            TaskKind::Academic,
            TaskKind::Shopping,
            TaskKind::Local,
            TaskKind::MultiStep,
        ] {
            assert!(!SearchSelector::select(kind).is_empty());
        }
    }
}
