//! `tool_audit_log` hash-chained writer (Issue #2, TRD §6.1 / BS v1.2).
//!
//! Every tool call must produce one row in `tool_audit_log`. The row is
//! hash-chained: `entry_hash = SHA256(previous_hash || canonical_fields)`,
//! so any tampering with a historical row breaks the chain.
//!
//! # Invariants
//!
//! - The writer holds the previous `entry_hash` in memory; on boot the
//!   bridge crate calls [`AuditLogWriter::hydrate_from_pool`] to fetch
//!   the most recent row's hash and seed the chain.
//! - All writes go through [`PooledConnectionExt::with_conn`] (TRD §2.4
//!   Golden Rule).
//! - A failed write is **fatal** to the bridge: the conversation is
//!   left in a recoverable state and the user is notified. Silently
//!   dropping audit rows would defeat the entire chain.

#![cfg(feature = "rusqlite")]

use parking_lot::Mutex;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::diagnostics::crash_logger::hex_helper;
use crate::error::{MukeiError, Result};

use super::pool::{DatabasePool, DbError, PooledConnectionExt};

/// One row staged for insertion into `tool_audit_log`. The bridge crate
/// constructs these from the agent loop's `ToolOutcome` stream.
#[derive(Clone, Debug)]
pub struct AuditEntry {
    /// Conversation id (NULL during boot smoke tests / debugger sessions).
    pub conversation_id: Option<i64>,
    /// Message id (NULL during boot smoke tests).
    pub message_id: Option<i64>,
    /// LLM-emitted tool call id.
    pub tool_call_id: String,
    /// Registered tool name (`web_search` / `read_file` / ...).
    pub tool_name: String,
    /// Canonical (key-sorted) JSON of the arguments.
    pub args_json: String,
    /// First ~256 chars of the output — enough for forensics, small
    /// enough not to bloat the audit DB.
    pub result_preview: String,
    /// Tool returned `ok == true`.
    pub success: bool,
    /// Wall-clock duration in milliseconds.
    pub duration_ms: u64,
    /// Stable `MukeiError::error_code()` if `success == false`.
    pub error_code: Option<String>,
    /// `FailureTracker::fingerprint` value (key-canonical SHA-256).
    pub fingerprint_sha256: String,
}

impl AuditEntry {
    /// Helper that canonicalises JSON args. Mirrors the executor's
    /// `FailureTracker::fingerprint` so audit + abuse tracker speak the
    /// same language.
    pub fn canonical_args(args: &Value) -> String {
        let mut sorted = serde_json::Map::new();
        if let Value::Object(map) = args {
            let mut keys: Vec<_> = map.keys().collect();
            keys.sort();
            for k in keys {
                sorted.insert(k.clone(), map[k].clone());
            }
        }
        serde_json::to_string(&Value::Object(sorted)).unwrap_or_else(|_| "{}".to_string())
    }
}

/// Writer that owns the rolling `previous_hash` and serialises inserts.
pub struct AuditLogWriter {
    /// Hex digest of the most recent row's `entry_hash`. `None` until
    /// the first row is written.
    previous_hash: Mutex<Option<String>>,
}

impl Default for AuditLogWriter {
    fn default() -> Self {
        Self {
            previous_hash: Mutex::new(None),
        }
    }
}

impl AuditLogWriter {
    /// Construct an empty writer.
    pub fn new() -> Self {
        Self::default()
    }

    /// Seed the chain from disk — fetch the most recent row's
    /// `entry_hash` so new entries link to it. Idempotent.
    pub async fn hydrate_from_pool(&self, pool: &DatabasePool) -> Result<()> {
        let hash: Option<String> = pool
            .with_conn(|c| {
                let mut stmt =
                    c.prepare("SELECT entry_hash FROM tool_audit_log ORDER BY id DESC LIMIT 1")?;
                let mut rows = stmt.query([])?;
                if let Some(row) = rows.next()? {
                    Ok::<_, DbError>(Some(row.get::<_, String>(0)?))
                } else {
                    Ok(None)
                }
            })
            .await?;
        *self.previous_hash.lock() = hash;
        Ok(())
    }

    /// Insert one audit row. The `entry_hash` is computed as
    /// `SHA256(previous_hash || canonical_fields)` and the rolling
    /// previous-hash is advanced on success.
    pub async fn record(&self, pool: &DatabasePool, entry: AuditEntry) -> Result<()> {
        let previous = self.previous_hash.lock().clone();
        let prev_for_hash = previous.clone().unwrap_or_default();

        // Canonical field set folded into the chain. Order matters and
        // is fixed by this implementation (changing the order requires
        // a chain reset; flagged in TRD \u00a76.1 amendment).
        let mut h = Sha256::new();
        h.update(prev_for_hash.as_bytes());
        h.update([0u8]);
        h.update(entry.tool_call_id.as_bytes());
        h.update([0u8]);
        h.update(entry.tool_name.as_bytes());
        h.update([0u8]);
        h.update(entry.args_json.as_bytes());
        h.update([0u8]);
        h.update(entry.fingerprint_sha256.as_bytes());
        h.update([0u8]);
        h.update(if entry.success { b"1" } else { b"0" });
        h.update([0u8]);
        h.update(entry.duration_ms.to_be_bytes());
        h.update([0u8]);
        if let Some(code) = &entry.error_code {
            h.update(code.as_bytes());
        }
        let entry_hash = hex_helper(&h.finalize());
        let entry_hash_for_db = entry_hash.clone();

        let prev_for_db = previous.clone();
        let entry_for_db = entry.clone();

        pool.with_conn(move |c| {
            c.execute(
                "INSERT INTO tool_audit_log \
                   (conversation_id, message_id, tool_call_id, tool_name, \
                    args_json, result_preview, success, duration_ms, error_code, \
                    fingerprint_sha256, previous_hash, entry_hash) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                rusqlite::params![
                    entry_for_db.conversation_id,
                    entry_for_db.message_id,
                    entry_for_db.tool_call_id,
                    entry_for_db.tool_name,
                    entry_for_db.args_json,
                    truncate_preview(&entry_for_db.result_preview),
                    if entry_for_db.success { 1_i64 } else { 0 },
                    entry_for_db.duration_ms as i64,
                    entry_for_db.error_code,
                    entry_for_db.fingerprint_sha256,
                    prev_for_db,
                    entry_hash_for_db,
                ],
            )?;
            Ok::<_, DbError>(())
        })
        .await
        .map_err(MukeiError::from)?;

        // Advance the chain. Failure to write makes us NOT advance —
        // re-trying after a transient error preserves continuity.
        *self.previous_hash.lock() = Some(entry_hash);
        Ok(())
    }

    /// Current chain tip (testing / forensics).
    pub fn current_tip(&self) -> Option<String> {
        self.previous_hash.lock().clone()
    }
}

// ---------------------------------------------------------------------
// Architect review GH #19 — Boot-time chain verifier.
//
// PRD REQ-SEC-03 promises an "immutable, append-only local audit log".
// That promise is only meaningful if SOMETHING detects tampering. We
// walk every row in `tool_audit_log` ordered by rowid and recompute
// each `entry_hash`, comparing against the persisted value. Any
// mismatch fails the boot with a typed error the bridge can surface
// to the user (and a `DiagnosticEvent::AuditChainBroken` is emitted
// at the same time by the diagnostics sink).
// ---------------------------------------------------------------------

/// Outcome of [`AuditLogReader::verify_chain`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AuditChainStatus {
    /// Chain verified clean. `rows_checked` is the total number of
    /// rows scanned; `tip` is the last `entry_hash` (or `None` for an
    /// empty log).
    Ok { rows_checked: u64, tip: Option<String> },
    /// Tampering detected at the given rowid. `expected` is the hash
    /// we recomputed; `found` is the value persisted in the database.
    Tampered { row_id: i64, expected: String, found: String },
}

/// Reader-only handle over `tool_audit_log`. Distinct from
/// [`AuditLogWriter`] so that boot-time verification can run in a
/// read-only transaction without contending with the writer mutex.
pub struct AuditLogReader;

impl AuditLogReader {
    /// Verify the hash chain end-to-end. Called from
    /// `MukeiAgent::initialize` AFTER the SQLCipher key is installed
    /// and BEFORE the agent loop accepts user input.
    ///
    /// Returns [`AuditChainStatus::Ok`] on a clean chain.
    /// Returns [`AuditChainStatus::Tampered`] on the FIRST inconsistent
    /// row — the caller can decide whether to quarantine the DB,
    /// surface a UI error, or both.
    pub async fn verify_chain(pool: &DatabasePool) -> Result<AuditChainStatus> {
        pool.with_conn(|c| {
            let mut stmt = c.prepare(
                "SELECT rowid, tool_call_id, tool_name, args_json, fingerprint_sha256, \
                        success, duration_ms, error_code, previous_hash, entry_hash \
                 FROM tool_audit_log \
                 ORDER BY rowid ASC",
            )?;
            let mut rows = stmt.query([])?;
            let mut running_prev: Option<String> = None;
            let mut rows_checked: u64 = 0;
            while let Some(row) = rows.next()? {
                let row_id: i64 = row.get(0)?;
                let tool_call_id: String = row.get(1)?;
                let tool_name: String = row.get(2)?;
                let args_json: String = row.get(3)?;
                let fingerprint: String = row.get(4)?;
                let success: i64 = row.get(5)?;
                let duration_ms: i64 = row.get(6)?;
                let error_code: Option<String> = row.get(7)?;
                let stored_prev: Option<String> = row.get(8)?;
                let stored_entry_hash: String = row.get(9)?;

                // Sanity: the `previous_hash` column must match the
                // running hash we accumulated walking the chain so far.
                if stored_prev != running_prev {
                    return Ok(AuditChainStatus::Tampered {
                        row_id,
                        expected: running_prev.clone().unwrap_or_default(),
                        found: stored_prev.unwrap_or_default(),
                    });
                }

                // Recompute entry_hash using the exact same canonical
                // field order as `AuditLogWriter::record`.
                let prev_for_hash = running_prev.clone().unwrap_or_default();
                let mut h = Sha256::new();
                h.update(prev_for_hash.as_bytes());
                h.update([0u8]);
                h.update(tool_call_id.as_bytes());
                h.update([0u8]);
                h.update(tool_name.as_bytes());
                h.update([0u8]);
                h.update(args_json.as_bytes());
                h.update([0u8]);
                h.update(fingerprint.as_bytes());
                h.update([0u8]);
                h.update(if success != 0 { b"1" } else { b"0" });
                h.update([0u8]);
                h.update((duration_ms as u64).to_be_bytes());
                h.update([0u8]);
                if let Some(code) = &error_code {
                    h.update(code.as_bytes());
                }
                let recomputed = hex_helper(&h.finalize());

                if recomputed != stored_entry_hash {
                    return Ok(AuditChainStatus::Tampered {
                        row_id,
                        expected: recomputed,
                        found: stored_entry_hash,
                    });
                }

                running_prev = Some(stored_entry_hash);
                rows_checked += 1;
            }
            Ok::<_, DbError>(AuditChainStatus::Ok {
                rows_checked,
                tip: running_prev,
            })
        })
        .await
        .map_err(MukeiError::from)
    }
}

const PREVIEW_MAX: usize = 256;

fn truncate_preview(s: &str) -> String {
    if s.len() <= PREVIEW_MAX {
        return s.to_string();
    }
    let mut out: String = s.chars().take(PREVIEW_MAX).collect();
    out.push_str(" \u{2026} [truncated]");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn canonical_args_is_key_order_invariant() {
        let a = json!({"a": 1, "b": 2});
        let b = json!({"b": 2, "a": 1});
        assert_eq!(
            AuditEntry::canonical_args(&a),
            AuditEntry::canonical_args(&b)
        );
    }

    #[test]
    fn preview_truncates_long_strings() {
        let long: String = "x".repeat(1000);
        let out = truncate_preview(&long);
        assert!(out.len() < long.len());
        assert!(out.ends_with("[truncated]"));
    }

    #[test]
    fn fresh_writer_has_no_chain_tip() {
        let w = AuditLogWriter::new();
        assert!(w.current_tip().is_none());
    }
}
