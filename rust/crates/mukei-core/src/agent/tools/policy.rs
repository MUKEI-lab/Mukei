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
}

impl ToolExecutionPolicy {
    /// Default threshold — see field doc above.
    pub const DEFAULT_MAX_FAILURES: u32 = 5;
    /// Default repeat-output window.
    pub const DEFAULT_REPEAT_OUTPUT_WINDOW: usize = 2;
    /// Default backoff hint duration (10 seconds).
    pub const DEFAULT_REPEAT_OUTPUT_BACKOFF: Duration = Duration::from_secs(10);
}

impl Default for ToolExecutionPolicy {
    fn default() -> Self {
        Self {
            max_failures_per_tool: Self::DEFAULT_MAX_FAILURES,
            repeat_output_window: Self::DEFAULT_REPEAT_OUTPUT_WINDOW,
            repeat_output_backoff: Self::DEFAULT_REPEAT_OUTPUT_BACKOFF,
        }
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
    fn instant_block_predicate_covers_abuse_and_permanent() {
        assert!(FailureKind::Permanent.is_instant_block());
        assert!(FailureKind::Abuse.is_instant_block());
        assert!(!FailureKind::Transient.is_instant_block());
        assert!(!FailureKind::Validation.is_instant_block());
        assert!(!FailureKind::Timeout.is_instant_block());
        assert!(!FailureKind::Cancelled.is_instant_block());
    }
}
