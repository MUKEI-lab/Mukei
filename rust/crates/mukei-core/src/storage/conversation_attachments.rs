//! Durable conversation-level references to Universal Storage files.
//!
//! Attachments are logical references, not file copies. The repository stores a
//! stable conversation UUID beside a storage node identity, validates Universal
//! Storage ownership on attach/read, and decrypts file bytes only through the
//! authenticated immutable object store.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use rusqlite::OptionalExtension;
use serde::Serialize;
use uuid::Uuid;

use crate::error::{MukeiError, Result};
use crate::storage::object_store::{ImmutableObjectStore, ObjectCipher, StoredObject};
use crate::storage::pool::{DatabasePool, DbError, PooledConnectionExt};
use crate::storage::universal::{StorageNodeId, StorageObjectId};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ConversationStorageAttachment {
    pub attachment_id: String,
    pub conversation_id: String,
    pub node_id: String,
    pub display_name: String,
    pub mime_type: Option<String>,
    pub size_bytes: u64,
    pub node_state: String,
    pub created_at: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConversationAttachmentContext {
    pub node_id: String,
    pub display_name: String,
    pub mime_type: Option<String>,
    pub content: String,
    pub truncated: bool,
}

#[async_trait]
pub trait ConversationAttachmentPort: Send + Sync {
    async fn add_attachment(
        &self,
        conversation_id: String,
        node_id: StorageNodeId,
    ) -> Result<ConversationStorageAttachment>;

    async fn remove_attachment(
        &self,
        conversation_id: String,
        node_id: StorageNodeId,
    ) -> Result<bool>;

    async fn remove_all_for_conversation(&self, conversation_id: String) -> Result<usize>;

    async fn list_all(&self) -> Result<Vec<ConversationStorageAttachment>>;

    async fn load_context(
        &self,
        conversation_id: String,
        per_file_limit_bytes: usize,
        total_limit_bytes: usize,
    ) -> Result<Vec<ConversationAttachmentContext>>;
}

pub struct SqlConversationAttachmentService<C> {
    pool: Arc<DatabasePool>,
    object_store: Arc<ImmutableObjectStore<C>>,
}

impl<C: ObjectCipher> SqlConversationAttachmentService<C> {
    pub fn new(pool: Arc<DatabasePool>, object_store: Arc<ImmutableObjectStore<C>>) -> Self {
        Self { pool, object_store }
    }
}

#[derive(Clone)]
struct AttachmentObjectRow {
    node_id: String,
    display_name: String,
    mime_type: Option<String>,
    object_id: String,
    plaintext_sha256: Vec<u8>,
    plaintext_size: i64,
    encrypted_size: i64,
    relative_path: String,
}

#[async_trait]
impl<C> ConversationAttachmentPort for SqlConversationAttachmentService<C>
where
    C: ObjectCipher + Send + Sync + 'static,
{
    async fn add_attachment(
        &self,
        conversation_id: String,
        node_id: StorageNodeId,
    ) -> Result<ConversationStorageAttachment> {
        validate_conversation_id(&conversation_id)?;
        let conversation_for_db = conversation_id.clone();
        self.pool
            .with_conn(move |connection| {
                let node_id = node_id.to_string();
                let target: Option<(String, Option<String>, i64)> = connection
                    .query_row(
                        "SELECT n.display_name, so.detected_mime, so.plaintext_size \
                         FROM storage_nodes n \
                         JOIN storage_scopes s ON s.scope_id = n.scope_id \
                         JOIN file_versions fv ON fv.version_id = n.current_version_id \
                         JOIN storage_objects so ON so.object_id = fv.object_id \
                         WHERE n.node_id = ?1 AND n.node_type = 'file' AND n.state = 'active' \
                           AND s.scope_type = 'universal' AND s.state = 'active' \
                           AND so.integrity_state = 'verified'",
                        [&node_id],
                        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                    )
                    .optional()?;
                let (display_name, mime_type, plaintext_size) = target.ok_or_else(|| {
                    invariant("attachment target is not an active verified Universal Storage file")
                })?;
                let now = chrono::Utc::now().to_rfc3339();
                let requested_attachment_id = Uuid::new_v4().to_string();
                connection.execute(
                    "INSERT INTO conversation_storage_attachments \
                        (attachment_id, conversation_id, node_id, state, created_at, updated_at, removed_at) \
                     VALUES (?1, ?2, ?3, 'active', ?4, ?4, NULL) \
                     ON CONFLICT(conversation_id, node_id) DO UPDATE SET \
                        state = 'active', updated_at = excluded.updated_at, removed_at = NULL",
                    rusqlite::params![
                        requested_attachment_id,
                        conversation_for_db,
                        node_id,
                        now,
                    ],
                )?;
                let (attachment_id, created_at): (String, String) = connection.query_row(
                    "SELECT attachment_id, created_at FROM conversation_storage_attachments \
                     WHERE conversation_id = ?1 AND node_id = ?2 AND state = 'active'",
                    rusqlite::params![conversation_for_db, node_id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )?;
                Ok::<_, DbError>(ConversationStorageAttachment {
                    attachment_id,
                    conversation_id: conversation_for_db,
                    node_id,
                    display_name,
                    mime_type,
                    size_bytes: u64::try_from(plaintext_size)
                        .map_err(|_| invariant("persisted attachment size is negative"))?,
                    node_state: "active".to_owned(),
                    created_at,
                })
            })
            .await
    }

    async fn remove_attachment(
        &self,
        conversation_id: String,
        node_id: StorageNodeId,
    ) -> Result<bool> {
        validate_conversation_id(&conversation_id)?;
        self.pool
            .with_conn(move |connection| {
                let now = chrono::Utc::now().to_rfc3339();
                let changed = connection.execute(
                    "UPDATE conversation_storage_attachments \
                     SET state = 'removed', removed_at = ?3, updated_at = ?3 \
                     WHERE conversation_id = ?1 AND node_id = ?2 AND state = 'active'",
                    rusqlite::params![conversation_id, node_id.to_string(), now],
                )?;
                Ok::<_, DbError>(changed == 1)
            })
            .await
    }

    async fn remove_all_for_conversation(&self, conversation_id: String) -> Result<usize> {
        validate_conversation_id(&conversation_id)?;
        self.pool
            .with_conn(move |connection| {
                let now = chrono::Utc::now().to_rfc3339();
                connection
                    .execute(
                        "UPDATE conversation_storage_attachments \
                         SET state = 'removed', removed_at = ?2, updated_at = ?2 \
                         WHERE conversation_id = ?1 AND state = 'active'",
                        rusqlite::params![conversation_id, now],
                    )
                    .map_err(DbError::from)
            })
            .await
    }

    async fn list_all(&self) -> Result<Vec<ConversationStorageAttachment>> {
        self.pool
            .with_conn(|connection| {
                let mut statement = connection.prepare(
                    "SELECT a.attachment_id, a.conversation_id, a.node_id, n.display_name, \
                            so.detected_mime, so.plaintext_size, n.state, a.created_at \
                     FROM conversation_storage_attachments a \
                     JOIN storage_nodes n ON n.node_id = a.node_id \
                     LEFT JOIN file_versions fv ON fv.version_id = n.current_version_id \
                     LEFT JOIN storage_objects so ON so.object_id = fv.object_id \
                     WHERE a.state = 'active' \
                     ORDER BY a.conversation_id, a.created_at, a.attachment_id",
                )?;
                let rows = statement
                    .query_map([], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, Option<String>>(4)?,
                            row.get::<_, Option<i64>>(5)?.unwrap_or(0),
                            row.get::<_, String>(6)?,
                            row.get::<_, String>(7)?,
                        ))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                rows.into_iter()
                    .map(
                        |(
                            attachment_id,
                            conversation_id,
                            node_id,
                            display_name,
                            mime_type,
                            size_bytes,
                            node_state,
                            created_at,
                        )| {
                            Ok(ConversationStorageAttachment {
                                attachment_id,
                                conversation_id,
                                node_id,
                                display_name,
                                mime_type,
                                size_bytes: u64::try_from(size_bytes).map_err(|_| {
                                    invariant("persisted attachment size is negative")
                                })?,
                                node_state,
                                created_at,
                            })
                        },
                    )
                    .collect::<std::result::Result<Vec<_>, DbError>>()
            })
            .await
    }

    async fn load_context(
        &self,
        conversation_id: String,
        per_file_limit_bytes: usize,
        total_limit_bytes: usize,
    ) -> Result<Vec<ConversationAttachmentContext>> {
        validate_conversation_id(&conversation_id)?;
        if per_file_limit_bytes == 0 || total_limit_bytes == 0 {
            return Ok(Vec::new());
        }
        let conversation_for_db = conversation_id.clone();
        let rows = self
            .pool
            .with_conn(move |connection| {
                let active_count: i64 = connection.query_row(
                    "SELECT COUNT(*) FROM conversation_storage_attachments \
                     WHERE conversation_id = ?1 AND state = 'active'",
                    [&conversation_for_db],
                    |row| row.get(0),
                )?;
                let mut statement = connection.prepare(
                    "SELECT n.node_id, n.display_name, so.detected_mime, so.object_id, \
                            so.plaintext_sha256, so.plaintext_size, so.encrypted_size, so.relative_path \
                     FROM conversation_storage_attachments a \
                     JOIN storage_nodes n ON n.node_id = a.node_id \
                     JOIN storage_scopes s ON s.scope_id = n.scope_id \
                     JOIN file_versions fv ON fv.version_id = n.current_version_id \
                     JOIN storage_objects so ON so.object_id = fv.object_id \
                     WHERE a.conversation_id = ?1 AND a.state = 'active' \
                       AND n.node_type = 'file' AND n.state = 'active' \
                       AND s.scope_type = 'universal' AND s.state = 'active' \
                       AND so.integrity_state = 'verified' \
                     ORDER BY a.created_at, a.attachment_id",
                )?;
                let rows = statement
                    .query_map([&conversation_for_db], |row| {
                        Ok(AttachmentObjectRow {
                            node_id: row.get(0)?,
                            display_name: row.get(1)?,
                            mime_type: row.get(2)?,
                            object_id: row.get(3)?,
                            plaintext_sha256: row.get(4)?,
                            plaintext_size: row.get(5)?,
                            encrypted_size: row.get(6)?,
                            relative_path: row.get(7)?,
                        })
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                if i64::try_from(rows.len()).unwrap_or(i64::MAX) != active_count {
                    return Err(invariant(
                        "one or more conversation attachments are unavailable or unsafe",
                    ));
                }
                Ok::<_, DbError>(rows)
            })
            .await?;

        let mut remaining = total_limit_bytes;
        let mut contexts = Vec::with_capacity(rows.len());
        for row in rows {
            if remaining == 0 {
                break;
            }
            let object = stored_object_from_row(&row)?;
            let store = Arc::clone(&self.object_store);
            let bytes = tokio::task::spawn_blocking(move || store.read_verified(&object))
                .await
                .map_err(|error| MukeiError::BlockingJoinFailed(error.to_string()))?
                .map_err(|_| MukeiError::DatabaseCorruption)?;
            let text = String::from_utf8(bytes).map_err(|_| MukeiError::DatabaseCorruption)?;
            let allowed = per_file_limit_bytes.min(remaining);
            let (content, truncated) = truncate_utf8_owned(text, allowed);
            remaining = remaining.saturating_sub(content.len());
            contexts.push(ConversationAttachmentContext {
                node_id: row.node_id,
                display_name: row.display_name,
                mime_type: row.mime_type,
                content,
                truncated,
            });
        }
        Ok(contexts)
    }
}

fn stored_object_from_row(row: &AttachmentObjectRow) -> Result<StoredObject> {
    let object_id = Uuid::parse_str(&row.object_id)
        .map(StorageObjectId)
        .map_err(|_| MukeiError::DatabaseCorruption)?;
    let plaintext_sha256: [u8; 32] = row
        .plaintext_sha256
        .as_slice()
        .try_into()
        .map_err(|_| MukeiError::DatabaseCorruption)?;
    let plaintext_size =
        u64::try_from(row.plaintext_size).map_err(|_| MukeiError::DatabaseCorruption)?;
    let encrypted_size =
        u64::try_from(row.encrypted_size).map_err(|_| MukeiError::DatabaseCorruption)?;
    Ok(StoredObject {
        object_id,
        plaintext_sha256,
        plaintext_size,
        encrypted_size,
        relative_path: PathBuf::from(&row.relative_path),
        deduplicated: true,
    })
}

fn truncate_utf8_owned(value: String, max_bytes: usize) -> (String, bool) {
    if value.len() <= max_bytes {
        return (value, false);
    }
    let mut end = max_bytes;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    (value[..end].to_owned(), true)
}

fn validate_conversation_id(value: &str) -> Result<()> {
    if value.trim() != value || Uuid::parse_str(value).is_err() {
        return Err(MukeiError::Invariant(
            "conversation attachment requires a canonical UUID conversation id".into(),
        ));
    }
    Ok(())
}

fn invariant(message: impl Into<String>) -> DbError {
    DbError::Domain(MukeiError::Invariant(message.into()))
}
