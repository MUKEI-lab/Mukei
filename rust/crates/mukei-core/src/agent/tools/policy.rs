//! Tool execution policy + failure classification (TRD §2.5).
//!
//! Two types live here:
//!   * [`FailureKind`] — classifies why a single tool invocation failed.
//!     Drives both the abuse-tracker accounting AND the structured
//!     feedback envelope handed back to the LLM.
//!   * [`ToolExecutionPolicy`] — configurable thresholds. Constructed
//!     once at boot from `config.toml::[agent]` and passed to
//!     [`super::ToolExecutor::with_policy`].

use std::time::Duration;

use crate::error::MukeiError;

// ---------------------------------------------------------------------
// FailureKind
// ---------------------------------------------------------------------

/// Classification of why a single tool invocation failed. Drives both
/// the structured feedback to the LLM and the abuse-tracker accounting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailureKind {
    /// Network / HTTP / disk-flake / external-service-down.
    /// Counts toward the threshold but the LLM is told the tool may
    /// succeed on retry with different arguments.
    Transient,
    /// The tool ran, the arguments were syntactically valid, but the
    /// requested operation was semantically impossible (e.g. SAF token
    /// not granted; expression contained a non-whitelisted identifier).
    /// Counts toward the threshold. The LLM is told NOT to retry with
    /// the same arguments — different arguments may work.
    Validation,
    /// The user / OS cancelled the operation. NOT a failure — does not
    /// count toward the threshold; the LLM is told the result is
    /// indeterminate.
    Cancelled,
    /// The tool exceeded its per-call timeout. Counts as a normal
    /// failure for threshold purposes but emits a distinct remediation
    /// hint ("simplify the request").
    Timeout,
    /// The tool reported an unrecoverable condition (sandbox violation,
    /// permanently disabled, unknown). **Bypasses the threshold** —
    /// blocks the tool immediately.
    Permanent,
    /// The tool produced byte-identical output multiple times in a row
    /// for the same `(tool, fingerprint)` pair. The model is stuck in a
    /// no-progress loop. **Bypasses the threshold** — blocks the tool
    /// immediately for the rest of the turn.
    Abuse,
}

impl FailureKind {
    /// Classify a [`MukeiError`] into the policy class that drives
    /// threshold accounting and feedback shape.
    pub fn classify(err: &MukeiError) -> Self {
        match err {
            MukeiError::Cancelled => Self::Cancelled,
            MukeiError::ToolTimeout(_) => Self::Timeout,

            // Permanent — instant block.
            MukeiError::SandboxViolation
            | MukeiError::ToolPermanentlyDisabled { .. }
            | MukeiError::UnknownTool { .. }
            | MukeiError::PermissionDenied
            | MukeiError::SafRevoked
            | MukeiError::SafRequired => Self::Permanent,

            // Abuse-triggered block coming back from the watchdog layer.
            MukeiError::ToolAbuseBlocked { .. } => Self::Abuse,

            // Validation — same arguments will keep failing.
            MukeiError::ToolArgumentInvalid { .. }
            | MukeiError::ToolArgsRejected { .. }
            | MukeiError::ToolParseFailed(_)
            | MukeiError::BinaryFile => Self::Validation,

            // Everything else (network / HTTP / I/O / catch-all) is transient.
            _ => Self::Transient,
        }
    }

    /// True iff this failure should advance the per-fingerprint counter.
    pub fn counts_toward_threshold(self) -> bool {
        !matches!(self, Self::Cancelled | Self::Abuse | Self::Permanent)
    }

    /// True iff this failure blocks the tool immediately, regardless of
    /// the counter.
    pub fn is_instant_block(self) -> bool {
        matches!(self, Self::Permanent | Self::Abuse)
    }

    /// Backwards-compatible predicate retained for older tests/callers.
    ///
    /// Unlike [`Self::is_instant_block`], this only returns `true` for the
    /// `Permanent` class and not for `Abuse`.
    pub fn is_permanent(self) -> bool {
        matches!(self, Self::Permanent)
    }

    /// Stable identifier used in the structured-feedback envelope.
    pub fn as_tag(self) -> &'static str {
        match self {
            Self::Transient => "transient",
            Self::Validation => "validation",
            Self::Cancelled => "cancelled",
            Self::Timeout => "timeout",
            Self::Permanent => "permanent",
            Self::Abuse => "abuse",
        }
    }

    /// Human-readable remediation guidance handed back to the LLM so it
    /// can plan the next turn without guessing.
    pub fn remediation_hint(self) -> &'static str {
        match self {
            Self::Transient =>
                "The tool failed transiently (network / external service). Retrying with the SAME arguments is allowed, but consider whether different arguments would also satisfy the user.",
            Self::Validation =>
                "The tool rejected the arguments as semantically invalid. Do NOT retry with the same arguments — reformulate the call or pick a different tool.",
            Self::Cancelled =>
                "The tool was cancelled before completing. Treat the result as indeterminate; do not infer anything from the lack of output.",
            Self::Timeout =>
                "The tool exceeded its per-call timeout. If you retry, simplify the request (shorter query, smaller file, narrower expression).",
            Self::Permanent =>
                "This tool is now permanently disabled for the rest of the conversation. Produce a final answer using ONLY the context already gathered — do not call this tool again.",
            Self::Abuse =>
                "This tool was called repeatedly with the same arguments and returned no progress. It is now blocked for the rest of this turn. Produce a final answer from the context already gathered, OR pick a different tool.",
        }
    }
}

// ---------------------------------------------------------------------
// ToolExecutionPolicy
// ---------------------------------------------------------------------

/// Configurable tool-execution policy.
///
/// Constructed once at boot from `config.toml::[agent]` and passed into
/// [`super::ToolExecutor::with_policy`]. Tests construct it directly.
///
/// # Threshold semantics (architect review GH #14 — canonical)
///
/// `max_failures_per_tool` is the number of consecutive recorded
/// failures **tolerated** before the abuse blocker activates. With the
/// default value of 5:
///
/// * Failures 1..=5: counted, but the tool is NOT yet blocked. Each
///   failure surfaces a `<external_data source="tool_error">` envelope
///   with `attempt="N/5"`.
/// * Failure 6 (i.e. one strictly greater than the threshold): the tool
///   becomes abuse-blocked for the remainder of the turn.
///
/// **Wire contract** — every consumer of this struct MUST honour the
/// same semantics:
/// * `FailureTracker::record_failure` returns `true` when post-increment
///   `count > threshold` (i.e. on the 6th hit at default 5).
/// * `ToolExecutor` pre-dispatch check blocks when
///   `pre_count > threshold` (same predicate, same comparator).
///
/// The legacy PRD §8.2 wording "fails twice consecutively" predates the
/// v0.7.5 raise to 5 and is superseded by this docstring.
#[derive(Debug, Clone)]
pub struct ToolExecutionPolicy {
    /// Threshold after which the abuse blocker activates for a given
    /// `(tool, fingerprint)` pair. Default 5 (raised from the legacy 2
    /// per audit recommendation — the old value was too brittle for
    /// network-dependent tools).
    pub max_failures_per_tool: u32,
    /// How many consecutive identical outputs must be observed before
    /// the executor decides the tool is stuck and injects a backoff
    /// hint into the feedback envelope. Default 2.
    pub repeat_output_window: usize,
    /// Pause emitted into the feedback envelope when the repeat-output
    /// detector fires. The LLM is told "wait at least this long before
    /// retrying"; the executor itself does not sleep.
    pub repeat_output_backoff: Duration,
    /// Architect review GH #13 (PRD REQ-CON-02): cap on the number of
    /// `tokio::spawn` tool tasks alive at once. Without this cap, a
    /// 50-call LLM batch saturates sockets / fds / the runtime queue,
    /// defeating the `TOOL_BLOCKING_SLOTS=2` discipline (TRD §2.2).
    pub max_concurrent_tools: usize,
}

impl ToolExecutionPolicy {
    /// Default threshold — see field doc above.
    pub const DEFAULT_MAX_FAILURES: u32 = 5;
    /// Default repeat-output window.
    pub const DEFAULT_REPEAT_OUTPUT_WINDOW: usize = 2;
    /// Default backoff hint duration (10 seconds).
    pub const DEFAULT_REPEAT_OUTPUT_BACKOFF: Duration = Duration::from_secs(10);
    /// Default concurrency cap. Aligned with `TOOL_BLOCKING_SLOTS=2`
    /// from TRD §2.2 so a 50-call LLM batch can no longer saturate the
    /// runtime. Configurable at boot via `config.toml::[agent]`.
    pub const DEFAULT_MAX_CONCURRENT_TOOLS: usize = 4;
}

impl Default for ToolExecutionPolicy {
    fn default() -> Self {
        Self {
            max_failures_per_tool: Self::DEFAULT_MAX_FAILURES,
            repeat_output_window: Self::DEFAULT_REPEAT_OUTPUT_WINDOW,
            repeat_output_backoff: Self::DEFAULT_REPEAT_OUTPUT_BACKOFF,
            max_concurrent_tools: Self::DEFAULT_MAX_CONCURRENT_TOOLS,
        }
    }
}

// Issue #13 (legacy): bridge between the on-disk `[agent]` config
// section and the runtime policy. Without this conversion the config
// schema would be cosmetic — the agent would always use defaults. The
// bridge crate calls `ToolExecutor::with_policy((&cfg.agent).into())`
// at boot.
impl From<&crate::config::AgentCfg> for ToolExecutionPolicy {
    fn from(cfg: &crate::config::AgentCfg) -> Self {
        Self {
            max_failures_per_tool: cfg.max_failures_per_tool,
            repeat_output_window: cfg.repeat_output_window as usize,
            repeat_output_backoff: Duration::from_secs(cfg.repeat_output_backoff_secs as u64),
            // GH #13: fall back to the default if the config did not
            // surface a value. AgentCfg gains this field below.
            max_concurrent_tools: cfg.max_concurrent_tools as usize,
        }
    }
}

impl From<crate::config::AgentCfg> for ToolExecutionPolicy {
    fn from(cfg: crate::config::AgentCfg) -> Self {
        (&cfg).into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_policy_matches_audit_recommendation() {
        let p = ToolExecutionPolicy::default();
        assert_eq!(p.max_failures_per_tool, 5);
        assert_eq!(p.repeat_output_window, 2);
        assert_eq!(p.repeat_output_backoff.as_secs(), 10);
        // GH #13: concurrency cap is wired and matches the documented
        // default of 4 (aligned to TOOL_BLOCKING_SLOTS).
        assert_eq!(p.max_concurrent_tools, 4);
    }

    #[test]
    fn failure_kind_classification_matrix() {
        assert_eq!(FailureKind::classify(&MukeiError::Cancelled), FailureKind::Cancelled);
        assert_eq!(FailureKind::classify(&MukeiError::ToolTimeout(None)), FailureKind::Timeout);
        assert_eq!(FailureKind::classify(&MukeiError::SandboxViolation), FailureKind::Permanent);
        assert_eq!(
            FailureKind::classify(&MukeiError::ToolPermanentlyDisabled { tool_name: "x".into() }),
            FailureKind::Permanent
        );
        assert_eq!(
            FailureKind::classify(&MukeiError::ToolArgumentInvalid { field: "q", reason: "empty".into() }),
            FailureKind::Validation
        );
        assert_eq!(
            FailureKind::classify(&MukeiError::WebSearchFailed("net".into())),
            FailureKind::Transient
        );
        assert_eq!(
            FailureKind::classify(&MukeiError::ToolAbuseBlocked { tool_name: "x".into() }),
            FailureKind::Abuse
        );
    }

    #[test]
    fn cancelled_abuse_and_permanent_do_not_count() {
        // Cancelled: user/OS asked us to stop.
        assert!(!FailureKind::Cancelled.counts_toward_threshold());
        // Abuse / Permanent: already blocked, no need to keep counting.
        assert!(!FailureKind::Abuse.counts_toward_threshold());
        assert!(!FailureKind::Permanent.counts_toward_threshold());
        // Everything else does count.
        assert!(FailureKind::Transient.counts_toward_threshold());
        assert!(FailureKind::Validation.counts_toward_threshold());
        assert!(FailureKind::Timeout.counts_toward_threshold());
    }

    #[test]
    fn config_round_trips_into_policy() {
        // Issue #13 regression: AgentCfg → ToolExecutionPolicy carries
        // every field. If a new field is added to AgentCfg, this test
        // must be updated and the conversion above amended.
        let cfg = crate::config::AgentCfg {
            max_failures_per_tool: 7,
            recovered_history_window: 12,
            repeat_output_window: 4,
            repeat_output_backoff_secs: 30,
            max_concurrent_tools: 6,
        };
        let policy: ToolExecutionPolicy = (&cfg).into();
        assert_eq!(policy.max_failures_per_tool, 7);
        assert_eq!(policy.repeat_output_window, 4);
        assert_eq!(policy.repeat_output_backoff.as_secs(), 30);
        assert_eq!(policy.max_concurrent_tools, 6);
    }

    #[test]
    fn instant_block_predicate_covers_abuse_and_permanent() {
        assert!(FailureKind::Permanent.is_instant_block());
        assert!(FailureKind::Abuse.is_instant_block());
        assert!(!FailureKind::Transient.is_instant_block());
        assert!(!FailureKind::Validation.is_instant_block());
        assert!(!FailureKind::Timeout.is_instant_block());
        assert!(!FailureKind::Cancelled.is_instant_block());
    }
}
