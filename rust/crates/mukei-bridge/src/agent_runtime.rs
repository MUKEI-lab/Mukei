//! Bridge-side wiring of the `mukei-core` agent loop.
//!
//! Audit v2 — Issues #14, #21, #2 (root-cause block).
//!
//! Prior to this module the bridge crate's `MukeiAgentRust::initialize`
//! loaded the config from disk but used it only for a log line, and
//! `send_message` called `engine::llama_wrapper::run_inference` directly,
//! bypassing `AgentLoop::run` entirely. As a result **every** per-turn
//! fix wired into the agent loop (per-turn resets, graceful degrade on
//! parse errors, abuse pre-check, sentinel escaping, watchdog rearm)
//! was correct in the library but 100% unreachable from the running app.
//!
//! This module exposes the missing pieces:
//!
//! 1. `load_config` — load + validate the user's `config.toml`, so the
//!    watchdog / agent / saf sections drive real behaviour.
//! 2. `open_pool` — open the SQLCipher (or plain-SQLite on desktop)
//!    database, run `Migrator::apply_pending`.
//! 3. `build_agent_loop` — construct the shared `Arc<AgentLoop>` from
//!    the loaded config + opened pool + shared `ToolRegistry`.
//! 4. `BridgeContextBackend` — `ContextBackend` impl that loads the
//!    recent `messages` table rows into the prompt history. Without
//!    this, `ContextBudgetManager` would have nothing to read.
//! 5. `CharHeuristicTokens` — `TokenCount` impl that estimates token
//!    counts as `chars / 4`, conservative until a real tokenizer is
//!    wired at boot.
//! 6. `reconcile_vector_store` — boot-time SQL/vector-store consistency
//!    check so orphaned chunks are surfaced before the first RAG query.
//!
//! All SQLite-touching paths are gated behind the `rusqlite` feature so
//! the bridge still compiles cleanly on hosts where persistence is off.

#![cfg_attr(
    not(feature = "rusqlite"),
    allow(dead_code, unused_imports, unused_variables)
)]

use std::path::Path;
use std::sync::Arc;

use mukei_core::agent::{
    context::{ContextBackend, TokenCount},
    AgentEventSink, AgentLoop, ContextBudgetManager, FailureTracker, ToolExecutor, Watchdog,
    WatchdogHandle,
};
use mukei_core::config::MukeiConfig;
use mukei_core::engine::{
    BackendUnavailableReason, InferenceBackend, UnavailableInferenceBackend,
};
use mukei_core::error::Result;
use mukei_core::tools::ToolRegistry;
use mukei_core::types::{BranchId, ChatMessage, ConversationId};

use crate::core_saf;

/// `TokenCount` implementation that approximates tokenisation from raw
/// character length. Over-counting is safer than under-counting because
/// it makes the context budget tighter, not looser.
pub struct CharHeuristicTokens {
    bytes_per_token: usize,
}

impl Default for CharHeuristicTokens {
    fn default() -> Self {
        Self { bytes_per_token: 4 }
    }
}

#[async_trait::async_trait]
impl TokenCount for CharHeuristicTokens {
    async fn count(&self, s: &str) -> usize {
        s.len().div_ceil(self.bytes_per_token.max(1))
    }
}

/// Load + validate user config.
pub fn load_config(path: &Path) -> Result<MukeiConfig> {
    MukeiConfig::load_and_validate(path)
}

/// Open the SQLite / SQLCipher pool and apply migrations.
#[cfg(feature = "rusqlite")]
pub async fn open_pool(
    cfg: &MukeiConfig,
    #[cfg(feature = "sqlcipher")] unwrapped_database_key: zeroize::Zeroizing<Vec<u8>>,
) -> Result<mukei_core::storage::DatabasePool> {
    use mukei_core::storage::{DatabasePool, Migrator};

    #[cfg(feature = "sqlcipher")]
    let open_result =
        DatabasePool::open_with_cipher_key_result(&cfg.database_path, unwrapped_database_key)?;
    #[cfg(feature = "sqlcipher")]
    let encryption_status = open_result.encryption_status;
    #[cfg(feature = "sqlcipher")]
    let pool = open_result.pool;
    #[cfg(not(feature = "sqlcipher"))]
    let pool = DatabasePool::open(&cfg.database_path)?;

    let migrator = Migrator::embedded();
    if let Some(backup) = migrator
        .create_pre_migration_backup(&pool, &cfg.database_path)
        .await?
    {
        tracing::warn!(
            backup_path = %mukei_core::diagnostics::redact_path(&backup.path),
            from_version = backup.from_version,
            to_version = backup.to_version,
            "created encrypted pre-migration database backup"
        );
    }
    migrator.apply_pending(&pool).await?;
    #[cfg(feature = "sqlcipher")]
    tracing::info!(
        db_path = %mukei_core::diagnostics::redact_path(&cfg.database_path),
        encryption_status = ?encryption_status,
        "embedded migrations applied during bridge boot"
    );
    #[cfg(not(feature = "sqlcipher"))]
    tracing::info!(
        db_path = %mukei_core::diagnostics::redact_path(&cfg.database_path),
        "embedded migrations applied during bridge boot"
    );
    Ok(pool)
}

#[cfg(not(feature = "rusqlite"))]
pub async fn open_pool(_cfg: &MukeiConfig) -> Result<()> {
    tracing::warn!("bridge built without rusqlite — DatabasePool disabled, persistence off");
    Ok(())
}

/// Durable sink used by `AgentLoop` for assistant tool-call attempts,
/// validator envelopes, tool results, and supervisor directives. A loop
/// iteration does not advance until the message commit succeeds.
#[cfg(feature = "rusqlite")]
pub struct BridgeTurnPersistence {
    pool: Arc<mukei_core::storage::DatabasePool>,
    turn: mukei_core::storage::PersistedTurn,
}

#[cfg(feature = "rusqlite")]
impl BridgeTurnPersistence {
    pub fn new(
        pool: Arc<mukei_core::storage::DatabasePool>,
        turn: mukei_core::storage::PersistedTurn,
    ) -> Self {
        Self { pool, turn }
    }
}

#[cfg(feature = "rusqlite")]
#[async_trait::async_trait]
impl AgentEventSink for BridgeTurnPersistence {
    async fn persist_intermediate(&self, message: &ChatMessage) -> Result<()> {
        mukei_core::storage::ConversationRepository::append_intermediate_message(
            &self.pool,
            self.turn.clone(),
            message.clone(),
        )
        .await
        .map(|_| ())
    }
}

/// Bridge-side `ContextBackend` that reads `messages` rows out of the
/// opened SQLite pool.
#[cfg(feature = "rusqlite")]
pub struct BridgeContextBackend {
    pool: Arc<mukei_core::storage::DatabasePool>,
    limit: i64,
}

#[cfg(feature = "rusqlite")]
impl BridgeContextBackend {
    pub fn new(pool: Arc<mukei_core::storage::DatabasePool>, limit: i64) -> Self {
        Self { pool, limit }
    }
}

#[cfg(feature = "rusqlite")]
#[async_trait::async_trait]
impl ContextBackend for BridgeContextBackend {
    async fn load_history(
        &self,
        conversation: ConversationId,
        branch: BranchId,
        active_history: &[ChatMessage],
    ) -> Result<Vec<ChatMessage>> {
        use mukei_core::storage::PooledConnectionExt;

        let active_ids: std::collections::HashSet<uuid::Uuid> =
            active_history.iter().map(|message| message.id.0).collect();
        let conversation_external_id = conversation.0.to_string();
        let branch_external_id = branch.0.to_string();
        let conversation_external_id_for_query = conversation_external_id.clone();
        let branch_external_id_for_query = branch_external_id.clone();
        let limit = self.limit;
        let query_limit = limit.saturating_add(i64::try_from(active_ids.len()).unwrap_or(i64::MAX));
        let rows: Vec<(String, String, String, String, Option<String>, i64)> = self
            .pool
            .with_conn(move |c| {
                let mut stmt = c.prepare(
                    "SELECT m.external_id, m.role, m.content, m.created_at, \
                            parent.external_id, m.token_count \
                     FROM messages m \
                     JOIN branches b ON b.id = m.branch_id \
                                    AND b.conversation_id = m.conversation_id \
                     JOIN conversations conv ON conv.id = m.conversation_id \
                     LEFT JOIN messages parent ON parent.id = m.parent_message_id \
                     WHERE conv.external_id = ?1 \
                       AND b.external_id = ?2 \
                       AND m.conversation_id = b.conversation_id \
                       AND m.branch_id = b.id \
                       AND m.deleted = 0 \
                       AND m.status = 'completed' \
                     ORDER BY m.created_at DESC, m.id DESC LIMIT ?3",
                )?;
                let mapped = stmt
                    .query_map(
                        (
                            &conversation_external_id_for_query,
                            &branch_external_id_for_query,
                            query_limit,
                        ),
                        |row| {
                            Ok((
                                row.get(0)?,
                                row.get(1)?,
                                row.get(2)?,
                                row.get(3)?,
                                row.get(4)?,
                                row.get(5)?,
                            ))
                        },
                    )?
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                Ok::<_, mukei_core::storage::DbError>(mapped)
            })
            .await?;

        let mut messages: Vec<ChatMessage> = rows
            .into_iter()
            .rev()
            .filter_map(|(id, role, content, created_at, parent, token_count)| {
                let id = uuid::Uuid::parse_str(&id).ok()?;
                if active_ids.contains(&id) {
                    return None;
                }
                let created_at = chrono::DateTime::parse_from_rfc3339(&created_at)
                    .ok()?
                    .with_timezone(&chrono::Utc);
                let parent = parent
                    .and_then(|value| uuid::Uuid::parse_str(&value).ok())
                    .map(mukei_core::types::MessageId);
                Some(ChatMessage {
                    id: mukei_core::types::MessageId(id),
                    role: parse_role(&role),
                    branch: mukei_core::types::BranchId(
                        uuid::Uuid::parse_str(&branch_external_id).ok()?,
                    ),
                    is_active: true,
                    created_at,
                    content,
                    parent,
                    token_count: u32::try_from(token_count).ok(),
                })
            })
            .collect();
        let limit = usize::try_from(limit.max(0)).unwrap_or(usize::MAX);
        if messages.len() > limit {
            messages.drain(..messages.len().saturating_sub(limit));
        }
        Ok(messages)
    }

    async fn rag_lookup(&self, _q: &str, _k: usize) -> Result<Vec<String>> {
        Ok(Vec::new())
    }
}

#[cfg(not(feature = "rusqlite"))]
pub struct BridgeContextBackend {
    limit: i64,
}

#[cfg(not(feature = "rusqlite"))]
impl BridgeContextBackend {
    pub fn new(limit: i64) -> Self {
        Self { limit }
    }
}

#[cfg(not(feature = "rusqlite"))]
#[async_trait::async_trait]
impl ContextBackend for BridgeContextBackend {
    async fn load_history(
        &self,
        _conversation: ConversationId,
        _branch: BranchId,
        _active_history: &[ChatMessage],
    ) -> Result<Vec<ChatMessage>> {
        let _ = self.limit;
        Ok(Vec::new())
    }

    async fn rag_lookup(&self, _q: &str, _k: usize) -> Result<Vec<String>> {
        Ok(Vec::new())
    }
}

/// Build the shared `Arc<AgentLoop>` from loaded config, rebuilt tool
/// registry, and optional database pool. This compatibility assembly is
/// intentionally fail-closed until a production activation path injects a
/// runnable backend; it never selects the development mock implicitly.
#[cfg(feature = "rusqlite")]
pub fn build_agent_loop(
    cfg: &MukeiConfig,
    registry: Arc<ToolRegistry>,
    pool: Arc<mukei_core::storage::DatabasePool>,
    audit_writer: Arc<mukei_core::storage::AuditLogWriter>,
) -> Arc<AgentLoop> {
    tracing::warn!(
        backend_kind = "unavailable",
        "agent runtime built without an activated production inference backend"
    );
    build_agent_loop_with_backend(
        cfg,
        registry,
        pool,
        audit_writer,
        Arc::new(UnavailableInferenceBackend::new_with_reason(
            "production_backend_not_activated",
            BackendUnavailableReason::NotInjected,
        )),
    )
}

/// Production assembly boundary. A caller that owns model activation injects
/// the authoritative active backend (typically `ModelActivationService`) here.
#[cfg(feature = "rusqlite")]
pub fn build_agent_loop_with_backend(
    cfg: &MukeiConfig,
    registry: Arc<ToolRegistry>,
    pool: Arc<mukei_core::storage::DatabasePool>,
    audit_writer: Arc<mukei_core::storage::AuditLogWriter>,
    inference_backend: Arc<dyn InferenceBackend>,
) -> Arc<AgentLoop> {
    let backend = Arc::new(BridgeContextBackend::new(
        pool.clone(),
        cfg.agent.recovered_history_window as i64,
    ));
    let tokenizer = Arc::new(CharHeuristicTokens::default());
    let context = ContextBudgetManager::new(backend, tokenizer, cfg.n_ctx);

    let policy = mukei_core::agent::tools::ToolExecutionPolicy::from(&cfg.agent);
    let tracker = Arc::new(FailureTracker::new());
    let executor =
        ToolExecutor::with_policy_and_audit(registry, tracker, policy, pool, audit_writer);

    let watchdog = WatchdogHandle::new(Watchdog::new(
        cfg.watchdog.max_iterations,
        cfg.watchdog.max_token_budget,
        std::time::Duration::from_secs(cfg.watchdog.max_wall_seconds),
    ));

    AgentLoop::new_with_backend(context, executor, watchdog, inference_backend)
}

#[cfg(not(feature = "rusqlite"))]
pub fn build_agent_loop(cfg: &MukeiConfig, registry: Arc<ToolRegistry>) -> Arc<AgentLoop> {
    tracing::warn!(
        backend_kind = "unavailable",
        "agent runtime built without an activated production inference backend"
    );
    build_agent_loop_with_backend(
        cfg,
        registry,
        Arc::new(UnavailableInferenceBackend::new_with_reason(
            "production_backend_not_activated",
            BackendUnavailableReason::NotInjected,
        )),
    )
}

#[cfg(not(feature = "rusqlite"))]
pub fn build_agent_loop_with_backend(
    cfg: &MukeiConfig,
    registry: Arc<ToolRegistry>,
    inference_backend: Arc<dyn InferenceBackend>,
) -> Arc<AgentLoop> {
    let backend = Arc::new(BridgeContextBackend::new(
        cfg.agent.recovered_history_window as i64,
    ));
    let tokenizer = Arc::new(CharHeuristicTokens::default());
    let context = ContextBudgetManager::new(backend, tokenizer, cfg.n_ctx);

    let policy = mukei_core::agent::tools::ToolExecutionPolicy::from(&cfg.agent);
    let tracker = Arc::new(FailureTracker::new());
    let executor = ToolExecutor::with_policy(registry, tracker, policy);

    let watchdog = WatchdogHandle::new(Watchdog::new(
        cfg.watchdog.max_iterations,
        cfg.watchdog.max_token_budget,
        std::time::Duration::from_secs(cfg.watchdog.max_wall_seconds),
    ));

    AgentLoop::new_with_backend(context, executor, watchdog, inference_backend)
}

/// Hydrate the global SAF registry from disk.
#[cfg(feature = "rusqlite")]
pub async fn hydrate_saf_registry(
    saf: &core_saf::SafRegistry,
    pool: &mukei_core::storage::DatabasePool,
) -> Result<usize> {
    saf.hydrate_from_pool(pool).await
}

#[cfg(not(feature = "rusqlite"))]
pub async fn hydrate_saf_registry(_saf: &core_saf::SafRegistry, _pool: &()) -> Result<usize> {
    Ok(0)
}

/// Boot-time reconciliation of persisted `chunks` rows vs the vector-store
/// snapshot on disk (Issue #11). Best-effort: callers log the report and
/// continue booting even if reconciliation fails.
#[cfg(feature = "rusqlite")]
pub async fn reconcile_vector_store(
    cfg: &MukeiConfig,
    pool: &mukei_core::storage::DatabasePool,
) -> Result<mukei_core::rag::indexer::ReconciliationReport> {
    use mukei_core::error::MukeiError;
    use mukei_core::rag::VectorStore;

    let vectors_dir = cfg.vectors_dir.clone();
    tokio::task::spawn_blocking(move || std::fs::create_dir_all(&vectors_dir))
        .await
        .map_err(|e| MukeiError::BlockingJoinFailed(e.to_string()))?
        .map_err(|e| MukeiError::Io(e.to_string()))?;

    let store_path = cfg.vectors_dir.join("mukei.usearch");
    let store = tokio::task::spawn_blocking(move || {
        let store = VectorStore::open(store_path);
        store.load()?;
        Ok::<_, MukeiError>(store)
    })
    .await
    .map_err(|e| MukeiError::BlockingJoinFailed(e.to_string()))??;

    mukei_core::rag::indexer::reconcile(pool, &store).await
}

#[cfg(feature = "rusqlite")]
pub async fn purge_vector_chunks(cfg: &MukeiConfig, chunk_ids: Vec<u64>) -> Result<usize> {
    use mukei_core::error::MukeiError;
    use mukei_core::rag::VectorStore;

    if chunk_ids.is_empty() {
        return Ok(0);
    }
    let store_path = cfg.vectors_dir.join("mukei.usearch");
    tokio::task::spawn_blocking(move || {
        let store = VectorStore::open(store_path);
        store.load()?;
        let removed = store.shred_many(&chunk_ids);
        store.save()?;
        Ok::<_, MukeiError>(removed)
    })
    .await
    .map_err(|e| MukeiError::BlockingJoinFailed(e.to_string()))?
}

/// Retry every SQL-committed document deletion whose vector cleanup did
/// not finish before a crash or I/O failure. Failed plans remain pending
/// with a redacted error and are retried on the next boot.
#[cfg(feature = "rusqlite")]
pub async fn drain_pending_document_cleanups(
    cfg: &MukeiConfig,
    pool: &mukei_core::storage::DatabasePool,
) -> Result<(usize, usize)> {
    let plans = core_saf::SafRegistry::pending_document_cleanups(pool).await?;
    let mut completed = 0usize;
    let mut failed = 0usize;
    for plan in plans {
        match purge_vector_chunks(cfg, plan.chunk_ids.clone()).await {
            Ok(_) => {
                core_saf::SafRegistry::mark_document_cleanup_complete(pool, &plan.file_token)
                    .await?;
                completed += 1;
            }
            Err(error) => {
                core_saf::SafRegistry::mark_document_cleanup_failed(pool, &plan.file_token, &error)
                    .await?;
                failed += 1;
                tracing::warn!(
                    token_fingerprint = %mukei_core::agent::FailureTracker::fingerprint(
                        "document_cleanup",
                        &serde_json::json!({"token": &plan.file_token}),
                    ),
                    code = error.error_code(),
                    "document vector cleanup remains pending"
                );
            }
        }
    }
    Ok((completed, failed))
}

#[cfg(not(feature = "rusqlite"))]
pub async fn drain_pending_document_cleanups(
    _cfg: &MukeiConfig,
    _pool: &(),
) -> Result<(usize, usize)> {
    Ok((0, 0))
}

#[cfg(not(feature = "rusqlite"))]
pub async fn reconcile_vector_store(_cfg: &MukeiConfig, _pool: &()) -> Result<()> {
    Ok(())
}

/// Convert SQL `role` values into the closed Rust enum.
fn parse_role(value: &str) -> mukei_core::types::Role {
    use mukei_core::types::Role;
    match value {
        "system" => Role::System,
        "user" => Role::User,
        "assistant" => Role::Assistant,
        "tool" => Role::Tool,
        "red_team" => Role::RedTeam,
        _ => Role::System,
    }
}

#[cfg(all(test, feature = "rusqlite"))]
mod tests {
    use super::*;
    use mukei_core::storage::{ConversationRepository, DatabasePool, Migrator};
    use mukei_core::types::{MessageId, Role};

    async fn migrated_pool() -> Arc<DatabasePool> {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("bridge-history.db");
        let pool = DatabasePool::open(&db_path).unwrap();
        Migrator::embedded().apply_pending(&pool).await.unwrap();
        std::mem::forget(dir);
        Arc::new(pool)
    }

    async fn completed_turn(
        pool: &DatabasePool,
        conversation: ConversationId,
        branch: BranchId,
        user_message_id: MessageId,
        assistant_message_id: MessageId,
        user: &str,
        assistant: &str,
    ) {
        let turn = ConversationRepository::begin_turn(
            pool,
            conversation,
            branch,
            user_message_id,
            assistant_message_id,
            user.to_string(),
        )
        .await
        .unwrap();
        ConversationRepository::complete_turn(pool, turn, assistant.to_string())
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn load_history_scopes_by_conversation_and_branch_without_duplicates() {
        let pool = migrated_pool().await;
        let backend = BridgeContextBackend::new(pool.clone(), 32);

        let conversation_a = ConversationId::new();
        let conversation_b = ConversationId::new();
        let branch_a1 = BranchId::new();
        let branch_a2 = BranchId::new();
        let branch_b1 = BranchId::new();

        let a1_user_1 = MessageId::new();
        let a1_assistant_1 = MessageId::new();
        completed_turn(
            &pool,
            conversation_a,
            branch_a1,
            a1_user_1,
            a1_assistant_1,
            "a1 user 1",
            "a1 assistant 1",
        )
        .await;
        completed_turn(
            &pool,
            conversation_a,
            branch_a1,
            MessageId::new(),
            MessageId::new(),
            "a1 user 2",
            "a1 assistant 2",
        )
        .await;
        completed_turn(
            &pool,
            conversation_a,
            branch_a2,
            MessageId::new(),
            MessageId::new(),
            "a2 user 1",
            "a2 assistant 1",
        )
        .await;
        completed_turn(
            &pool,
            conversation_b,
            branch_b1,
            MessageId::new(),
            MessageId::new(),
            "b1 user 1",
            "b1 assistant 1",
        )
        .await;

        let active_history = vec![ChatMessage::user_with_id(a1_user_1, branch_a1, "a1 user 1")];
        let loaded = backend
            .load_history(conversation_a, branch_a1, &active_history)
            .await
            .unwrap();

        assert_eq!(loaded.len(), 3, "active prompt must not be injected twice");
        assert_eq!(
            loaded
                .iter()
                .map(|message| message.content.as_str())
                .collect::<Vec<_>>(),
            vec!["a1 assistant 1", "a1 user 2", "a1 assistant 2"]
        );
        assert!(loaded.iter().all(|message| message.branch == branch_a1));
        assert!(loaded.iter().all(|message| {
            !matches!(
                message.content.as_str(),
                "a2 user 1" | "a2 assistant 1" | "b1 user 1" | "b1 assistant 1"
            )
        }));
        assert_eq!(loaded[0].role, Role::Assistant);
        assert_eq!(loaded[1].role, Role::User);
        assert_eq!(loaded[2].role, Role::Assistant);
        assert!(
            loaded
                .windows(2)
                .all(|pair| pair[0].created_at <= pair[1].created_at),
            "history order must be deterministic and oldest-first"
        );
        assert_eq!(loaded[0].parent, Some(a1_user_1));
    }

    #[tokio::test]
    async fn load_history_requires_matching_conversation_for_branch() {
        let pool = migrated_pool().await;
        let backend = BridgeContextBackend::new(pool.clone(), 32);

        let conversation_a = ConversationId::new();
        let conversation_b = ConversationId::new();
        let branch_a = BranchId::new();
        let branch_b = BranchId::new();

        completed_turn(
            &pool,
            conversation_a,
            branch_a,
            MessageId::new(),
            MessageId::new(),
            "a user",
            "a assistant",
        )
        .await;
        completed_turn(
            &pool,
            conversation_b,
            branch_b,
            MessageId::new(),
            MessageId::new(),
            "b user",
            "b assistant",
        )
        .await;

        let wrong_pair = backend
            .load_history(conversation_a, branch_b, &[])
            .await
            .unwrap();
        assert!(
            wrong_pair.is_empty(),
            "branch identifier alone must not cross conversation boundaries"
        );
    }
}
