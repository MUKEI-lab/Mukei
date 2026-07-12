//! Download-domain durable projection helpers.

#[cfg(feature = "rusqlite")]
pub(crate) async fn recent_jobs_snapshot(
    pool: &mukei_core::storage::DatabasePool,
    limit: usize,
) -> Result<serde_json::Value, mukei_core::error::MukeiError> {
    let jobs = mukei_core::storage::DownloadJobRepository::list_recent(pool, limit).await?;
    serde_json::to_value(jobs).map_err(|error| {
        mukei_core::error::MukeiError::Internal(format!(
            "download projection serialization failed: {error}"
        ))
    })
}
