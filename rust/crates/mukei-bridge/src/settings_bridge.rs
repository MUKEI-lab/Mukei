//! Settings-domain durable projection helpers.

#[cfg(feature = "rusqlite")]
pub(crate) async fn settings_snapshot(
    pool: &mukei_core::storage::DatabasePool,
) -> Result<serde_json::Value, mukei_core::error::MukeiError> {
    let settings = mukei_core::storage::SettingsRepository::list_preferences(pool).await?;
    serde_json::to_value(settings).map_err(|error| {
        mukei_core::error::MukeiError::Internal(format!(
            "settings projection serialization failed: {error}"
        ))
    })
}
