//! Durable, versioned UI session state.
//!
//! This repository stores presentation restoration metadata only. Conversation
//! content, capabilities, operations, and security state remain authoritative in
//! their respective domain repositories.

use crate::error::{MukeiError, Result};
use crate::storage::pool::{DatabasePool, DbError, PooledConnectionExt};
use serde::{Deserialize, Serialize};

pub const DEFAULT_UI_PROFILE: &str = "default";
pub const UI_SESSION_SCHEMA_VERSION: i64 = 1;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct UiSessionRecord {
    pub profile_id: String,
    pub schema_version: i64,
    pub active_route: String,
    pub active_conversation_id: Option<String>,
    pub active_branch_id: Option<String>,
    pub timeline_anchor_message_id: Option<String>,
    pub selected_model_id: Option<String>,
    pub payload_json: String,
    pub updated_at: String,
}

impl Default for UiSessionRecord {
    fn default() -> Self {
        Self {
            profile_id: DEFAULT_UI_PROFILE.into(),
            schema_version: UI_SESSION_SCHEMA_VERSION,
            active_route: "boot".into(),
            active_conversation_id: None,
            active_branch_id: None,
            timeline_anchor_message_id: None,
            selected_model_id: None,
            payload_json: "{}".into(),
            updated_at: chrono::Utc::now().to_rfc3339(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct UiDraftRecord {
    pub conversation_id: String,
    pub branch_id: String,
    pub text: String,
    pub cursor_position: i64,
    pub attachment_refs_json: String,
    pub updated_at: String,
}

pub struct UiSessionRepository;

impl UiSessionRepository {
    pub async fn load_session(
        pool: &DatabasePool,
        profile_id: impl Into<String>,
    ) -> Result<Option<UiSessionRecord>> {
        let profile_id = profile_id.into();
        pool.with_conn(move |c| {
            let mut stmt = c.prepare(
                "SELECT profile_id, schema_version, active_route, active_conversation_id, \
                        active_branch_id, timeline_anchor_message_id, selected_model_id, \
                        payload_json, updated_at \
                 FROM ui_session_state WHERE profile_id = ?1",
            )?;
            let mut rows = stmt.query([profile_id])?;
            if let Some(row) = rows.next()? {
                Ok::<_, DbError>(Some(UiSessionRecord {
                    profile_id: row.get(0)?,
                    schema_version: row.get(1)?,
                    active_route: row.get(2)?,
                    active_conversation_id: row.get(3)?,
                    active_branch_id: row.get(4)?,
                    timeline_anchor_message_id: row.get(5)?,
                    selected_model_id: row.get(6)?,
                    payload_json: row.get(7)?,
                    updated_at: row.get(8)?,
                }))
            } else {
                Ok(None)
            }
        })
        .await
    }

    pub async fn save_session(pool: &DatabasePool, mut record: UiSessionRecord) -> Result<()> {
        Self::validate_session(&record)?;
        record.updated_at = chrono::Utc::now().to_rfc3339();
        pool.with_conn(move |c| {
            c.execute(
                "INSERT INTO ui_session_state (profile_id, schema_version, active_route, \
                    active_conversation_id, active_branch_id, timeline_anchor_message_id, \
                    selected_model_id, payload_json, updated_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9) \
                 ON CONFLICT(profile_id) DO UPDATE SET \
                    schema_version = excluded.schema_version, \
                    active_route = excluded.active_route, \
                    active_conversation_id = excluded.active_conversation_id, \
                    active_branch_id = excluded.active_branch_id, \
                    timeline_anchor_message_id = excluded.timeline_anchor_message_id, \
                    selected_model_id = excluded.selected_model_id, \
                    payload_json = excluded.payload_json, \
                    updated_at = excluded.updated_at",
                rusqlite::params![
                    record.profile_id,
                    record.schema_version,
                    record.active_route,
                    record.active_conversation_id,
                    record.active_branch_id,
                    record.timeline_anchor_message_id,
                    record.selected_model_id,
                    record.payload_json,
                    record.updated_at,
                ],
            )?;
            Ok::<_, DbError>(())
        })
        .await?;
        Ok(())
    }

    pub async fn load_draft(
        pool: &DatabasePool,
        conversation_id: impl Into<String>,
        branch_id: impl Into<String>,
    ) -> Result<Option<UiDraftRecord>> {
        let conversation_id = normalized_scope(conversation_id.into(), "default");
        let branch_id = normalized_scope(branch_id.into(), "main");
        pool.with_conn(move |c| {
            let mut stmt = c.prepare(
                "SELECT conversation_id, branch_id, text, cursor_position, \
                        attachment_refs_json, updated_at \
                 FROM ui_drafts WHERE conversation_id = ?1 AND branch_id = ?2",
            )?;
            let mut rows = stmt.query(rusqlite::params![conversation_id, branch_id])?;
            if let Some(row) = rows.next()? {
                Ok::<_, DbError>(Some(UiDraftRecord {
                    conversation_id: row.get(0)?,
                    branch_id: row.get(1)?,
                    text: row.get(2)?,
                    cursor_position: row.get(3)?,
                    attachment_refs_json: row.get(4)?,
                    updated_at: row.get(5)?,
                }))
            } else {
                Ok(None)
            }
        })
        .await
    }

    pub async fn save_draft(pool: &DatabasePool, mut draft: UiDraftRecord) -> Result<()> {
        draft.conversation_id = normalized_scope(draft.conversation_id, "default");
        draft.branch_id = normalized_scope(draft.branch_id, "main");
        if draft.text.len() > 256 * 1024 {
            return Err(MukeiError::ConfigInvalid {
                field: "ui_draft.text".into(),
                reason: "draft exceeds 256 KiB".into(),
            });
        }
        if draft.cursor_position < 0 {
            draft.cursor_position = 0;
        }
        if !serde_json::from_str::<serde_json::Value>(&draft.attachment_refs_json)
            .map(|value| value.is_array())
            .unwrap_or(false)
        {
            return Err(MukeiError::ConfigInvalid {
                field: "ui_draft.attachment_refs_json".into(),
                reason: "attachment references must be a JSON array".into(),
            });
        }
        draft.updated_at = chrono::Utc::now().to_rfc3339();
        pool.with_conn(move |c| {
            c.execute(
                "INSERT INTO ui_drafts (conversation_id, branch_id, text, cursor_position, \
                    attachment_refs_json, updated_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6) \
                 ON CONFLICT(conversation_id, branch_id) DO UPDATE SET \
                    text = excluded.text, \
                    cursor_position = excluded.cursor_position, \
                    attachment_refs_json = excluded.attachment_refs_json, \
                    updated_at = excluded.updated_at",
                rusqlite::params![
                    draft.conversation_id,
                    draft.branch_id,
                    draft.text,
                    draft.cursor_position,
                    draft.attachment_refs_json,
                    draft.updated_at,
                ],
            )?;
            Ok::<_, DbError>(())
        })
        .await?;
        Ok(())
    }

    pub async fn clear_draft(
        pool: &DatabasePool,
        conversation_id: impl Into<String>,
        branch_id: impl Into<String>,
    ) -> Result<()> {
        let conversation_id = normalized_scope(conversation_id.into(), "default");
        let branch_id = normalized_scope(branch_id.into(), "main");
        pool.with_conn(move |c| {
            c.execute(
                "DELETE FROM ui_drafts WHERE conversation_id = ?1 AND branch_id = ?2",
                rusqlite::params![conversation_id, branch_id],
            )?;
            Ok::<_, DbError>(())
        })
        .await?;
        Ok(())
    }

    fn validate_session(record: &UiSessionRecord) -> Result<()> {
        if record.profile_id.trim().is_empty() {
            return Err(MukeiError::ConfigInvalid {
                field: "ui_session.profile_id".into(),
                reason: "profile ID is required".into(),
            });
        }
        if record.schema_version <= 0 || record.schema_version > UI_SESSION_SCHEMA_VERSION {
            return Err(MukeiError::ConfigInvalid {
                field: "ui_session.schema_version".into(),
                reason: "unsupported UI session schema".into(),
            });
        }
        if record.active_route.len() > 128 {
            return Err(MukeiError::ConfigInvalid {
                field: "ui_session.active_route".into(),
                reason: "route is too long".into(),
            });
        }
        let payload = serde_json::from_str::<serde_json::Value>(&record.payload_json).map_err(|_| {
            MukeiError::ConfigInvalid {
                field: "ui_session.payload_json".into(),
                reason: "payload must be valid JSON".into(),
            }
        })?;
        if !payload.is_object() {
            return Err(MukeiError::ConfigInvalid {
                field: "ui_session.payload_json".into(),
                reason: "payload must be a JSON object".into(),
            });
        }
        Ok(())
    }
}

fn normalized_scope(value: String, fallback: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        fallback.into()
    } else {
        trimmed.chars().take(128).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Migrator;

    async fn migrated_pool() -> DatabasePool {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ui-session.db");
        let pool = DatabasePool::open(&path).unwrap();
        Migrator::embedded().apply_pending(&pool).await.unwrap();
        std::mem::forget(dir);
        pool
    }

    #[tokio::test]
    async fn session_and_draft_round_trip() {
        let pool = migrated_pool().await;
        let record = UiSessionRecord {
            active_route: "chat".into(),
            active_conversation_id: Some("conversation".into()),
            active_branch_id: Some("branch".into()),
            payload_json: "{\"expanded\":[]}".into(),
            ..UiSessionRecord::default()
        };
        UiSessionRepository::save_session(&pool, record.clone())
            .await
            .unwrap();
        let loaded = UiSessionRepository::load_session(&pool, DEFAULT_UI_PROFILE)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(loaded.active_route, "chat");
        assert_eq!(loaded.active_conversation_id.as_deref(), Some("conversation"));

        UiSessionRepository::save_draft(
            &pool,
            UiDraftRecord {
                conversation_id: "conversation".into(),
                branch_id: "branch".into(),
                text: "draft".into(),
                cursor_position: 5,
                attachment_refs_json: "[]".into(),
                updated_at: String::new(),
            },
        )
        .await
        .unwrap();
        let draft = UiSessionRepository::load_draft(&pool, "conversation", "branch")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(draft.text, "draft");
        UiSessionRepository::clear_draft(&pool, "conversation", "branch")
            .await
            .unwrap();
        assert!(UiSessionRepository::load_draft(&pool, "conversation", "branch")
            .await
            .unwrap()
            .is_none());
    }
}
