//! Crash-recovery cleanup for app-private staged plaintext.
//!
//! Only journal entries in terminal states are eligible. Non-terminal imports
//! remain untouched so recovery can resume without silently losing source data.

use crate::error::Result;
use crate::storage::pool::{DatabasePool, DbError, PooledConnectionExt};
use std::fs;
use std::path::{Component, Path, PathBuf};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StagedCleanupReport {
    pub removed: usize,
    pub missing: usize,
    pub unsafe_paths: usize,
    pub retained_non_files: usize,
}

pub struct StagedPlaintextCleanup;

impl StagedPlaintextCleanup {
    /// Removes residual staged plaintext belonging only to completed, cancelled,
    /// or failed imports. Paths are interpreted relative to the canonical staging
    /// root and are rejected if they contain traversal or resolve outside it.
    pub async fn sweep_terminal(
        pool: &DatabasePool,
        staging_root: impl Into<PathBuf>,
    ) -> Result<StagedCleanupReport> {
        let relative_paths = pool
            .with_conn(|connection| {
                let mut statement = connection.prepare(
                    "SELECT staging_relative_path FROM import_transactions \
                     WHERE state IN ('completed', 'cancelled', 'failed') \
                     ORDER BY updated_at ASC, transaction_id ASC",
                )?;
                let paths = statement
                    .query_map([], |row| row.get::<_, String>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok::<_, DbError>(paths)
            })
            .await?;

        let staging_root = staging_root.into();
        tokio::task::spawn_blocking(move || sweep_paths(&staging_root, relative_paths))
            .await
            .map_err(|error| crate::error::MukeiError::Invariant(format!(
                "staged cleanup task failed: {error}"
            )))
    }
}

fn sweep_paths(root: &Path, relative_paths: Vec<String>) -> Result<StagedCleanupReport> {
    fs::create_dir_all(root)?;
    let canonical_root = fs::canonicalize(root)?;
    let mut report = StagedCleanupReport::default();

    for relative in relative_paths {
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
            Err(error) => return Err(error.into()),
        };

        if metadata.file_type().is_symlink() {
            report.unsafe_paths += 1;
            continue;
        }
        if !metadata.is_file() {
            report.retained_non_files += 1;
            continue;
        }

        let canonical_candidate = fs::canonicalize(&candidate)?;
        if !canonical_candidate.starts_with(&canonical_root) {
            report.unsafe_paths += 1;
            continue;
        }

        fs::remove_file(canonical_candidate)?;
        report.removed += 1;
    }

    Ok(report)
}

fn is_safe_relative_path(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && !path.is_absolute()
        && path.components().all(|component| {
            matches!(component, Component::Normal(_) | Component::CurDir)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::migrations::Migrator;
    use crate::storage::universal::{ChatId, DuplicatePolicy};
    use crate::storage::universal_repository::UniversalStorageRepository;
    use crate::storage::{ImportJournalRepository, ImportState};

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

        let done = ImportJournalRepository::create(
            &pool,
            workspace.scope_id,
            workspace.uploaded_files_node_id(),
            "done.txt".to_string(),
            "done.txt".to_string(),
            Some(4),
            None,
        )
        .await
        .unwrap();
        ImportJournalRepository::transition(&pool, done, ImportState::Failed, None, None)
            .await
            .unwrap();

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
        assert_eq!(report.removed, 1);
        assert!(!staging.join("done.txt").exists());
        assert!(staging.join("active.txt").exists());
        let _ = DuplicatePolicy::RenameNewEntry;
    }

    #[test]
    fn rejects_absolute_and_parent_traversal_paths() {
        assert!(!is_safe_relative_path(Path::new("../escape.txt")));
        assert!(!is_safe_relative_path(Path::new("/absolute.txt")));
        assert!(is_safe_relative_path(Path::new("nested/file.txt")));
    }
}