//! Filesystem-backed storage projection helpers.
//!
//! `storage_snapshot` is synchronous by design because callers must execute it
//! inside the bounded runtime blocking pool, never on the Qt main thread.

pub(crate) fn storage_snapshot(
    model_root: &std::path::Path,
) -> Result<serde_json::Value, mukei_core::error::MukeiError> {
    let usage = mukei_core::storage::StorageQuotaManager::new(model_root).usage()?;
    let max_bytes = mukei_core::storage::DEFAULT_MAX_MODEL_STORAGE_BYTES;
    let accounted = usage.accounted_model_bytes();
    let ratio = if max_bytes == 0 {
        0.0
    } else {
        accounted as f64 / max_bytes as f64
    };
    let pressure = if ratio >= 0.95 {
        "critical"
    } else if ratio >= 0.80 {
        "warning"
    } else {
        "normal"
    };
    Ok(serde_json::json!({
        "model_bytes": usage.model_bytes,
        "partial_bytes": usage.partial_bytes,
        "total_bytes": usage.total_bytes,
        "accounted_model_bytes": accounted,
        "max_model_storage_bytes": max_bytes,
        "usage_ratio": ratio,
        "pressure": pressure
    }))
}
