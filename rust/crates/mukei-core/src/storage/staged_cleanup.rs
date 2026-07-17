//! Crash-recovery cleanup for app-private staged plaintext.
//!
//! Only journal entries in terminal states are eligible. Non-terminal imports
//! remain untouched so recovery can resume without silently losing source data.

use crate::error::{MukeiError, Result};
use crate::storage::pool::{DatabasePool, DbError, PooledConnectionExt};
use std::fs;
use std::path::{Component, Path, PathBuf};

const DEFAULT_SWEEP_LIMIT: usize = 256;
const MAX_SWEEP_LIMIT: usize = 4096;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StagedCleanupReport {
    pub examined: usize,
    pub removed: usize,
    pub missing: usize,
    pub unsafe_paths: usize,
    pub retained_non_files: usize,
}

pub struct StagedPlaintextCleanup;

impl StagedPlaintextCleanup {
    /// Removes a bounded batch of residual staged plaintext belonging only to
    /// completed, cancelled, or failed imports.
    pub async fn sweep_terminal(
        pool: &DatabasePool,
        staging_root: impl Into<PathBuf>,
    ) -> Result<StagedCleanupReport> {
        Self::sweep_terminal_batch(pool, staging_root, DEFAULT_SWEEP_LIMIT).await
    }

    /// Removes at most `limit` terminal journal entries. Bounding each sweep
    /// prevents an unexpectedly large or corrupted journal from monopolizing
    /// memory or a blocking worker during startup recovery.
    pub async fn sweep_terminal_batch(
        pool: &DatabasePool,
        staging_root: impl Into<PathBuf>,
        limit: usize,
    ) -> Result<StagedCleanupReport> {
        if !(1..=MAX_SWEEP_LIMIT).contains(&limit) {
            return Err(MukeiError::Invariant(format!(
                "staged cleanup limit must be between 1 and {MAX_SWEEP_LIMIT}"
            )));
        }
        let query_limit = i64::try_from(limit)
            .map_err(|_| MukeiError::Invariant("staged cleanup limit overflow".to_string()))?;
        let relative_paths = pool
            .with_conn(move |connection| {
                let mut statement = connection.prepare(
                    "SELECT staging_relative_path FROM import_transactions \
                     WHERE state IN ('completed', 'cancelled', 'failed') \
                     ORDER BY updated_at ASC, transaction_id ASC LIMIT ?1",
                )?;
                let paths = statement
                    .query_map([query_limit], |row| row.get::<_, String>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok::<_, DbError>(paths)
            })
            .await?;

        let staging_root = staging_root.into();
        tokio::task::spawn_blocking(move || sweep_paths(&staging_root, relative_paths))
            .await
            .map_err(|error| MukeiError::BlockingJoinFailed(error.to_string()))?
    }
}

fn sweep_paths(root: &Path, relative_paths: Vec<String>) -> Result<StagedCleanupReport> {
    fs::create_dir_all(root).map_err(cleanup_io_error)?;
    let canonical_root = fs::canonicalize(root).map_err(cleanup_io_error)?;
    let mut report = StagedCleanupReport::default();

    for relative in relative_paths {
        report.examined += 1;
        let relative = Path::new(&relative);
        if !is_safe_relative_path(relative) {
            report.unsafe_paths += 1;
            continue;
        }

        let candidate = canonical_root.join(relative);
        let metadata = match fs::symlink_metadata(&candidate) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                report.missing += 1;
                continue;
            }
            Err(error) => return Err(cleanup_io_error(error)),
        };

        if metadata.file_type().is_symlink() {
            report.unsafe_paths += 1;
            continue;
        }
        if !metadata.is_file() {
            report.retained_non_files += 1;
            continue;
        }

        let canonical_candidate = fs::canonicalize(&candidate).map_err(cleanup_io_error)?;
        if !canonical_candidate.starts_with(&canonical_root) {
            report.unsafe_paths += 1;
            continue;
        }

        fs::remove_file(canonical_candidate).map_err(cleanup_io_error)?;
        report.removed += 1;
    }

    Ok(report)
}

fn cleanup_io_error(error: std::io::Error) -> MukeiError {
    MukeiError::Invariant(format!("staged plaintext cleanup I/O failed: {}", error.kind()))
}

fn is_safe_relative_path(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && !path.is_absolute()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_) | Component::CurDir))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::migrations::Migrator;
    use crate::storage::universal::ChatId;
    use crate::storage::universal_repository::UniversalStorageRepository;
    use crate::storage::{ImportJournalRepository, ImportState};

    async fn failed_import(
        pool: &DatabasePool,
        workspace: &crate::storage::universal_repository::PersistedWorkspace,
        relative_path: &str,
        size: u64,
    ) {
        let transaction = ImportJournalRepository::create(
            pool,
            workspace.scope_id,
            workspace.uploaded_files_node_id(),
            relative_path.to_string(),
            relative_path.to_string(),
            Some(size),
            None,
        )
        .await
        .unwrap();
        ImportJournalRepository::transition(pool, transaction, ImportState::Failed, None, None)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn removes_only_terminal_staged_plaintext() {
        let root = tempfile::tempdir().unwrap();
        let staging = root.path().join("staging");
        fs::create_dir_all(&staging).unwrap();
        fs::write(staging.join("done.txt"), b"done").unwrap();
        fs::write(staging.join("active.txt"), b"active").unwrap();

        let pool = DatabasePool::open(&root.path().join("storage.db")).unwrap();
        Migrator::embedded().apply_pending(&pool).await.unwrap();
        let workspace = UniversalStorageRepository::ensure_workspace(
            &pool,
            ChatId::parse("cleanup-chat").unwrap(),
        )
        .await
        .unwrap();

        failed_import(&pool, &workspace, "done.txt", 4).await;
        let _active = ImportJournalRepository::create(
            &pool,
            workspace.scope_id,
            workspace.uploaded_files_node_id(),
            "active.txt".to_string(),
            "active.txt".to_string(),
            Some(6),
            None,
        )
        .await
        .unwrap();

        let report = StagedPlaintextCleanup::sweep_terminal(&pool, &staging)
            .await
            .unwrap();
        assert_eq!(report.examined, 1);
        assert_eq!(report.removed, 1);
        assert!(!staging.join("done.txt").exists());
        assert!(staging.join("active.txt").exists());
    }

    #[tokio::test]
    async fn bounds_each_recovery_sweep() {
        let root = tempfile::tempdir().unwrap();
        let staging = root.path().join("staging");
        fs::create_dir_all(&staging).unwrap();
        fs::write(staging.join("first.txt"), b"first").unwrap();
        fs::write(staging.join("second.txt"), b"second").unwrap();

        let pool = DatabasePool::open(&root.path().join("storage.db")).unwrap();
        Migrator::embedded().apply_pending(&pool).await.unwrap();
        let workspace = UniversalStorageRepository::ensure_workspace(
            &pool,
            ChatId::parse("bounded-cleanup-chat").unwrap(),
        )
        .await
        .unwrap();
        failed_import(&pool, &workspace, "first.txt", 5).await;
        failed_import(&pool, &workspace, "second.txt", 6).await;

        let report = StagedPlaintextCleanup::sweep_terminal_batch(&pool, &staging, 1)
            .await
            .unwrap();
        assert_eq!(report.examined, 1);
        assert_eq!(report.removed, 1);
        assert_eq!(
            usize::from(staging.join("first.txt").exists())
                + usize::from(staging.join("second.txt").exists()),
            1
        );
    }

    #[tokio::test]
    async fn rejects_invalid_recovery_limits() {
        let root = tempfile::tempdir().unwrap();
        let pool = DatabasePool::open(&root.path().join("storage.db")).unwrap();
        Migrator::embedded().apply_pending(&pool).await.unwrap();

        assert!(StagedPlaintextCleanup::sweep_terminal_batch(&pool, root.path(), 0)
            .await
            .is_err());
        assert!(StagedPlaintextCleanup::sweep_terminal_batch(
            &pool,
            root.path(),
            MAX_SWEEP_LIMIT + 1,
        )
        .await
        .is_err());
    }

    #[test]
    fn rejects_absolute_and_parent_traversal_paths() {
        assert!(!is_safe_relative_path(Path::new("../escape.txt")));
        assert!(!is_safe_relative_path(Path::new("/absolute.txt")));
        assert!(is_safe_relative_path(Path::new("nested/file.txt")));
    }
}
