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
    AgentLoop, ContextBudgetManager, FailureTracker, ToolExecutor, Watchdog, WatchdogHandle,
};
use mukei_core::config::MukeiConfig;
use mukei_core::error::Result;
use mukei_core::tools::ToolRegistry;
use mukei_core::types::ChatMessage;

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
    #[cfg(feature = "sqlcipher")] unwrapped_database_key: Vec<u8>,
) -> Result<mukei_core::storage::DatabasePool> {
    use mukei_core::storage::{DatabasePool, Migrator, MIGRATIONS_DIR};

    #[cfg(feature = "sqlcipher")]
    let open_result =
        DatabasePool::open_with_cipher_key_result(&cfg.database_path, unwrapped_database_key)?;
    #[cfg(feature = "sqlcipher")]
    let encryption_status = open_result.encryption_status;
    #[cfg(feature = "sqlcipher")]
    let pool = open_result.pool;
    #[cfg(not(feature = "sqlcipher"))]
    let pool = DatabasePool::open(&cfg.database_path)?;

    let migrations_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../")
        .join(MIGRATIONS_DIR);
    let migrator = Migrator::new(&migrations_dir);
    migrator.apply_pending(&pool).await?;
    #[cfg(feature = "sqlcipher")]
    tracing::info!(
        migrations_dir = %mukei_core::diagnostics::redact_path(&migrations_dir),
        db_path = %mukei_core::diagnostics::redact_path(&cfg.database_path),
        encryption_status = ?encryption_status,
        "migrations applied during bridge boot"
    );
    #[cfg(not(feature = "sqlcipher"))]
    tracing::info!(
        migrations_dir = %mukei_core::diagnostics::redact_path(&migrations_dir),
        db_path = %mukei_core::diagnostics::redact_path(&cfg.database_path),
        "migrations applied during bridge boot"
    );
    Ok(pool)
}

#[cfg(not(feature = "rusqlite"))]
pub async fn open_pool(_cfg: &MukeiConfig) -> Result<()> {
    tracing::warn!("bridge built without rusqlite — DatabasePool disabled, persistence off");
    Ok(())
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
    async fn load_history(&self) -> Result<Vec<ChatMessage>> {
        use mukei_core::storage::PooledConnectionExt;

        let limit = self.limit;
        let rows: Vec<(String, String)> = self
            .pool
            .with_conn(move |c| {
                let mut stmt = c.prepare(
                    "SELECT role, content FROM messages \
                     WHERE deleted = 0 \
                     ORDER BY id DESC LIMIT ?1",
                )?;
                let mapped = stmt
                    .query_map([limit], |row| {
                        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                    })?
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                Ok::<_, mukei_core::storage::DbError>(mapped)
            })
            .await?;

        let messages: Vec<ChatMessage> = rows
            .into_iter()
            .rev()
            .map(|(role, content)| ChatMessage {
                id: mukei_core::types::MessageId::default(),
                role: parse_role(&role),
                branch: mukei_core::types::BranchId::default(),
                is_active: true,
                created_at: chrono::Utc::now(),
                content,
                parent: None,
                token_count: None,
            })
            .collect();
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
    async fn load_history(&self) -> Result<Vec<ChatMessage>> {
        let _ = self.limit;
        Ok(Vec::new())
    }

    async fn rag_lookup(&self, _q: &str, _k: usize) -> Result<Vec<String>> {
        Ok(Vec::new())
    }
}

/// Build the shared `Arc<AgentLoop>` from loaded config, rebuilt tool
/// registry, and optional database pool.
#[cfg(feature = "rusqlite")]
pub fn build_agent_loop(
    cfg: &MukeiConfig,
    registry: Arc<ToolRegistry>,
    pool: Arc<mukei_core::storage::DatabasePool>,
    audit_writer: Arc<mukei_core::storage::AuditLogWriter>,
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

    AgentLoop::new(context, executor, watchdog)
}

#[cfg(not(feature = "rusqlite"))]
pub fn build_agent_loop(cfg: &MukeiConfig, registry: Arc<ToolRegistry>) -> Arc<AgentLoop> {
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

    AgentLoop::new(context, executor, watchdog)
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
