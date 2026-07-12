//! Repository for durable conversations, messages, and active turn state.
//!
//! This module keeps chat persistence behind the storage layer instead of
//! letting the bridge or agent loop issue ad-hoc SQL. Every method uses
//! [`PooledConnectionExt::with_conn`] so SQLite work stays on the blocking
//! pool.

use crate::error::Result;
use crate::storage::pool::{DatabasePool, DbError, PooledConnectionExt};
use crate::types::{BranchId, ChatMessage, ConversationId, MessageId, Role};
use rusqlite::OptionalExtension;

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

#[derive(Clone, Debug, serde::Serialize, PartialEq, Eq)]
pub struct ConversationSummary {
    pub conversation_id: String,
    pub title: String,
    pub active_branch_id: String,
    pub updated_at: String,
    pub preview: String,
}

#[derive(Clone, Debug, serde::Serialize, PartialEq, Eq)]
pub struct TimelineRow {
    #[serde(rename = "rowId")]
    pub row_id: String,
    #[serde(rename = "type")]
    pub row_type: String,
    pub text: String,
    pub phase: String,
    pub kind: String,
    pub status: String,
    pub timestamp: String,
    #[serde(rename = "toolName")]
    pub tool_name: String,
    #[serde(rename = "parentId")]
    pub parent_id: String,
    #[serde(rename = "conversationId")]
    pub conversation_id: String,
    #[serde(rename = "branchId")]
    pub branch_id: String,
}

#[derive(Clone, Debug, serde::Serialize, PartialEq, Eq)]
pub struct TimelinePage {
    pub items: Vec<TimelineRow>,
    pub has_older: bool,
    pub oldest_message_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PersistedTurn {
    pub conversation_id: i64,
    pub branch_id: i64,
    pub user_message_id: i64,
    pub assistant_message_id: i64,
    pub assistant_external_id: MessageId,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppendMessage {
    pub conversation_id: i64,
    pub branch_id: Option<i64>,
    pub parent_message_id: Option<i64>,
    pub external_id: MessageId,
    pub role: Role,
    pub content: String,
    pub status: MessageStatus,
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
        message: AppendMessage,
    ) -> Result<MessageRecord> {
        pool.with_conn(move |c| {
            let now = chrono::Utc::now().to_rfc3339();
            c.execute(
                "INSERT INTO messages \
                    (external_id, conversation_id, role, content, created_at, updated_at, \
                     branch_id, parent_message_id, token_count, deleted, status) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?5, ?6, ?7, 0, 0, ?8)",
                rusqlite::params![
                    message.external_id.0.to_string(),
                    message.conversation_id,
                    role_to_sql(message.role),
                    message.content,
                    now,
                    message.branch_id,
                    message.parent_message_id,
                    message.status.as_str(),
                ],
            )?;
            let id = c.last_insert_rowid();
            get_message_by_id(c, id)
        })
        .await
    }

    /// Persist an assistant/tool message emitted inside the ReAct loop.
    /// Parent resolution uses the external message id and is restricted to
    /// the same durable conversation and branch, preventing cross-thread
    /// graph corruption.
    pub async fn append_intermediate_message(
        pool: &DatabasePool,
        turn: PersistedTurn,
        message: ChatMessage,
    ) -> Result<MessageRecord> {
        pool.with_conn(move |c| {
            let tx = c.transaction()?;
            let parent_external_id = message.parent.map(|id| id.0.to_string());
            let parent_message_id = match parent_external_id {
                Some(parent_external_id) => Some(tx.query_row(
                    "SELECT id FROM messages \
                     WHERE external_id = ?1 AND conversation_id = ?2 AND branch_id = ?3 \
                       AND deleted = 0",
                    rusqlite::params![parent_external_id, turn.conversation_id, turn.branch_id,],
                    |row| row.get::<_, i64>(0),
                )?),
                None => None,
            };
            let created_at = message.created_at.to_rfc3339();
            tx.execute(
                "INSERT INTO messages \
                    (external_id, conversation_id, role, content, created_at, updated_at, \
                     branch_id, parent_message_id, token_count, deleted, status) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?5, ?6, ?7, ?8, 0, 'completed')",
                rusqlite::params![
                    message.id.0.to_string(),
                    turn.conversation_id,
                    role_to_sql(message.role),
                    message.content,
                    created_at,
                    turn.branch_id,
                    parent_message_id,
                    message.token_count.map(i64::from),
                ],
            )?;
            let id = tx.last_insert_rowid();
            tx.execute(
                "UPDATE conversations SET updated_at = ?1 WHERE id = ?2",
                rusqlite::params![chrono::Utc::now().to_rfc3339(), turn.conversation_id],
            )?;
            tx.commit()?;
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
        Self::complete_turn_with_parent(pool, turn, content, None, None).await
    }

    pub async fn complete_turn_with_parent(
        pool: &DatabasePool,
        turn: PersistedTurn,
        content: String,
        parent_external_id: Option<MessageId>,
        token_count: Option<u32>,
    ) -> Result<()> {
        finalize_turn(
            pool,
            turn,
            MessageStatus::Completed,
            content,
            parent_external_id,
            token_count,
            true,
        )
        .await
    }

    pub async fn fail_turn(
        pool: &DatabasePool,
        turn: PersistedTurn,
        status: MessageStatus,
        content: String,
    ) -> Result<()> {
        Self::fail_turn_with_parent(pool, turn, status, content, None).await
    }

    pub async fn fail_turn_with_parent(
        pool: &DatabasePool,
        turn: PersistedTurn,
        status: MessageStatus,
        content: String,
        parent_external_id: Option<MessageId>,
    ) -> Result<()> {
        let terminal = match status {
            MessageStatus::Cancelled => MessageStatus::Cancelled,
            _ => MessageStatus::Failed,
        };
        // Preserve the recovery snapshot for interrupted/failed turns so a
        // later resume or regeneration can use the exact partial prefix.
        finalize_turn(
            pool,
            turn,
            terminal,
            content,
            parent_external_id,
            None,
            false,
        )
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

    pub async fn list_conversation_summaries(
        pool: &DatabasePool,
        limit: usize,
    ) -> Result<Vec<ConversationSummary>> {
        let limit = limit.clamp(1, 200) as i64;
        pool.with_conn(move |c| {
            let mut stmt = c.prepare(
                "SELECT c.external_id, c.title, COALESCE(b.external_id, ''), c.updated_at, \
                        COALESCE((SELECT content FROM messages m \
                                  WHERE m.conversation_id = c.id AND m.deleted = 0 \
                                  ORDER BY m.id DESC LIMIT 1), '') \
                 FROM conversations c \
                 LEFT JOIN branches b ON b.id = c.active_branch_id \
                 WHERE c.archived = 0 \
                 ORDER BY c.updated_at DESC, c.id DESC LIMIT ?1",
            )?;
            let rows = stmt
                .query_map([limit], |row| {
                    Ok(ConversationSummary {
                        conversation_id: row.get(0)?,
                        title: row.get(1)?,
                        active_branch_id: row.get(2)?,
                        updated_at: row.get(3)?,
                        preview: row.get(4)?,
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok::<_, DbError>(rows)
        })
        .await
    }

    pub async fn timeline_page(
        pool: &DatabasePool,
        conversation_external_id: ConversationId,
        branch_external_id: BranchId,
        before_message_external_id: Option<MessageId>,
        limit: usize,
    ) -> Result<TimelinePage> {
        let limit = limit.clamp(1, 200) as i64;
        pool.with_conn(move |c| {
            let before_id = match before_message_external_id {
                Some(message_id) => c
                    .query_row(
                        "SELECT m.id FROM messages m \
                         JOIN conversations c ON c.id = m.conversation_id \
                         JOIN branches b ON b.id = m.branch_id \
                         WHERE m.external_id = ?1 AND c.external_id = ?2 AND b.external_id = ?3",
                        rusqlite::params![
                            message_id.0.to_string(),
                            conversation_external_id.0.to_string(),
                            branch_external_id.0.to_string(),
                        ],
                        |row| row.get::<_, i64>(0),
                    )
                    .optional()?,
                None => None,
            };

            let conversation_id: i64 = c.query_row(
                "SELECT id FROM conversations WHERE external_id = ?1 AND archived = 0",
                [conversation_external_id.0.to_string()],
                |row| row.get(0),
            )?;
            let branch_id: i64 = c.query_row(
                "SELECT id FROM branches WHERE external_id = ?1 AND conversation_id = ?2",
                rusqlite::params![branch_external_id.0.to_string(), conversation_id],
                |row| row.get(0),
            )?;

            let boundary = before_id.unwrap_or(i64::MAX);
            let mut stmt = c.prepare(
                "SELECT m.id, m.external_id, m.role, m.content, m.created_at, m.status, \
                        COALESCE(m.tool_name, ''), COALESCE(parent.external_id, '') \
                 FROM messages m \
                 LEFT JOIN messages parent ON parent.id = m.parent_message_id \
                 WHERE m.conversation_id = ?1 AND m.branch_id = ?2 AND m.deleted = 0 \
                   AND m.id < ?3 \
                 ORDER BY m.id DESC LIMIT ?4",
            )?;
            let mut raw = stmt
                .query_map(
                    rusqlite::params![conversation_id, branch_id, boundary, limit + 1],
                    |row| {
                        let role: String = row.get(2)?;
                        let tool_name: String = row.get(6)?;
                        let (row_type, kind, phase) = match role.as_str() {
                            "user" => ("user_message", "", ""),
                            "assistant" if !tool_name.is_empty() => {
                                ("timeline_event", "tool", "result")
                            }
                            "assistant" => ("assistant_message", "", ""),
                            "tool" => ("timeline_event", "tool", "result"),
                            "system" => ("timeline_event", "system", "result"),
                            _ => ("timeline_event", "system", "failure"),
                        };
                        Ok((
                            row.get::<_, i64>(0)?,
                            TimelineRow {
                                row_id: row.get(1)?,
                                row_type: row_type.into(),
                                text: row.get(3)?,
                                phase: phase.into(),
                                kind: kind.into(),
                                status: row.get(5)?,
                                timestamp: row.get(4)?,
                                tool_name,
                                parent_id: row.get(7)?,
                                conversation_id: conversation_external_id.0.to_string(),
                                branch_id: branch_external_id.0.to_string(),
                            },
                        ))
                    },
                )?
                .collect::<rusqlite::Result<Vec<_>>>()?;

            let has_older = raw.len() as i64 > limit;
            if has_older {
                raw.pop();
            }
            raw.reverse();
            let oldest_message_id = raw
                .first()
                .map(|(_, row)| row.row_id.clone())
                .unwrap_or_default();
            Ok::<_, DbError>(TimelinePage {
                items: raw.into_iter().map(|(_, row)| row).collect(),
                has_older,
                oldest_message_id,
            })
        })
        .await
    }

    pub async fn mark_incomplete_turns_failed(pool: &DatabasePool) -> Result<usize> {
        pool.with_conn(|c| {
            let tx = c.transaction()?;
            let now = chrono::Utc::now().to_rfc3339();
            let changed = tx.execute(
                "UPDATE messages \
                 SET content = COALESCE( \
                        NULLIF(( \
                            SELECT generated_prefix \
                            FROM recovery_state \
                            WHERE recovery_state.last_message_id = messages.id \
                        ), ''), \
                        content \
                     ), \
                     status = 'failed', \
                     updated_at = ?1 \
                 WHERE status IN ('pending', 'streaming')",
                [now.as_str()],
            )?;
            tx.execute(
                "UPDATE recovery_state \
                 SET resumed_after_kill = 1, updated_at = ?1 \
                 WHERE EXISTS ( \
                    SELECT 1 FROM messages \
                    WHERE messages.id = recovery_state.last_message_id \
                      AND messages.status = 'failed' \
                 )",
                [now.as_str()],
            )?;
            tx.commit()?;
            Ok::<_, DbError>(changed)
        })
        .await
    }
}

async fn finalize_turn(
    pool: &DatabasePool,
    turn: PersistedTurn,
    status: MessageStatus,
    content: String,
    parent_external_id: Option<MessageId>,
    token_count: Option<u32>,
    clear_recovery: bool,
) -> Result<()> {
    pool.with_conn(move |c| {
        let tx = c.transaction()?;
        let now = chrono::Utc::now().to_rfc3339();
        let resolved_parent_id = match parent_external_id {
            Some(parent_external_id) => Some(tx.query_row(
                "SELECT id FROM messages \
                 WHERE external_id = ?1 AND conversation_id = ?2 AND branch_id = ?3 \
                   AND id <> ?4 AND deleted = 0",
                rusqlite::params![
                    parent_external_id.0.to_string(),
                    turn.conversation_id,
                    turn.branch_id,
                    turn.assistant_message_id,
                ],
                |row| row.get::<_, i64>(0),
            )?),
            None => None,
        };
        let token_count = token_count.unwrap_or_else(|| estimate_tokens(&content));
        tx.execute(
            "UPDATE messages \
             SET content = ?1, token_count = ?2, status = ?3, updated_at = ?4, \
                 parent_message_id = COALESCE(?5, parent_message_id) \
             WHERE id = ?6",
            rusqlite::params![
                content,
                i64::from(token_count),
                status.as_str(),
                now,
                resolved_parent_id,
                turn.assistant_message_id,
            ],
        )?;
        tx.execute(
            "UPDATE conversations SET updated_at = ?1 WHERE id = ?2",
            rusqlite::params![now, turn.conversation_id],
        )?;
        if clear_recovery {
            tx.execute(
                "DELETE FROM recovery_state WHERE id = 1 AND last_message_id = ?1",
                [turn.assistant_message_id],
            )?;
        } else {
            tx.execute(
                "UPDATE recovery_state \
                 SET generated_prefix = ?1, last_token_count = ?2, resumed_after_kill = 1, \
                     updated_at = ?3 \
                 WHERE id = 1 AND last_message_id = ?4",
                rusqlite::params![
                    content,
                    i64::from(token_count),
                    now,
                    turn.assistant_message_id,
                ],
            )?;
        }
        tx.commit()?;
        Ok::<_, DbError>(())
    })
    .await
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
    use crate::storage::{DatabasePool, Migrator, RecoveryStore};

    async fn migrated_pool() -> DatabasePool {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("chat.db");
        let pool = DatabasePool::open(&db_path).unwrap();
        Migrator::embedded().apply_pending(&pool).await.unwrap();
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
    async fn timeline_page_is_conversation_and_branch_scoped() {
        let pool = migrated_pool().await;
        let conversation_a = ConversationId::new();
        let branch_a = BranchId::new();
        let turn_a = ConversationRepository::begin_turn(
            &pool,
            conversation_a,
            branch_a,
            MessageId::new(),
            MessageId::new(),
            "alpha".into(),
        )
        .await
        .unwrap();
        ConversationRepository::complete_turn(&pool, turn_a, "answer alpha".into())
            .await
            .unwrap();

        let conversation_b = ConversationId::new();
        let branch_b = BranchId::new();
        let turn_b = ConversationRepository::begin_turn(
            &pool,
            conversation_b,
            branch_b,
            MessageId::new(),
            MessageId::new(),
            "beta".into(),
        )
        .await
        .unwrap();
        ConversationRepository::complete_turn(&pool, turn_b, "answer beta".into())
            .await
            .unwrap();

        let page = ConversationRepository::timeline_page(&pool, conversation_a, branch_a, None, 20)
            .await
            .unwrap();
        assert_eq!(page.items.len(), 2);
        assert!(page
            .items
            .iter()
            .all(|row| row.conversation_id == conversation_a.0.to_string()));
        assert!(page
            .items
            .iter()
            .all(|row| row.branch_id == branch_a.0.to_string()));
        assert!(page.items.iter().any(|row| row.text == "alpha"));
        assert!(!page.items.iter().any(|row| row.text.contains("beta")));
    }

    #[tokio::test]
    async fn timeline_page_uses_stable_message_anchor() {
        let pool = migrated_pool().await;
        let conversation = ConversationId::new();
        let branch = BranchId::new();
        for prompt in ["one", "two"] {
            let turn = ConversationRepository::begin_turn(
                &pool,
                conversation,
                branch,
                MessageId::new(),
                MessageId::new(),
                prompt.into(),
            )
            .await
            .unwrap();
            ConversationRepository::complete_turn(&pool, turn, format!("answer {prompt}"))
                .await
                .unwrap();
        }

        let newest = ConversationRepository::timeline_page(&pool, conversation, branch, None, 2)
            .await
            .unwrap();
        assert_eq!(newest.items.len(), 2);
        assert!(newest.has_older);
        let anchor = MessageId(uuid::Uuid::parse_str(&newest.oldest_message_id).unwrap());
        let older =
            ConversationRepository::timeline_page(&pool, conversation, branch, Some(anchor), 2)
                .await
                .unwrap();
        assert_eq!(older.items.len(), 2);
        assert!(!older.has_older);
        assert!(older
            .items
            .iter()
            .all(|row| !newest.items.iter().any(|newer| newer.row_id == row.row_id)));
    }

    #[tokio::test]
    async fn boot_recovery_marks_incomplete_turn_failed_and_preserves_snapshot() {
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

        let recovery = RecoveryStore::load(&pool)
            .await
            .unwrap()
            .expect("interrupted recovery snapshot must be preserved");
        assert_eq!(recovery.last_message_id, messages[1].id);
        assert_eq!(recovery.generated_prefix, "interrupted");
        assert!(recovery.resumed_after_kill);
    }
}
