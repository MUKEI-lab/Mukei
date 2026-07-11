//! Local storage quota accounting for model/cache style files.
//!
//! This is intentionally filesystem-only: it can run before any SQLite
//! download-job table exists, and it gives mobile callers a deterministic
//! app-level guard before a multi-GB model stream starts writing.

use std::path::{Path, PathBuf};

use crate::error::{MukeiError, Result};

/// Default total budget for verified model files plus `.partial` files.
pub const DEFAULT_MAX_MODEL_STORAGE_BYTES: u64 = 32 * 1024 * 1024 * 1024;

/// Default cap for stale/in-progress partial files under the model root.
pub const DEFAULT_MAX_PARTIAL_STORAGE_BYTES: u64 = 18 * 1024 * 1024 * 1024;

/// Quota knobs for storage roots managed by the app.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StorageQuotaPolicy {
    /// Maximum bytes allowed for verified models plus partials.
    pub max_model_storage_bytes: u64,
    /// Maximum bytes allowed for `.partial` files.
    pub max_partial_storage_bytes: u64,
    /// Maximum bytes accepted for one new download.
    pub max_single_download_bytes: u64,
}

impl Default for StorageQuotaPolicy {
    fn default() -> Self {
        Self {
            max_model_storage_bytes: DEFAULT_MAX_MODEL_STORAGE_BYTES,
            max_partial_storage_bytes: DEFAULT_MAX_PARTIAL_STORAGE_BYTES,
            max_single_download_bytes: super::model_download::MAX_MODEL_DOWNLOAD_BYTES,
        }
    }
}

/// Current byte usage under a managed storage root.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StorageUsage {
    /// Verified/non-partial model bytes.
    pub model_bytes: u64,
    /// Bytes in `.partial` files.
    pub partial_bytes: u64,
    /// All regular-file bytes found under the root.
    pub total_bytes: u64,
}

impl StorageUsage {
    /// Bytes that count against the model-storage quota.
    pub fn accounted_model_bytes(&self) -> u64 {
        self.model_bytes.saturating_add(self.partial_bytes)
    }
}

/// Filesystem quota manager for app-private model storage.
#[derive(Clone, Debug)]
pub struct StorageQuotaManager {
    root: PathBuf,
    policy: StorageQuotaPolicy,
}

impl StorageQuotaManager {
    /// Create a quota manager using the default mobile-safe policy.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self::with_policy(root, StorageQuotaPolicy::default())
    }

    /// Create a quota manager with an explicit policy, useful for tests
    /// and product-flavor overrides.
    pub fn with_policy(root: impl Into<PathBuf>, policy: StorageQuotaPolicy) -> Self {
        Self {
            root: root.into(),
            policy,
        }
    }

    /// Return current recursive usage. Missing roots count as empty.
    pub fn usage(&self) -> Result<StorageUsage> {
        usage_for_root(&self.root)
    }

    /// Reject a model download before the network stream starts writing
    /// if either the single-download cap or aggregate model quota would
    /// be exceeded.
    pub fn ensure_model_download_allowed(&self, expected_bytes: u64) -> Result<StorageUsage> {
        if expected_bytes > self.policy.max_single_download_bytes {
            return Err(MukeiError::DownloadTooLarge {
                max_bytes: self.policy.max_single_download_bytes,
                actual_bytes: expected_bytes,
            });
        }

        let usage = self.usage()?;
        if usage.partial_bytes > self.policy.max_partial_storage_bytes {
            return Err(MukeiError::StorageQuotaExceeded {
                max_bytes: self.policy.max_partial_storage_bytes,
                requested_bytes: 0,
                used_bytes: usage.partial_bytes,
            });
        }

        let used = usage.accounted_model_bytes();
        if used.saturating_add(expected_bytes) > self.policy.max_model_storage_bytes {
            return Err(MukeiError::StorageQuotaExceeded {
                max_bytes: self.policy.max_model_storage_bytes,
                requested_bytes: expected_bytes,
                used_bytes: used,
            });
        }

        Ok(usage)
    }
}

fn usage_for_root(root: &Path) -> Result<StorageUsage> {
    let mut usage = StorageUsage::default();
    if !root.exists() {
        return Ok(usage);
    }
    visit_usage(root, &mut usage)?;
    Ok(usage)
}

fn visit_usage(path: &Path, usage: &mut StorageUsage) -> Result<()> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(MukeiError::Io(format!("storage quota metadata: {err}"))),
    };

    if metadata.file_type().is_symlink() {
        return Ok(());
    }
    if metadata.is_file() {
        let len = metadata.len();
        usage.total_bytes = usage.total_bytes.saturating_add(len);
        if path.extension().and_then(|ext| ext.to_str()) == Some("partial") {
            usage.partial_bytes = usage.partial_bytes.saturating_add(len);
        } else {
            usage.model_bytes = usage.model_bytes.saturating_add(len);
        }
        return Ok(());
    }
    if metadata.is_dir() {
        let entries = std::fs::read_dir(path)
            .map_err(|err| MukeiError::Io(format!("storage quota read_dir: {err}")))?;
        for entry in entries {
            let entry =
                entry.map_err(|err| MukeiError::Io(format!("storage quota entry: {err}")))?;
            visit_usage(&entry.path(), usage)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_bytes(path: &Path, bytes: usize) {
        let mut file = std::fs::File::create(path).unwrap();
        file.write_all(&vec![7_u8; bytes]).unwrap();
    }

    #[test]
    fn usage_counts_model_and_partial_bytes() {
        let dir = tempfile::tempdir().unwrap();
        write_bytes(&dir.path().join("a.gguf"), 11);
        write_bytes(&dir.path().join("b.gguf.partial"), 13);

        let usage = StorageQuotaManager::new(dir.path()).usage().unwrap();
        assert_eq!(usage.model_bytes, 11);
        assert_eq!(usage.partial_bytes, 13);
        assert_eq!(usage.total_bytes, 24);
    }

    #[test]
    fn preflight_rejects_when_aggregate_model_quota_would_be_exceeded() {
        let dir = tempfile::tempdir().unwrap();
        write_bytes(&dir.path().join("existing.gguf"), 80);
        let policy = StorageQuotaPolicy {
            max_model_storage_bytes: 100,
            max_partial_storage_bytes: 100,
            max_single_download_bytes: 100,
        };
        let manager = StorageQuotaManager::with_policy(dir.path(), policy);

        let err = manager.ensure_model_download_allowed(25).unwrap_err();
        assert!(matches!(
            err,
            MukeiError::StorageQuotaExceeded {
                max_bytes: 100,
                requested_bytes: 25,
                used_bytes: 80,
            }
        ));
    }

    #[test]
    fn preflight_rejects_when_partial_quota_is_already_exceeded() {
        let dir = tempfile::tempdir().unwrap();
        write_bytes(&dir.path().join("stale.gguf.partial"), 12);
        let policy = StorageQuotaPolicy {
            max_model_storage_bytes: 100,
            max_partial_storage_bytes: 10,
            max_single_download_bytes: 100,
        };
        let manager = StorageQuotaManager::with_policy(dir.path(), policy);

        let err = manager.ensure_model_download_allowed(1).unwrap_err();
        assert!(matches!(
            err,
            MukeiError::StorageQuotaExceeded {
                max_bytes: 10,
                requested_bytes: 0,
                used_bytes: 12,
            }
        ));
    }
}
