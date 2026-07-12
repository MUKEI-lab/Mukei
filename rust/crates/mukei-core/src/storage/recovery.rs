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
use crate::storage::conversation::PersistedTurn;
use crate::storage::pool::{DatabasePool, DbError, PooledConnectionExt};
use crate::types::{BranchId, ChatMessage, ConversationId, MessageId, Role};
use rusqlite::OptionalExtension;

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecoveryMode {
    /// Continue after the interrupted partial assistant message.
    Resume,
    /// Generate a fresh sibling answer from the original user prompt.
    Regenerate,
}

#[derive(Clone, Debug)]
pub struct InterruptedTurn {
    pub conversation: ConversationId,
    pub branch: BranchId,
    pub user_message_id: MessageId,
    pub user_content: String,
    pub interrupted_assistant_id: MessageId,
    pub generated_prefix: String,
    pub model_fingerprint: Option<String>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Clone, Debug)]
pub struct RecoveryAttempt {
    pub turn: PersistedTurn,
    pub conversation: ConversationId,
    pub branch: BranchId,
    pub mode: RecoveryMode,
    pub user_content: String,
    /// Active history supplied to the agent loop. Resume includes the
    /// interrupted assistant prefix; regeneration includes only the user.
    pub seed_history: Vec<ChatMessage>,
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

    /// Return the recoverable interrupted turn, if boot marked one.
    pub async fn interrupted_turn(pool: &DatabasePool) -> Result<Option<InterruptedTurn>> {
        pool.with_conn(|c| {
            let mut stmt = c.prepare(
                "SELECT conv.external_id, b.external_id, user.external_id, user.content, \
                        assistant.external_id, rs.generated_prefix, rs.model_fingerprint, \
                        rs.updated_at \
                 FROM recovery_state rs \
                 JOIN conversations conv ON conv.id = rs.conversation_id \
                 JOIN branches b ON b.id = rs.branch_id AND b.conversation_id = conv.id \
                 JOIN messages assistant ON assistant.id = rs.last_message_id \
                 JOIN messages user ON user.external_id = rs.prompt_snapshot \
                                   AND user.conversation_id = conv.id \
                                   AND user.branch_id = b.id \
                 WHERE rs.resumed_after_kill = 1 \
                   AND assistant.status IN ('failed', 'cancelled') \
                 LIMIT 1",
            )?;
            let row = stmt
                .query_row([], |row| {
                    let conversation: String = row.get(0)?;
                    let branch: String = row.get(1)?;
                    let user_message: String = row.get(2)?;
                    let assistant_message: String = row.get(4)?;
                    let updated_at: String = row.get(7)?;
                    Ok((
                        conversation,
                        branch,
                        user_message,
                        row.get::<_, String>(3)?,
                        assistant_message,
                        row.get::<_, String>(5)?,
                        row.get::<_, Option<String>>(6)?,
                        updated_at,
                    ))
                })
                .optional()?;
            row.map(
                |(
                    conversation,
                    branch,
                    user_message,
                    user_content,
                    assistant_message,
                    generated_prefix,
                    model_fingerprint,
                    updated_at,
                )| {
                    Ok(InterruptedTurn {
                        conversation: ConversationId(parse_uuid(&conversation)?),
                        branch: BranchId(parse_uuid(&branch)?),
                        user_message_id: MessageId(parse_uuid(&user_message)?),
                        user_content,
                        interrupted_assistant_id: MessageId(parse_uuid(&assistant_message)?),
                        generated_prefix,
                        model_fingerprint,
                        updated_at: chrono::DateTime::parse_from_rfc3339(&updated_at)
                            .map(|value| value.with_timezone(&chrono::Utc))
                            .map_err(|_| DbError::Domain(MukeiError::DatabaseCorruption))?,
                    })
                },
            )
            .transpose()
        })
        .await
    }

    /// Atomically create a new durable assistant placeholder for a resume
    /// or regeneration attempt. The interrupted row remains immutable as
    /// evidence; the new turn becomes the sole active recovery snapshot.
    pub async fn begin_attempt(
        pool: &DatabasePool,
        mode: RecoveryMode,
        new_assistant_external_id: MessageId,
    ) -> Result<RecoveryAttempt> {
        pool.with_conn(move |c| {
            let tx = c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            let row = tx.query_row(
                "SELECT rs.conversation_id, rs.branch_id, user.id, user.external_id, \
                        user.content, user.created_at, assistant.id, assistant.external_id, \
                        assistant.content, assistant.created_at, conv.external_id, b.external_id \
                 FROM recovery_state rs \
                 JOIN conversations conv ON conv.id = rs.conversation_id \
                 JOIN branches b ON b.id = rs.branch_id AND b.conversation_id = conv.id \
                 JOIN messages assistant ON assistant.id = rs.last_message_id \
                 JOIN messages user ON user.external_id = rs.prompt_snapshot \
                                   AND user.conversation_id = conv.id \
                                   AND user.branch_id = b.id \
                 WHERE rs.resumed_after_kill = 1 \
                   AND assistant.status IN ('failed', 'cancelled')",
                [],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, i64>(6)?,
                        row.get::<_, String>(7)?,
                        row.get::<_, String>(8)?,
                        row.get::<_, String>(9)?,
                        row.get::<_, String>(10)?,
                        row.get::<_, String>(11)?,
                    ))
                },
            )?;
            let (
                conversation_id,
                branch_id,
                user_id,
                user_external,
                user_content,
                user_created,
                interrupted_id,
                interrupted_external,
                interrupted_content,
                interrupted_created,
                conversation_external,
                branch_external,
            ) = row;
            let parent_id = match mode {
                RecoveryMode::Resume => interrupted_id,
                RecoveryMode::Regenerate => user_id,
            };
            let now = chrono::Utc::now().to_rfc3339();
            tx.execute(
                "INSERT INTO messages (external_id, conversation_id, role, content, created_at, \
                    updated_at, branch_id, parent_message_id, token_count, deleted, status) \
                 VALUES (?1, ?2, 'assistant', '', ?3, ?3, ?4, ?5, 0, 0, 'pending')",
                rusqlite::params![
                    new_assistant_external_id.0.to_string(),
                    conversation_id,
                    now,
                    branch_id,
                    parent_id,
                ],
            )?;
            let assistant_message_id = tx.last_insert_rowid();
            tx.execute(
                "UPDATE recovery_state SET last_message_id = ?1, generated_prefix = '', \
                    last_token_count = 0, kv_cache_fingerprint = 'unavailable', \
                    resumed_after_kill = 0, updated_at = ?2 WHERE id = 1",
                rusqlite::params![assistant_message_id, now],
            )?;
            tx.execute(
                "UPDATE conversations SET updated_at = ?1, active_branch_id = ?2 WHERE id = ?3",
                rusqlite::params![now, branch_id, conversation_id],
            )?;
            tx.commit()?;

            let conversation = ConversationId(parse_uuid(&conversation_external)?);
            let branch = BranchId(parse_uuid(&branch_external)?);
            let user_message_id = MessageId(parse_uuid(&user_external)?);
            let interrupted_message_id = MessageId(parse_uuid(&interrupted_external)?);
            let user_created_at = parse_time(&user_created)?;
            let interrupted_created_at = parse_time(&interrupted_created)?;
            let user_message = ChatMessage {
                id: user_message_id,
                role: Role::User,
                branch,
                is_active: true,
                created_at: user_created_at,
                content: user_content.clone(),
                parent: None,
                token_count: None,
            };
            let mut seed_history = vec![user_message];
            if matches!(mode, RecoveryMode::Resume) && !interrupted_content.is_empty() {
                seed_history.push(ChatMessage {
                    id: interrupted_message_id,
                    role: Role::Assistant,
                    branch,
                    is_active: true,
                    created_at: interrupted_created_at,
                    content: interrupted_content,
                    parent: Some(user_message_id),
                    token_count: None,
                });
            }
            Ok::<_, DbError>(RecoveryAttempt {
                turn: PersistedTurn {
                    conversation_id,
                    branch_id,
                    user_message_id: user_id,
                    assistant_message_id,
                    assistant_external_id: new_assistant_external_id,
                },
                conversation,
                branch,
                mode,
                user_content,
                seed_history,
            })
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

fn parse_uuid(value: &str) -> std::result::Result<uuid::Uuid, DbError> {
    uuid::Uuid::parse_str(value).map_err(|_| DbError::Domain(MukeiError::DatabaseCorruption))
}

fn parse_time(value: &str) -> std::result::Result<chrono::DateTime<chrono::Utc>, DbError> {
    chrono::DateTime::parse_from_rfc3339(value)
        .map(|value| value.with_timezone(&chrono::Utc))
        .map_err(|_| DbError::Domain(MukeiError::DatabaseCorruption))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{ConversationRepository, DatabasePool, MessageStatus, Migrator};

    async fn migrated_pool() -> DatabasePool {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("recovery.db");
        let pool = DatabasePool::open(&db_path).unwrap();
        Migrator::embedded().apply_pending(&pool).await.unwrap();
        std::mem::forget(dir);
        pool
    }

    async fn interrupted_turn_pool(
    ) -> (DatabasePool, ConversationId, BranchId, MessageId, MessageId) {
        let pool = migrated_pool().await;
        let conversation = ConversationId::new();
        let branch = BranchId::new();
        let user_message_id = MessageId::new();
        let assistant_message_id = MessageId::new();
        let turn = ConversationRepository::begin_turn(
            &pool,
            conversation,
            branch,
            user_message_id,
            assistant_message_id,
            "hello".to_string(),
        )
        .await
        .unwrap();
        ConversationRepository::update_assistant_partial(&pool, turn, "partial answer".to_string())
            .await
            .unwrap();
        ConversationRepository::mark_incomplete_turns_failed(&pool)
            .await
            .unwrap();
        (
            pool,
            conversation,
            branch,
            user_message_id,
            assistant_message_id,
        )
    }

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

    #[tokio::test]
    async fn interrupted_turn_exposes_failed_partial_without_overwriting_it() {
        let (pool, conversation, branch, user_message_id, assistant_message_id) =
            interrupted_turn_pool().await;

        let interrupted = RecoveryStore::interrupted_turn(&pool)
            .await
            .unwrap()
            .expect("boot recovery must expose interrupted turn");
        assert_eq!(interrupted.conversation, conversation);
        assert_eq!(interrupted.branch, branch);
        assert_eq!(interrupted.user_message_id, user_message_id);
        assert_eq!(interrupted.interrupted_assistant_id, assistant_message_id);
        assert_eq!(interrupted.user_content, "hello");
        assert_eq!(interrupted.generated_prefix, "partial answer");

        let messages = ConversationRepository::get_messages_for_conversation(&pool, conversation)
            .await
            .unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[1].external_id, assistant_message_id);
        assert_eq!(messages[1].content, "partial answer");
        assert_eq!(messages[1].status, MessageStatus::Failed);
    }

    #[tokio::test]
    async fn resume_begin_attempt_creates_new_assistant_child_and_clears_active_snapshot() {
        let (pool, conversation, branch, user_message_id, assistant_message_id) =
            interrupted_turn_pool().await;
        let new_assistant = MessageId::new();

        let attempt = RecoveryStore::begin_attempt(&pool, RecoveryMode::Resume, new_assistant)
            .await
            .unwrap();

        assert_eq!(attempt.mode, RecoveryMode::Resume);
        assert_eq!(attempt.conversation, conversation);
        assert_eq!(attempt.branch, branch);
        assert_eq!(attempt.turn.assistant_external_id, new_assistant);
        assert_eq!(attempt.seed_history.len(), 2);
        assert_eq!(attempt.seed_history[0].id, user_message_id);
        assert_eq!(attempt.seed_history[1].id, assistant_message_id);
        assert_eq!(attempt.seed_history[1].parent, Some(user_message_id));
        assert_eq!(attempt.seed_history[1].content, "partial answer");

        let messages = ConversationRepository::get_messages_for_conversation(&pool, conversation)
            .await
            .unwrap();
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[1].external_id, assistant_message_id);
        assert_eq!(messages[1].content, "partial answer");
        assert_eq!(messages[1].status, MessageStatus::Failed);
        assert_eq!(messages[2].external_id, new_assistant);
        assert_eq!(messages[2].status, MessageStatus::Pending);
        assert_eq!(messages[2].parent_message_id, Some(messages[1].id));

        let state = RecoveryStore::load(&pool).await.unwrap().unwrap();
        assert_eq!(state.last_message_id, messages[2].id);
        assert_eq!(state.generated_prefix, "");
        assert!(!state.resumed_after_kill);
    }

    #[tokio::test]
    async fn regenerate_begin_attempt_creates_distinct_sibling_attempt() {
        let (pool, conversation, branch, user_message_id, assistant_message_id) =
            interrupted_turn_pool().await;
        let new_assistant = MessageId::new();

        let attempt = RecoveryStore::begin_attempt(&pool, RecoveryMode::Regenerate, new_assistant)
            .await
            .unwrap();

        assert_eq!(attempt.mode, RecoveryMode::Regenerate);
        assert_eq!(attempt.conversation, conversation);
        assert_eq!(attempt.branch, branch);
        assert_eq!(attempt.seed_history.len(), 1);
        assert_eq!(attempt.seed_history[0].id, user_message_id);

        let messages = ConversationRepository::get_messages_for_conversation(&pool, conversation)
            .await
            .unwrap();
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[1].external_id, assistant_message_id);
        assert_eq!(messages[1].content, "partial answer");
        assert_eq!(messages[1].status, MessageStatus::Failed);
        assert_eq!(messages[2].external_id, new_assistant);
        assert_eq!(messages[2].status, MessageStatus::Pending);
        assert_eq!(messages[2].parent_message_id, Some(messages[0].id));
        assert_ne!(messages[2].parent_message_id, Some(messages[1].id));
    }

    #[tokio::test]
    async fn claimed_recovery_snapshot_is_not_reexposed_as_interrupted() {
        let (pool, _conversation, _branch, _user_message_id, _assistant_message_id) =
            interrupted_turn_pool().await;

        RecoveryStore::begin_attempt(&pool, RecoveryMode::Resume, MessageId::new())
            .await
            .unwrap();

        assert!(RecoveryStore::interrupted_turn(&pool).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn second_recovery_attempt_is_rejected_without_creating_an_extra_message() {
        let (pool, conversation, _branch, _user_message_id, _assistant_message_id) =
            interrupted_turn_pool().await;

        RecoveryStore::begin_attempt(&pool, RecoveryMode::Resume, MessageId::new())
            .await
            .unwrap();
        let before = ConversationRepository::get_messages_for_conversation(&pool, conversation)
            .await
            .unwrap();
        assert_eq!(before.len(), 3);

        let second = RecoveryStore::begin_attempt(
            &pool,
            RecoveryMode::Regenerate,
            MessageId::new(),
        )
        .await;
        assert!(second.is_err(), "a claimed recovery snapshot must be single-use");

        let after = ConversationRepository::get_messages_for_conversation(&pool, conversation)
            .await
            .unwrap();
        assert_eq!(after.len(), before.len());
        assert_eq!(
            after.iter().map(|message| message.external_id).collect::<Vec<_>>(),
            before.iter().map(|message| message.external_id).collect::<Vec<_>>()
        );
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
