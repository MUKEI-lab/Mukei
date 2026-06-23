//! `ToolExecutor` — parallel tokio dispatch + abuse tracker.
//!
//! Reads its policy from [`super::ToolExecutionPolicy`] and surfaces a
//! single classified outcome ([`ToolOutcome`]) per tool call.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use parking_lot::Mutex;
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;

use crate::agent::tools::feedback::render_tool_error_envelope;
use crate::agent::tools::policy::{FailureKind, ToolExecutionPolicy};
use crate::agent::tools::watchdog::OutputRepeatTracker;
use crate::diagnostics::crash_logger::hex_helper;
use crate::error::{MukeiError, Result};
use crate::tools::validator::TypedToolCall;
use crate::types::ToolResult;

#[cfg(feature = "rusqlite")]
use crate::storage::{AuditEntry, AuditLogWriter, DatabasePool};

// ---------------------------------------------------------------------
// FailureTracker
// ---------------------------------------------------------------------

/// Per-tool consecutive-failure counter keyed by canonical fingerprint.
#[derive(Default)]
pub struct FailureTracker {
    /// Maps `tool_name` → { fingerprint → consecutive_fail_count }.
    /// `parking_lot::Mutex` keeps the hot path single-digit µs.
    inner: Mutex<HashMap<String, HashMap<String, u32>>>,
    /// Configurable threshold; falls back to the policy default if no
    /// custom policy is wired.
    threshold: u32,
}

impl FailureTracker {
    /// Construct a tracker with the default threshold.
    pub fn new() -> Self {
        Self::with_threshold(ToolExecutionPolicy::DEFAULT_MAX_FAILURES)
    }

    /// Construct a tracker with an explicit threshold (e.g. from config).
    pub fn with_threshold(threshold: u32) -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
            threshold,
        }
    }

    /// Threshold currently in effect for this tracker.
    pub fn threshold(&self) -> u32 {
        self.threshold
    }

    /// Compute a SHA-256 fingerprint over the tool arguments with
    /// **JSON-object-key-canonical** ordering. Two payloads that differ
    /// only by key order collide.
    pub fn fingerprint(tool_name: &str, args: &serde_json::Value) -> String {
        let mut sorted = serde_json::Map::new();
        if let serde_json::Value::Object(map) = args {
            let mut keys: Vec<_> = map.keys().collect();
            keys.sort();
            for k in keys {
                sorted.insert(k.clone(), map[k].clone());
            }
        }
        let canonical = serde_json::Value::Object(sorted);

        let mut h = Sha256::new();
        h.update(tool_name.as_bytes());
        h.update([0u8]);
        let bytes = serde_json::to_vec(&canonical).unwrap_or_else(|err| {
            tracing::error!(?err, tool = %tool_name, "fingerprint canonicalisation failed — using fallback marker");
            b"<MUKEI_FP_CANONICALISATION_FAILED>".to_vec()
        });
        h.update(&bytes);
        hex_helper(&h.finalize())
    }

    /// Record a failure for `tool_name`+`fingerprint`. Returns `true`
    /// once the count exceeds the threshold (i.e. the abuse blocker
    /// should activate).
    pub fn record_failure(&self, tool_name: &str, fingerprint: &str) -> bool {
        let mut g = self.inner.lock();
        let per_tool: &mut HashMap<String, u32> = g.entry(tool_name.to_string()).or_default();
        let count = per_tool.entry(fingerprint.to_string()).or_insert(0);
        *count += 1;
        *count > self.threshold
    }

    /// Forget all recorded failures for a tool.
    pub fn reset(&self, tool_name: &str) {
        self.inner.lock().remove(tool_name);
    }

    /// Current consecutive-failure count for the given pair.
    pub fn count_for(&self, tool_name: &str, fingerprint: &str) -> u32 {
        self.inner
            .lock()
            .get(tool_name)
            .and_then(|per_tool| per_tool.get(fingerprint))
            .copied()
            .unwrap_or(0)
    }

    /// Internal accessor used by the executor's success path to clear
    /// the per-fingerprint streak after a successful call.
    pub(crate) fn clear_fingerprint(&self, tool_name: &str, fingerprint: &str) {
        if let Some(per_tool) = self.inner.lock().get_mut(tool_name) {
            per_tool.remove(fingerprint);
        }
    }
}

// ---------------------------------------------------------------------
// ToolOutcome
// ---------------------------------------------------------------------

/// Single classified outcome of a tool invocation.
#[derive(Debug, Clone)]
pub struct ToolOutcome {
    /// User-visible tool result (success or structured-error envelope).
    pub result: ToolResult,
    /// Failure class if `result.ok == false`; `None` on success.
    pub failure_kind: Option<FailureKind>,
    /// If the executor detected a no-progress loop and emitted the
    /// repeat-output supervisor block, this contains it.
    pub repeat_output_notice: Option<String>,
    /// Number of consecutive failures recorded so far for this
    /// `(tool, fingerprint)` pair, AFTER applying this outcome.
    pub attempt: u32,
}

// ---------------------------------------------------------------------
// ToolExecutor
// ---------------------------------------------------------------------

#[cfg(feature = "rusqlite")]
#[derive(Clone)]
pub struct ToolAuditSink {
    pool: Arc<DatabasePool>,
    writer: Arc<AuditLogWriter>,
}

#[cfg(feature = "rusqlite")]
impl ToolAuditSink {
    pub fn new(pool: Arc<DatabasePool>, writer: Arc<AuditLogWriter>) -> Self {
        Self { pool, writer }
    }
}

/// Parallel tokio executor — owns the [`FailureTracker`], the
/// [`OutputRepeatTracker`], and the [`ToolExecutionPolicy`].
pub struct ToolExecutor {
    registry: Arc<crate::tools::ToolRegistry>,
    tracker: Arc<FailureTracker>,
    repeats: Arc<OutputRepeatTracker>,
    policy: ToolExecutionPolicy,
    #[cfg(feature = "rusqlite")]
    audit: Option<ToolAuditSink>,
}

impl ToolExecutor {
    /// Construct with the default [`ToolExecutionPolicy`].
    pub fn new(registry: Arc<crate::tools::ToolRegistry>, tracker: Arc<FailureTracker>) -> Self {
        Self::with_policy(registry, tracker, ToolExecutionPolicy::default())
    }

    /// Construct with a custom policy (e.g. loaded from `config.toml`).
    pub fn with_policy(
        registry: Arc<crate::tools::ToolRegistry>,
        tracker: Arc<FailureTracker>,
        policy: ToolExecutionPolicy,
    ) -> Self {
        Self {
            registry,
            tracker,
            repeats: Arc::new(OutputRepeatTracker::new()),
            policy,
            #[cfg(feature = "rusqlite")]
            audit: None,
        }
    }

    /// Construct with a custom policy plus a hash-chained audit sink.
    #[cfg(feature = "rusqlite")]
    pub fn with_policy_and_audit(
        registry: Arc<crate::tools::ToolRegistry>,
        tracker: Arc<FailureTracker>,
        policy: ToolExecutionPolicy,
        pool: Arc<DatabasePool>,
        writer: Arc<AuditLogWriter>,
    ) -> Self {
        Self {
            registry,
            tracker,
            repeats: Arc::new(OutputRepeatTracker::new()),
            policy,
            audit: Some(ToolAuditSink::new(pool, writer)),
        }
    }

    /// Access the underlying failure tracker.
    pub fn tracker(&self) -> &FailureTracker {
        &self.tracker
    }

    /// Access the same-output / no-progress detector. The agent loop
    /// uses this to clear the ring at the start of every new turn
    /// (Issue #5 — the doc comment on `OutputRepeatTracker::clear`
    /// explicitly says "called at the start of each new run()").
    pub fn repeats(&self) -> &OutputRepeatTracker {
        &self.repeats
    }

    /// Access the effective policy.
    pub fn policy(&self) -> &ToolExecutionPolicy {
        &self.policy
    }

    #[cfg(feature = "rusqlite")]
    async fn audit_outcome(
        &self,
        call: &TypedToolCall,
        fingerprint: &str,
        output: &str,
        success: bool,
        took: std::time::Duration,
        error_code: Option<String>,
    ) -> Result<()> {
        let Some(audit) = &self.audit else {
            return Ok(());
        };
        let duration_ms = took.as_millis().min(u64::MAX as u128) as u64;
        let entry = AuditEntry {
            conversation_id: None,
            message_id: None,
            tool_call_id: call.id.0.to_string(),
            tool_name: call.name.clone(),
            args_json: AuditEntry::canonical_args(&call.arguments),
            result_preview: output.to_string(),
            success,
            duration_ms,
            error_code,
            fingerprint_sha256: fingerprint.to_string(),
        };
        audit.writer.record(&audit.pool, entry).await
    }

    #[cfg(not(feature = "rusqlite"))]
    async fn audit_outcome(
        &self,
        _call: &TypedToolCall,
        _fingerprint: &str,
        _output: &str,
        _success: bool,
        _took: std::time::Duration,
        _error_code: Option<String>,
    ) -> Result<()> {
        Ok(())
    }

    /// Clear ALL per-`(tool, fingerprint)` failure counters AND the
    /// same-output ring. Issue #4: the FailureTracker outlives the
    /// AgentLoop instance, so state from a previous turn / previous
    /// conversation must not leak into the next one.
    ///
    /// `AgentLoop::run` calls this once at the top of every invocation.
    pub fn reset_for_new_turn(&self) {
        // FailureTracker::reset takes a tool name — so we walk the
        // registry and reset each registered tool. New tools added in
        // the future inherit this behaviour automatically.
        for name in self.registry.names() {
            self.tracker.reset(&name);
        }
        self.repeats.clear();
    }

    /// Run a set of validated tool calls **in parallel** via
    /// `tokio::spawn`. Returns one [`ToolOutcome`] per call plus an
    /// optional `blocked` error — emitted when any call hits the abuse
    /// threshold OR triggers a permanent/abuse instant-block.
    pub async fn execute_parallel(
        &self,
        calls: Vec<TypedToolCall>,
        cancel_token: CancellationToken,
    ) -> Result<(Vec<ToolOutcome>, Option<MukeiError>)> {
        // ----- Issue #9 (CRITICAL): pre-dispatch block check ----------
        // Don't spawn a network / disk call we already know is going to
        // be blocked by the abuse tracker. We emit a structured
        // `tool_error` envelope upfront and skip the spawn entirely.
        let mut outcomes: Vec<ToolOutcome> = Vec::with_capacity(calls.len());
        let mut blocked: Option<MukeiError> = None;
        let mut handles = Vec::with_capacity(calls.len());

        for call in calls {
            let fp = FailureTracker::fingerprint(&call.name, &call.arguments);
            // (a) Already-blocked fingerprint — the tracker has it AT or
            //     OVER threshold.
            let pre_count = self.tracker.count_for(&call.name, &fp);
            if pre_count > self.tracker.threshold() {
                blocked.get_or_insert_with(|| MukeiError::ToolAbuseBlocked {
                    tool_name: call.name.clone(),
                });
                let synthetic_err = MukeiError::ToolAbuseBlocked {
                    tool_name: call.name.clone(),
                };
                let envelope = render_tool_error_envelope(
                    &call.name,
                    &synthetic_err,
                    FailureKind::Abuse,
                    pre_count,
                    self.tracker.threshold(),
                );
                self.audit_outcome(
                    &call,
                    &fp,
                    &envelope,
                    false,
                    std::time::Duration::from_millis(0),
                    Some(synthetic_err.error_code().to_string()),
                )
                .await?;
                outcomes.push(ToolOutcome {
                    result: ToolResult {
                        call_id: call.id,
                        name: call.name,
                        output: envelope,
                        ok: false,
                        took: std::time::Duration::from_millis(0),
                        trust: "system".to_string(),
                    },
                    failure_kind: Some(FailureKind::Abuse),
                    repeat_output_notice: None,
                    attempt: pre_count,
                });
                continue;
            }
            // (b) Tool not in the registry — instant Permanent block.
            //     No need to spawn a task only to discover the same.
            if self.registry.get(&call.name).is_none() {
                let synthetic_err = MukeiError::UnknownTool {
                    tool_name: call.name.clone(),
                };
                blocked.get_or_insert_with(|| MukeiError::UnknownTool {
                    tool_name: call.name.clone(),
                });
                let envelope = render_tool_error_envelope(
                    &call.name,
                    &synthetic_err,
                    FailureKind::Permanent,
                    pre_count,
                    self.tracker.threshold(),
                );
                self.audit_outcome(
                    &call,
                    &fp,
                    &envelope,
                    false,
                    std::time::Duration::from_millis(0),
                    Some(synthetic_err.error_code().to_string()),
                )
                .await?;
                outcomes.push(ToolOutcome {
                    result: ToolResult {
                        call_id: call.id,
                        name: call.name,
                        output: envelope,
                        ok: false,
                        took: std::time::Duration::from_millis(0),
                        trust: "system".to_string(),
                    },
                    failure_kind: Some(FailureKind::Permanent),
                    repeat_output_notice: None,
                    attempt: pre_count,
                });
                continue;
            }

            // ----- Live dispatch path ----------------------------------
            let registry = self.registry.clone();
            let token = cancel_token.clone();
            handles.push(tokio::spawn(async move {
                let tool = registry
                    .get(&call.name)
                    .ok_or_else(|| MukeiError::UnknownTool {
                        tool_name: call.name.clone(),
                    })?;
                let started = Instant::now();
                let result = tokio::select! {
                    res = tool.run(call.arguments.clone()) => res,
                    _ = token.cancelled() => Err(MukeiError::Cancelled),
                };
                Ok::<_, MukeiError>((call, fp, started.elapsed(), result))
            }));
        }

        for h in handles {
            match h.await {
                // ---------- Success path ----------
                Ok(Ok((call, fp, took, Ok(out)))) => {
                    // No-progress detection runs BEFORE we clear the
                    // failure streak so a model that's been "succeeding
                    // with the same answer" still trips the abuse hint.
                    let no_progress = self.repeats.record_and_check(
                        &call.name,
                        &fp,
                        &out,
                        self.policy.repeat_output_window,
                    );

                    // No-progress is its own instant-block path under
                    // the FailureKind::Abuse semantics.
                    if no_progress {
                        blocked.get_or_insert_with(|| MukeiError::ToolAbuseBlocked {
                            tool_name: call.name.clone(),
                        });
                        self.repeats.forget(&call.name, &fp);
                    }

                    let repeat_output_notice = if no_progress {
                        Some(
                            crate::agent::tools::feedback::render_repeat_output_envelope(
                                &call.name,
                                self.policy.repeat_output_backoff,
                            ),
                        )
                    } else {
                        None
                    };

                    // A success resets the failure streak for this
                    // (tool, fingerprint).
                    self.tracker.clear_fingerprint(&call.name, &fp);
                    self.audit_outcome(&call, &fp, &out, true, took, None)
                        .await?;

                    outcomes.push(ToolOutcome {
                        result: ToolResult {
                            call_id: call.id,
                            name: call.name,
                            output: out,
                            ok: true,
                            took,
                            trust: "computed".to_string(),
                        },
                        failure_kind: if no_progress {
                            Some(FailureKind::Abuse)
                        } else {
                            None
                        },
                        repeat_output_notice,
                        attempt: 0,
                    });
                }

                // ---------- Failure path ----------
                Ok(Ok((call, fp, took, Err(err)))) => {
                    let kind = FailureKind::classify(&err);

                    // Cancelled / Abuse / Permanent do NOT advance the
                    // counter. Everything else (Transient, Validation,
                    // Timeout) does.
                    let attempt = if kind.counts_toward_threshold() {
                        let bumped = self.tracker.record_failure(&call.name, &fp);
                        let attempt_count = self.tracker.count_for(&call.name, &fp);
                        if bumped {
                            blocked.get_or_insert_with(|| MukeiError::ToolAbuseBlocked {
                                tool_name: call.name.clone(),
                            });
                            self.repeats.forget(&call.name, &fp);
                        }
                        attempt_count
                    } else {
                        self.tracker.count_for(&call.name, &fp)
                    };

                    // Permanent / Abuse classifications block
                    // immediately even on the first occurrence.
                    if kind.is_instant_block() {
                        blocked.get_or_insert_with(|| MukeiError::ToolAbuseBlocked {
                            tool_name: call.name.clone(),
                        });
                    }

                    let envelope = render_tool_error_envelope(
                        &call.name,
                        &err,
                        kind,
                        attempt,
                        self.tracker.threshold(),
                    );
                    self.audit_outcome(
                        &call,
                        &fp,
                        &envelope,
                        false,
                        took,
                        Some(err.error_code().to_string()),
                    )
                    .await?;

                    outcomes.push(ToolOutcome {
                        result: ToolResult {
                            call_id: call.id,
                            name: call.name,
                            output: envelope,
                            ok: false,
                            took,
                            trust: "system".to_string(),
                        },
                        failure_kind: Some(kind),
                        repeat_output_notice: None,
                        attempt,
                    });
                }

                Ok(Err(e)) => return Err(e),
                Err(e) => return Err(MukeiError::BlockingJoinFailed(e.to_string())),
            }
        }

        Ok((outcomes, blocked))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_is_key_order_invariant() {
        let a = serde_json::json!({"a": 1, "b": 2});
        let b = serde_json::json!({"b": 2, "a": 1});
        assert_eq!(
            FailureTracker::fingerprint("x", &a),
            FailureTracker::fingerprint("x", &b),
        );
    }

    #[test]
    fn default_threshold_is_five() {
        assert_eq!(FailureTracker::new().threshold(), 5);
    }

    #[test]
    fn threshold_is_configurable() {
        let t = FailureTracker::with_threshold(3);
        let fp: String = "x".into();
        assert!(!t.record_failure("tool", &fp)); // 1
        assert!(!t.record_failure("tool", &fp)); // 2
        assert!(!t.record_failure("tool", &fp)); // 3 == threshold
        assert!(t.record_failure("tool", &fp)); // 4 > threshold
    }

    #[test]
    fn default_threshold_takes_six_hits_to_block() {
        let t = FailureTracker::new();
        let fp: String = "x".into();
        for _ in 0..5 {
            assert!(!t.record_failure("tool", &fp));
        }
        assert!(t.record_failure("tool", &fp));
    }

    #[test]
    fn reset_clears_state() {
        let t = FailureTracker::new();
        t.record_failure("tool", "fp");
        t.reset("tool");
        assert_eq!(t.count_for("tool", "fp"), 0);
    }

    #[test]
    fn clear_fingerprint_only_clears_one_entry() {
        let t = FailureTracker::new();
        t.record_failure("tool", "fp1");
        t.record_failure("tool", "fp2");
        t.clear_fingerprint("tool", "fp1");
        assert_eq!(t.count_for("tool", "fp1"), 0);
        assert_eq!(t.count_for("tool", "fp2"), 1);
    }
}
