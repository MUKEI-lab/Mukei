//! Private-document durable projection helpers.
//!
//! Permission acquisition/release remains orchestrated by the Qt entrypoint
//! because it must correlate an accepted request with the existing platform
//! callback surface. Database projection work is isolated here.

#[cfg(feature = "rusqlite")]
pub(crate) async fn document_list_snapshot(
    pool: &mukei_core::storage::DatabasePool,
    limit: usize,
) -> Result<serde_json::Value, mukei_core::error::MukeiError> {
    let documents = mukei_core::storage::SafRegistry::list_document_projections(pool, limit).await?;
    serde_json::to_value(documents).map_err(|error| {
        mukei_core::error::MukeiError::Internal(format!(
            "document projection serialization failed: {error}"
        ))
    })
}
