//! UI-session database projection helpers.
//!
//! These functions perform SQLite work only from runtime workers. Plaintext
//! database keys are never accepted by or returned from this module.

#[cfg(feature = "rusqlite")]
pub(crate) async fn ui_session_snapshot(
    pool: &mukei_core::storage::DatabasePool,
) -> Result<serde_json::Value, mukei_core::error::MukeiError> {
    let session = mukei_core::storage::UiSessionRepository::load_session(
        pool,
        mukei_core::storage::DEFAULT_UI_PROFILE,
    )
    .await?;
    let active_draft = if let Some(record) = session.as_ref() {
        match (
            record.active_conversation_id.as_deref(),
            record.active_branch_id.as_deref(),
        ) {
            (Some(conversation_id), Some(branch_id))
                if !conversation_id.is_empty() && !branch_id.is_empty() =>
            {
                mukei_core::storage::UiSessionRepository::load_draft(
                    pool,
                    conversation_id.to_string(),
                    branch_id.to_string(),
                )
                .await?
            }
            _ => None,
        }
    } else {
        None
    };
    Ok(serde_json::json!({
        "session": session,
        "active_draft": active_draft,
    }))
}

#[cfg(feature = "rusqlite")]
pub(crate) async fn draft_snapshot(
    pool: &mukei_core::storage::DatabasePool,
    conversation_id: String,
    branch_id: String,
) -> Result<serde_json::Value, mukei_core::error::MukeiError> {
    let draft = mukei_core::storage::UiSessionRepository::load_draft(
        pool,
        conversation_id.clone(),
        branch_id.clone(),
    )
    .await?;
    Ok(serde_json::json!({
        "conversation_id": conversation_id,
        "branch_id": branch_id,
        "draft": draft,
    }))
}
