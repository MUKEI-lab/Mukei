//! Repository for durable conversations, messages, and active turn state.
//!
//! This module keeps chat persistence behind the storage layer instead of
//! letting the bridge or agent loop issue ad-hoc SQL. Every method uses
//! [`PooledConnectionExt::with_conn`] so SQLite work stays on the blocking
//! pool.

use crate::error::Result;
use crate::storage::pool::{DatabasePool, DbError, PooledConnectionExt};
use crate::types::{BranchId, ConversationId, MessageId, Role};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum MessageStatus {
    Pending,
    Streaming,
    Completed,
    Failed,
    Cancelled,
}

impl MessageStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Streaming => "streaming",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    fn from_sql(value: &str) -> Self {
        match value {
            "pending" => Self::Pending,
            "streaming" => Self::Streaming,
            "completed" => Self::Completed,
            "failed" => Self::Failed,
            "cancelled" => Self::Cancelled,
            _ => Self::Failed,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConversationRecord {
    pub id: i64,
    pub external_id: ConversationId,
    pub title: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub archived: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MessageRecord {
    pub id: i64,
    pub external_id: MessageId,
    pub conversation_id: i64,
    pub role: Role,
    pub content: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub branch_id: Option<i64>,
    pub parent_message_id: Option<i64>,
    pub token_count: u32,
    pub status: MessageStatus,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PersistedTurn {
    pub conversation_id: i64,
    pub branch_id: i64,
    pub user_message_id: i64,
    pub assistant_message_id: i64,
    pub assistant_external_id: MessageId,
}

pub struct ConversationRepository;

impl ConversationRepository {
    pub async fn create_conversation(
        pool: &DatabasePool,
        external_id: ConversationId,
        title: impl Into<String>,
    ) -> Result<ConversationRecord> {
        let title = title.into();
        pool.with_conn(move |c| {
            let now = chrono::Utc::now().to_rfc3339();
            c.execute(
                "INSERT OR IGNORE INTO conversations \
                    (external_id, title, created_at, updated_at, archived) \
                 VALUES (?1, ?2, ?3, ?3, 0)",
                rusqlite::params![external_id.0.to_string(), title, now],
            )?;
            get_conversation_by_external_id(c, external_id)
        })
        .await
    }

    pub async fn get_conversation(
        pool: &DatabasePool,
        external_id: ConversationId,
    ) -> Result<Option<ConversationRecord>> {
        pool.with_conn(
            move |c| match get_conversation_by_external_id(c, external_id) {
                Ok(row) => Ok(Some(row)),
                Err(DbError::Sqlite(rusqlite::Error::QueryReturnedNoRows)) => Ok(None),
                Err(e) => Err(e),
            },
        )
        .await
    }

    pub async fn list_conversations(pool: &DatabasePool) -> Result<Vec<ConversationRecord>> {
        pool.with_conn(|c| {
            let mut stmt = c.prepare(
                "SELECT id, external_id, title, created_at, updated_at, archived \
                 FROM conversations \
                 WHERE archived = 0 \
                 ORDER BY updated_at DESC, id DESC",
            )?;
            let rows = stmt
                .query_map([], map_conversation_row)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok::<_, DbError>(rows)
        })
        .await
    }

    pub async fn begin_turn(
        pool: &DatabasePool,
        conversation_external_id: ConversationId,
        branch_external_id: BranchId,
        user_external_id: MessageId,
        assistant_external_id: MessageId,
        user_content: String,
    ) -> Result<PersistedTurn> {
        pool.with_conn(move |c| {
            let tx = c.transaction()?;
            let now = chrono::Utc::now().to_rfc3339();
            tx.execute(
                "INSERT OR IGNORE INTO conversations \
                    (external_id, title, created_at, updated_at, archived) \
                 VALUES (?1, '', ?2, ?2, 0)",
                rusqlite::params![conversation_external_id.0.to_string(), now],
            )?;
            let conversation_id: i64 = tx.query_row(
                "SELECT id FROM conversations WHERE external_id = ?1",
                [conversation_external_id.0.to_string()],
                |row| row.get(0),
            )?;

            tx.execute(
                "INSERT OR IGNORE INTO branches \
                    (external_id, conversation_id, title, created_at, updated_at, is_active) \
                 VALUES (?1, ?2, '', ?3, ?3, 1)",
                rusqlite::params![branch_external_id.0.to_string(), conversation_id, now],
            )?;
            let branch_id: i64 = tx.query_row(
                "SELECT id FROM branches WHERE external_id = ?1",
                [branch_external_id.0.to_string()],
                |row| row.get(0),
            )?;
            let parent_message_id: Option<i64> = tx
                .query_row(
                    "SELECT id FROM messages \
                     WHERE conversation_id = ?1 AND branch_id = ?2 AND deleted = 0 \
                     ORDER BY created_at DESC, id DESC LIMIT 1",
                    rusqlite::params![conversation_id, branch_id],
                    |row| row.get(0),
                )
                .ok();

            tx.execute(
                "INSERT INTO messages \
                    (external_id, conversation_id, role, content, created_at, updated_at, \
                     branch_id, parent_message_id, token_count, deleted, status) \
                 VALUES (?1, ?2, 'user', ?3, ?4, ?4, ?5, ?6, 0, 0, 'completed')",
                rusqlite::params![
                    user_external_id.0.to_string(),
                    conversation_id,
                    user_content,
                    now,
                    branch_id,
                    parent_message_id,
                ],
            )?;
            let user_message_id = tx.last_insert_rowid();

            tx.execute(
                "INSERT INTO messages \
                    (external_id, conversation_id, role, content, created_at, updated_at, \
                     branch_id, parent_message_id, token_count, deleted, status) \
                 VALUES (?1, ?2, 'assistant', '', ?3, ?3, ?4, ?5, 0, 0, 'pending')",
                rusqlite::params![
                    assistant_external_id.0.to_string(),
                    conversation_id,
                    now,
                    branch_id,
                    user_message_id,
                ],
            )?;
            let assistant_message_id = tx.last_insert_rowid();

            tx.execute(
                "UPDATE conversations \
                 SET updated_at = ?1, active_branch_id = ?2 \
                 WHERE id = ?3",
                rusqlite::params![now, branch_id, conversation_id],
            )?;

            tx.execute(
                "INSERT INTO recovery_state \
                    (id, conversation_id, branch_id, last_message_id, prompt_snapshot, \
                     generated_prefix, last_token_count, kv_cache_fingerprint, \
                     model_fingerprint, watchdog_fingerprint, resumed_after_kill, updated_at) \
                 VALUES (1, ?1, ?2, ?3, ?4, '', 0, 'unavailable', NULL, NULL, 0, ?5) \
                 ON CONFLICT(id) DO UPDATE SET \
                    conversation_id = excluded.conversation_id, \
                    branch_id = excluded.branch_id, \
                    last_message_id = excluded.last_message_id, \
                    prompt_snapshot = excluded.prompt_snapshot, \
                    generated_prefix = '', \
                    last_token_count = 0, \
                    kv_cache_fingerprint = excluded.kv_cache_fingerprint, \
                    model_fingerprint = NULL, \
                    watchdog_fingerprint = NULL, \
                    resumed_after_kill = 0, \
                    updated_at = excluded.updated_at",
                rusqlite::params![
                    conversation_id,
                    branch_id,
                    assistant_message_id,
                    user_external_id.0.to_string(),
                    now,
                ],
            )?;

            tx.commit()?;
            Ok::<_, DbError>(PersistedTurn {
                conversation_id,
                branch_id,
                user_message_id,
                assistant_message_id,
                assistant_external_id,
            })
        })
        .await
    }

    pub async fn append_message(
        pool: &DatabasePool,
        conversation_id: i64,
        branch_id: Option<i64>,
        parent_message_id: Option<i64>,
        external_id: MessageId,
        role: Role,
        content: String,
        status: MessageStatus,
    ) -> Result<MessageRecord> {
        pool.with_conn(move |c| {
            let now = chrono::Utc::now().to_rfc3339();
            c.execute(
                "INSERT INTO messages \
                    (external_id, conversation_id, role, content, created_at, updated_at, \
                     branch_id, parent_message_id, token_count, deleted, status) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?5, ?6, ?7, 0, 0, ?8)",
                rusqlite::params![
                    external_id.0.to_string(),
                    conversation_id,
                    role_to_sql(role),
                    content,
                    now,
                    branch_id,
                    parent_message_id,
                    status.as_str(),
                ],
            )?;
            let id = c.last_insert_rowid();
            get_message_by_id(c, id)
        })
        .await
    }

    pub async fn update_message(
        pool: &DatabasePool,
        message_id: i64,
        content: String,
        token_count: Option<u32>,
        status: MessageStatus,
    ) -> Result<()> {
        pool.with_conn(move |c| {
            let now = chrono::Utc::now().to_rfc3339();
            c.execute(
                "UPDATE messages \
                 SET content = ?1, token_count = COALESCE(?2, token_count), \
                     status = ?3, updated_at = ?4 \
                 WHERE id = ?5",
                rusqlite::params![
                    content,
                    token_count.map(|v| v as i64),
                    status.as_str(),
                    now,
                    message_id,
                ],
            )?;
            c.execute(
                "UPDATE conversations \
                 SET updated_at = ?1 \
                 WHERE id = (SELECT conversation_id FROM messages WHERE id = ?2)",
                rusqlite::params![now, message_id],
            )?;
            Ok::<_, DbError>(())
        })
        .await
    }

    pub async fn update_assistant_partial(
        pool: &DatabasePool,
        turn: PersistedTurn,
        content: String,
    ) -> Result<()> {
        pool.with_conn(move |c| {
            let now = chrono::Utc::now().to_rfc3339();
            let token_count = estimate_tokens(&content);
            c.execute(
                "UPDATE messages \
                 SET content = ?1, token_count = ?2, status = 'streaming', updated_at = ?3 \
                 WHERE id = ?4",
                rusqlite::params![content, token_count as i64, now, turn.assistant_message_id],
            )?;
            c.execute(
                "UPDATE recovery_state \
                 SET generated_prefix = ?1, last_token_count = ?2, updated_at = ?3 \
                 WHERE id = 1 AND last_message_id = ?4",
                rusqlite::params![content, token_count as i64, now, turn.assistant_message_id],
            )?;
            c.execute(
                "UPDATE conversations SET updated_at = ?1 WHERE id = ?2",
                rusqlite::params![now, turn.conversation_id],
            )?;
            Ok::<_, DbError>(())
        })
        .await
    }

    pub async fn mark_message_status(
        pool: &DatabasePool,
        message_id: i64,
        status: MessageStatus,
    ) -> Result<()> {
        pool.with_conn(move |c| {
            let now = chrono::Utc::now().to_rfc3339();
            c.execute(
                "UPDATE messages SET status = ?1, updated_at = ?2 WHERE id = ?3",
                rusqlite::params![status.as_str(), now, message_id],
            )?;
            c.execute(
                "UPDATE conversations \
                 SET updated_at = ?1 \
                 WHERE id = (SELECT conversation_id FROM messages WHERE id = ?2)",
                rusqlite::params![now, message_id],
            )?;
            Ok::<_, DbError>(())
        })
        .await
    }

    pub async fn complete_turn(
        pool: &DatabasePool,
        turn: PersistedTurn,
        content: String,
    ) -> Result<()> {
        pool.with_conn(move |c| {
            let tx = c.transaction()?;
            let now = chrono::Utc::now().to_rfc3339();
            let token_count = estimate_tokens(&content);
            tx.execute(
                "UPDATE messages \
                 SET content = ?1, token_count = ?2, status = 'completed', updated_at = ?3 \
                 WHERE id = ?4",
                rusqlite::params![content, token_count as i64, now, turn.assistant_message_id],
            )?;
            tx.execute(
                "UPDATE conversations SET updated_at = ?1 WHERE id = ?2",
                rusqlite::params![now, turn.conversation_id],
            )?;
            tx.execute(
                "DELETE FROM recovery_state WHERE id = 1 AND last_message_id = ?1",
                [turn.assistant_message_id],
            )?;
            tx.commit()?;
            Ok::<_, DbError>(())
        })
        .await
    }

    pub async fn fail_turn(
        pool: &DatabasePool,
        turn: PersistedTurn,
        status: MessageStatus,
        content: String,
    ) -> Result<()> {
        let terminal = match status {
            MessageStatus::Cancelled => MessageStatus::Cancelled,
            _ => MessageStatus::Failed,
        };
        pool.with_conn(move |c| {
            let tx = c.transaction()?;
            let now = chrono::Utc::now().to_rfc3339();
            let token_count = estimate_tokens(&content);
            tx.execute(
                "UPDATE messages \
                 SET content = ?1, token_count = ?2, status = ?3, updated_at = ?4 \
                 WHERE id = ?5",
                rusqlite::params![
                    content,
                    token_count as i64,
                    terminal.as_str(),
                    now,
                    turn.assistant_message_id,
                ],
            )?;
            tx.execute(
                "DELETE FROM recovery_state WHERE id = 1 AND last_message_id = ?1",
                [turn.assistant_message_id],
            )?;
            tx.commit()?;
            Ok::<_, DbError>(())
        })
        .await
    }

    pub async fn get_messages_for_conversation(
        pool: &DatabasePool,
        external_id: ConversationId,
    ) -> Result<Vec<MessageRecord>> {
        pool.with_conn(move |c| {
            let conversation_id: i64 = c.query_row(
                "SELECT id FROM conversations WHERE external_id = ?1",
                [external_id.0.to_string()],
                |row| row.get(0),
            )?;
            let mut stmt = c.prepare(
                "SELECT id, external_id, conversation_id, role, content, created_at, updated_at, \
                        branch_id, parent_message_id, token_count, status \
                 FROM messages \
                 WHERE conversation_id = ?1 AND deleted = 0 \
                 ORDER BY created_at ASC, id ASC",
            )?;
            let rows = stmt
                .query_map([conversation_id], map_message_row)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok::<_, DbError>(rows)
        })
        .await
    }

    pub async fn mark_incomplete_turns_failed(pool: &DatabasePool) -> Result<usize> {
        pool.with_conn(|c| {
            let tx = c.transaction()?;
            let now = chrono::Utc::now().to_rfc3339();
            let changed = tx.execute(
                "UPDATE messages \
                 SET status = 'failed', updated_at = ?1 \
                 WHERE status IN ('pending', 'streaming')",
                [now],
            )?;
            tx.execute("DELETE FROM recovery_state", [])?;
            tx.commit()?;
            Ok::<_, DbError>(changed)
        })
        .await
    }
}

fn get_conversation_by_external_id(
    c: &mut crate::storage::pool::Conn,
    external_id: ConversationId,
) -> std::result::Result<ConversationRecord, DbError> {
    c.query_row(
        "SELECT id, external_id, title, created_at, updated_at, archived \
         FROM conversations WHERE external_id = ?1",
        [external_id.0.to_string()],
        map_conversation_row,
    )
    .map_err(DbError::from)
}

fn get_message_by_id(
    c: &mut crate::storage::pool::Conn,
    id: i64,
) -> std::result::Result<MessageRecord, DbError> {
    c.query_row(
        "SELECT id, external_id, conversation_id, role, content, created_at, updated_at, \
                branch_id, parent_message_id, token_count, status \
         FROM messages WHERE id = ?1",
        [id],
        map_message_row,
    )
    .map_err(DbError::from)
}

fn map_conversation_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ConversationRecord> {
    let external_id: String = row.get(1)?;
    let created_at: String = row.get(3)?;
    let updated_at: String = row.get(4)?;
    Ok(ConversationRecord {
        id: row.get(0)?,
        external_id: ConversationId(uuid::Uuid::parse_str(&external_id).map_err(parse_uuid_err)?),
        title: row.get(2)?,
        created_at: parse_rfc3339(&created_at),
        updated_at: parse_rfc3339(&updated_at),
        archived: row.get::<_, i64>(5)? != 0,
    })
}

fn map_message_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<MessageRecord> {
    let external_id: String = row.get(1)?;
    let role: String = row.get(3)?;
    let created_at: String = row.get(5)?;
    let updated_at: Option<String> = row.get(6)?;
    let status: String = row.get(10)?;
    Ok(MessageRecord {
        id: row.get(0)?,
        external_id: MessageId(uuid::Uuid::parse_str(&external_id).map_err(parse_uuid_err)?),
        conversation_id: row.get(2)?,
        role: role_from_sql(&role),
        content: row.get(4)?,
        created_at: parse_rfc3339(&created_at),
        updated_at: updated_at
            .as_deref()
            .map(parse_rfc3339)
            .unwrap_or_else(|| parse_rfc3339(&created_at)),
        branch_id: row.get(7)?,
        parent_message_id: row.get(8)?,
        token_count: row.get::<_, i64>(9)? as u32,
        status: MessageStatus::from_sql(&status),
    })
}

fn parse_uuid_err(err: uuid::Error) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(err))
}

fn parse_rfc3339(value: &str) -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::parse_from_rfc3339(value)
        .map(|d| d.with_timezone(&chrono::Utc))
        .unwrap_or_else(|_| chrono::Utc::now())
}

fn role_to_sql(role: Role) -> &'static str {
    match role {
        Role::System => "system",
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::Tool => "tool",
        Role::RedTeam => "red_team",
    }
}

fn role_from_sql(value: &str) -> Role {
    match value {
        "system" => Role::System,
        "user" => Role::User,
        "assistant" => Role::Assistant,
        "tool" => Role::Tool,
        "red_team" => Role::RedTeam,
        _ => Role::System,
    }
}

fn estimate_tokens(content: &str) -> u32 {
    content.len().div_ceil(4).min(u32::MAX as usize) as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{DatabasePool, Migrator};

    async fn migrated_pool() -> DatabasePool {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("chat.db");
        let pool = DatabasePool::open(&db_path).unwrap();
        let migrations_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../")
            .join(crate::storage::MIGRATIONS_DIR);
        Migrator::new(migrations_dir)
            .apply_pending(&pool)
            .await
            .unwrap();
        std::mem::forget(dir);
        pool
    }

    #[tokio::test]
    async fn begin_and_complete_turn_persists_ordered_messages() {
        let pool = migrated_pool().await;
        let conversation = ConversationId::new();
        let branch = BranchId::new();
        let user_one = MessageId::new();
        let assistant_one = MessageId::new();

        let turn = ConversationRepository::begin_turn(
            &pool,
            conversation,
            branch,
            user_one,
            assistant_one,
            "hello".to_string(),
        )
        .await
        .unwrap();
        ConversationRepository::update_assistant_partial(
            &pool,
            turn.clone(),
            "partial".to_string(),
        )
        .await
        .unwrap();
        ConversationRepository::complete_turn(&pool, turn, "partial final".to_string())
            .await
            .unwrap();

        let messages = ConversationRepository::get_messages_for_conversation(&pool, conversation)
            .await
            .unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].external_id, user_one);
        assert_eq!(messages[0].status, MessageStatus::Completed);
        assert_eq!(messages[1].external_id, assistant_one);
        assert_eq!(messages[1].content, "partial final");
        assert_eq!(messages[1].status, MessageStatus::Completed);
        assert_eq!(messages[1].parent_message_id, Some(messages[0].id));

        let turn_two = ConversationRepository::begin_turn(
            &pool,
            conversation,
            branch,
            MessageId::new(),
            MessageId::new(),
            "again".to_string(),
        )
        .await
        .unwrap();
        let messages = ConversationRepository::get_messages_for_conversation(&pool, conversation)
            .await
            .unwrap();
        assert_eq!(messages.len(), 4);
        assert_eq!(messages[2].parent_message_id, Some(messages[1].id));
        assert_eq!(turn_two.conversation_id, messages[0].conversation_id);
    }

    #[tokio::test]
    async fn boot_recovery_marks_incomplete_turn_failed() {
        let pool = migrated_pool().await;
        let conversation = ConversationId::new();
        let turn = ConversationRepository::begin_turn(
            &pool,
            conversation,
            BranchId::new(),
            MessageId::new(),
            MessageId::new(),
            "hello".to_string(),
        )
        .await
        .unwrap();
        ConversationRepository::update_assistant_partial(&pool, turn, "interrupted".to_string())
            .await
            .unwrap();

        let changed = ConversationRepository::mark_incomplete_turns_failed(&pool)
            .await
            .unwrap();
        assert_eq!(changed, 1);

        let messages = ConversationRepository::get_messages_for_conversation(&pool, conversation)
            .await
            .unwrap();
        assert_eq!(messages[1].content, "interrupted");
        assert_eq!(messages[1].status, MessageStatus::Failed);
    }
}
