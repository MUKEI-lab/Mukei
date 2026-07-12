//! Recovery-domain persistence projection helpers.
//!
//! CXX-Qt entrypoints remain in `lib.rs`; this module owns the database work
//! needed to build a recovery snapshot so the Qt-facing wrapper stays thin.

#[cfg(feature = "rusqlite")]
pub(crate) async fn interrupted_turn_snapshot(
    pool: &mukei_core::storage::DatabasePool,
) -> Result<serde_json::Value, mukei_core::error::MukeiError> {
    Ok(match mukei_core::storage::RecoveryStore::interrupted_turn(pool).await? {
        Some(turn) => serde_json::json!({
            "conversation_id": turn.conversation.0.to_string(),
            "branch_id": turn.branch.0.to_string(),
            "user_message_id": turn.user_message_id.0.to_string(),
            "interrupted_assistant_id": turn.interrupted_assistant_id.0.to_string(),
            "user_content": turn.user_content,
            "generated_prefix": turn.generated_prefix,
            "model_fingerprint": turn.model_fingerprint,
            "updated_at": turn.updated_at.to_rfc3339(),
        }),
        None => serde_json::Value::Null,
    })
}
