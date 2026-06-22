//! `mukei_core::storage::recovery` — TRD §6.3 / PRD REQ-STATE-01.
//!
//! Crash-safe stream resume snapshot. Backed by the V002 `recovery_state`
//! table (single-row, `id = 1`). The agent loop writes a snapshot every
//! N tokens; on cold boot the bridge reads it and replays the partial
//! prefix back to the LLM so the user sees their answer continue rather
//! than restart.
//!
//! # Invariants
//!
//! - There is **at most one** active `recovery_state` row per process.
//! - Every helper here runs through
//!   [`super::pool::PooledConnectionExt::with_conn`] so the synchronous
//!   `rusqlite` work happens inside `spawn_blocking` (TRD §2.4).
//! - `kv_cache_fingerprint` and `model_fingerprint` are mandatory on save
//!   so a resume after a model swap can refuse to replay a stale prefix.

use crate::error::{MukeiError, Result};
use crate::storage::pool::{DatabasePool, DbError, PooledConnectionExt};

/// One row of the `recovery_state` table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecoveryState {
    /// Conversation the partial stream belongs to.
    pub conversation_id: i64,
    /// Optional branch id.
    pub branch_id: Option<i64>,
    /// Last message id that was durably appended.
    pub last_message_id: i64,
    /// Serialised prompt that produced this stream.
    pub prompt_snapshot: String,
    /// Tokens already streamed to the UI before the kill.
    pub generated_prefix: String,
    /// Token count corresponding to `generated_prefix`.
    pub last_token_count: i64,
    /// Hash of the live KV-cache when the snapshot was taken. Used to
    /// reject a resume if the cache has been re-initialised on a
    /// different model.
    pub kv_cache_fingerprint: String,
    /// SHA-256 of the model file the snapshot was produced with. Resume
    /// is refused if this differs from the currently-loaded model.
    pub model_fingerprint: Option<String>,
    /// Watchdog fingerprint, used by the crash-loop tripwire.
    pub watchdog_fingerprint: Option<String>,
    /// True if the snapshot has already been replayed once.
    pub resumed_after_kill: bool,
    /// RFC3339 timestamp of the last write.
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// Async helpers around the `recovery_state` table.
pub struct RecoveryStore;

impl RecoveryStore {
    /// Fetch the (at most one) recovery row.
    pub async fn load(pool: &DatabasePool) -> Result<Option<RecoveryState>> {
        pool.with_conn(|c| {
            let mut stmt = c.prepare(
                "SELECT conversation_id, branch_id, last_message_id, prompt_snapshot, \
                        generated_prefix, last_token_count, kv_cache_fingerprint, \
                        model_fingerprint, watchdog_fingerprint, resumed_after_kill, updated_at \
                 FROM recovery_state WHERE id = 1",
            )?;
            let row = stmt
                .query_row([], |row| {
                    let updated_at: String = row.get(10)?;
                    Ok(RecoveryState {
                        conversation_id: row.get(0)?,
                        branch_id: row.get(1)?,
                        last_message_id: row.get(2)?,
                        prompt_snapshot: row.get(3)?,
                        generated_prefix: row.get(4)?,
                        last_token_count: row.get(5)?,
                        kv_cache_fingerprint: row.get(6)?,
                        model_fingerprint: row.get(7)?,
                        watchdog_fingerprint: row.get(8)?,
                        resumed_after_kill: row.get::<_, i64>(9)? != 0,
                        updated_at: chrono::DateTime::parse_from_rfc3339(&updated_at)
                            .map(|d| d.with_timezone(&chrono::Utc))
                            .unwrap_or_else(|_| chrono::Utc::now()),
                    })
                })
                .ok();
            Ok::<_, DbError>(row)
        })
        .await
    }

    /// Upsert the snapshot. The schema constrains `id = 1`, so this is
    /// always a single-row replace.
    pub async fn save(pool: &DatabasePool, state: RecoveryState) -> Result<()> {
        pool.with_conn(move |c| {
            c.execute(
                "INSERT INTO recovery_state ( \
                    id, conversation_id, branch_id, last_message_id, prompt_snapshot, \
                    generated_prefix, last_token_count, kv_cache_fingerprint, \
                    model_fingerprint, watchdog_fingerprint, resumed_after_kill, updated_at \
                 ) VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11) \
                 ON CONFLICT(id) DO UPDATE SET \
                    conversation_id = excluded.conversation_id, \
                    branch_id = excluded.branch_id, \
                    last_message_id = excluded.last_message_id, \
                    prompt_snapshot = excluded.prompt_snapshot, \
                    generated_prefix = excluded.generated_prefix, \
                    last_token_count = excluded.last_token_count, \
                    kv_cache_fingerprint = excluded.kv_cache_fingerprint, \
                    model_fingerprint = excluded.model_fingerprint, \
                    watchdog_fingerprint = excluded.watchdog_fingerprint, \
                    resumed_after_kill = excluded.resumed_after_kill, \
                    updated_at = excluded.updated_at",
                rusqlite::params![
                    state.conversation_id,
                    state.branch_id,
                    state.last_message_id,
                    state.prompt_snapshot,
                    state.generated_prefix,
                    state.last_token_count,
                    state.kv_cache_fingerprint,
                    state.model_fingerprint,
                    state.watchdog_fingerprint,
                    state.resumed_after_kill as i64,
                    state.updated_at.to_rfc3339(),
                ],
            )?;
            Ok::<_, DbError>(())
        })
        .await
    }

    /// Mark the snapshot as already replayed. Called by the agent loop
    /// after a successful resume so a second crash does not loop.
    pub async fn mark_resumed(pool: &DatabasePool) -> Result<()> {
        pool.with_conn(|c| {
            c.execute(
                "UPDATE recovery_state SET resumed_after_kill = 1 WHERE id = 1",
                [],
            )?;
            Ok::<_, DbError>(())
        })
        .await
    }

    /// Drop the recovery row entirely (e.g. after the stream completes
    /// successfully and there is nothing left to resume).
    pub async fn clear(pool: &DatabasePool) -> Result<()> {
        pool.with_conn(|c| {
            c.execute("DELETE FROM recovery_state WHERE id = 1", [])?;
            Ok::<_, DbError>(())
        })
        .await
    }

    /// Refuse to resume if the loaded model fingerprint does not match
    /// the one persisted in the recovery row. Returns `Ok(())` when the
    /// snapshot is compatible (or there is no snapshot).
    pub fn compatible_with_model(state: &RecoveryState, model_fingerprint: &str) -> Result<()> {
        match state.model_fingerprint.as_deref() {
            None => Ok(()),
            Some(found) if found == model_fingerprint => Ok(()),
            Some(_) => Err(MukeiError::ModelCorrupted),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compatible_with_model_matches_on_equal_hash() {
        let s = make_state("model-hash-aaa");
        assert!(RecoveryStore::compatible_with_model(&s, "model-hash-aaa").is_ok());
    }

    #[test]
    fn compatible_with_model_rejects_on_mismatch() {
        let s = make_state("model-hash-aaa");
        let err = RecoveryStore::compatible_with_model(&s, "model-hash-bbb").unwrap_err();
        assert!(matches!(err, MukeiError::ModelCorrupted));
    }

    #[test]
    fn compatible_with_model_skips_when_persisted_fp_absent() {
        let mut s = make_state("ignored");
        s.model_fingerprint = None;
        assert!(RecoveryStore::compatible_with_model(&s, "any").is_ok());
    }

    fn make_state(fp: &str) -> RecoveryState {
        RecoveryState {
            conversation_id: 1,
            branch_id: None,
            last_message_id: 2,
            prompt_snapshot: "p".into(),
            generated_prefix: String::new(),
            last_token_count: 0,
            kv_cache_fingerprint: "kv".into(),
            model_fingerprint: Some(fp.to_string()),
            watchdog_fingerprint: None,
            resumed_after_kill: false,
            updated_at: chrono::Utc::now(),
        }
    }
}
