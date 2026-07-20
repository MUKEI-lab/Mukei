//! `mukei_core::storage::migrations` — TRD §6.1.
//!
//! Strict append-only migration framework. The boot path runs
//! `Migrator::apply_pending` after opening the SQLCipher database.
//! Each `V###__name.sql` adds rows to `migrations_applied` and bumps
//! `PRAGMA user_version`. The order is *strictly* monotonic; conflict
//! produces `MukeiError::MigrationOrderConflict`.

use std::path::{Path, PathBuf};

use crate::error::{MukeiError, Result};

#[cfg(feature = "rusqlite")]
use rusqlite::OptionalExtension;

/// Subdirectory inside the project / data root that holds the raw
/// `.sql` migration files. Reference clone of the on-disk layout from
/// `BS v1.2` §14.
pub const MIGRATIONS_DIR: &str = "migrations";

/// File prefix. Each migration file is named `V{nnn}__{name}.sql`.
pub const MIGRATION_FILE_PREFIX: &str = "V";

/// One row in the `migrations_applied` SQLite table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MigrationRecord {
    pub id: u32,
    pub name: String,
    pub applied_at: chrono::DateTime<chrono::Utc>,
    pub checksum: String, // SHA-256 of the migration body
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MigrationBackup {
    pub path: PathBuf,
    pub from_version: u32,
    pub to_version: u32,
}

/// Pure-data migrator — does *not* open a SQLite handle. The core
/// migrator owns the schema; the bridge crate opens the real
/// connection and invokes `apply_one` on each row.
pub struct Migrator {
    source: MigrationSource,
}

enum MigrationSource {
    Directory(PathBuf),
    Embedded,
}

const LEGACY_V09_V008_TOMBSTONES: &str =
    include_str!("../../../../migrations/legacy/V008__schema_metadata_and_rag_tombstones.sql",);

const EMBEDDED_MIGRATIONS: &[(u32, &str, &str)] = &[
    (
        1,
        "V001__schema",
        include_str!("../../../../migrations/V001__schema.sql"),
    ),
    (
        2,
        "V002__recovery_state",
        include_str!("../../../../migrations/V002__recovery_state.sql"),
    ),
    (
        3,
        "V003__tooling_and_saf",
        include_str!("../../../../migrations/V003__tooling_and_saf.sql"),
    ),
    (
        4,
        "V004__branching",
        include_str!("../../../../migrations/V004__branching.sql"),
    ),
    (
        5,
        "V005__audit_chain_checks",
        include_str!("../../../../migrations/V005__audit_chain_checks.sql"),
    ),
    (
        6,
        "V006__branch_message_constraints",
        include_str!("../../../../migrations/V006__branch_message_constraints.sql"),
    ),
    (
        7,
        "V007__message_status",
        include_str!("../../../../migrations/V007__message_status.sql"),
    ),
    (
        8,
        "V008__settings_and_secret_refs",
        include_str!("../../../../migrations/V008__settings_and_secret_refs.sql"),
    ),
    (
        9,
        "V009__schema_metadata_and_rag_tombstones",
        include_str!("../../../../migrations/V009__schema_metadata_and_rag_tombstones.sql"),
    ),
    (
        10,
        "V010__reliability_hardening",
        include_str!("../../../../migrations/V010__reliability_hardening.sql"),
    ),
    (
        11,
        "V011__ui_projection_sessions",
        include_str!("../../../../migrations/V011__ui_projection_sessions.sql"),
    ),
    (
        12,
        "V012__document_access_and_ingestion_jobs",
        include_str!("../../../../migrations/V012__document_access_and_ingestion_jobs.sql"),
    ),
    (
        13,
        "V013__saas_tenancy_entitlements_usage_ledger",
        include_str!("../../../../migrations/V013__saas_tenancy_entitlements_usage_ledger.sql"),
    ),
    (
        14,
        "V014__universal_storage_and_workspaces",
        include_str!("../../../../migrations/V014__universal_storage_and_workspaces.sql"),
    ),
    (
        15,
        "V015__workspace_scope_isolation_guards",
        include_str!("../../../../migrations/V015__workspace_scope_isolation_guards.sql"),
    ),
    (
        16,
        "V016__storage_identity_and_recovery_hardening",
        include_str!("../../../../migrations/V016__storage_identity_and_recovery_hardening.sql"),
    ),
    (
        17,
        "V017__conversation_storage_attachments",
        include_str!("../../../../migrations/V017__conversation_storage_attachments.sql"),
    ),
];

const MIGRATION_LOCK_STALE_AFTER_SECS: i64 = 15 * 60;

fn table_exists(
    c: &mut super::pool::Conn,
    table: &str,
) -> std::result::Result<bool, super::pool::DbError> {
    let exists: i64 = c.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
        [table],
        |row| row.get(0),
    )?;
    Ok(exists != 0)
}

fn acquire_migration_lock(
    c: &mut super::pool::Conn,
) -> std::result::Result<String, super::pool::DbError> {
    let holder = uuid::Uuid::new_v4().to_string();
    let tx = c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let existing: Option<(String, String)> = tx
        .query_row(
            "SELECT holder, acquired_at FROM migration_lock WHERE id = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;

    if let Some((existing_holder, acquired_at)) = existing {
        let acquired = chrono::DateTime::parse_from_rfc3339(&acquired_at)
            .map(|value| value.with_timezone(&chrono::Utc))
            .ok();
        let is_stale = existing_holder == "bootstrap"
            || acquired
                .map(|value| {
                    chrono::Utc::now()
                        .signed_duration_since(value)
                        .num_seconds()
                        >= MIGRATION_LOCK_STALE_AFTER_SECS
                })
                .unwrap_or(true);
        if !is_stale {
            return Err(super::pool::DbError::Domain(MukeiError::MigrationLocked));
        }
        tx.execute("DELETE FROM migration_lock WHERE id = 1", [])?;
    }

    tx.execute(
        "INSERT INTO migration_lock (id, holder, acquired_at) VALUES (1, ?1, ?2)",
        rusqlite::params![&holder, chrono::Utc::now().to_rfc3339()],
    )?;
    tx.commit()?;
    Ok(holder)
}

fn release_migration_lock(
    c: &mut super::pool::Conn,
    holder: &str,
) -> std::result::Result<(), super::pool::DbError> {
    c.execute(
        "DELETE FROM migration_lock WHERE id = 1 AND holder = ?1",
        [holder],
    )?;
    Ok(())
}

impl Migrator {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self {
            source: MigrationSource::Directory(dir.into()),
        }
    }

    /// Build a migrator from SQL bundled into the Rust binary. This is
    /// the release/mobile boot path: Android packages do not contain the
    /// workspace source tree, so runtime DB migrations must not depend
    /// on `CARGO_MANIFEST_DIR` filesystem layout.
    pub fn embedded() -> Self {
        Self {
            source: MigrationSource::Embedded,
        }
    }

    pub fn dir(&self) -> &Path {
        match &self.source {
            MigrationSource::Directory(dir) => dir,
            MigrationSource::Embedded => Path::new("<embedded>"),
        }
    }

    pub fn latest_version(&self) -> Result<u32> {
        Ok(self
            .list_available()?
            .into_iter()
            .map(|(id, _, _)| id)
            .max()
            .unwrap_or(0))
    }

    /// Create an encrypted byte-for-byte backup before applying an
    /// upgrade. SQLCipher remains at rest because the database file is
    /// copied, not exported through plaintext SQL. The caller invokes this
    /// after opening the keyed pool but before any numbered migration.
    pub async fn create_pre_migration_backup(
        &self,
        pool: &super::pool::DatabasePool,
        database_path: &Path,
    ) -> Result<Option<MigrationBackup>> {
        use super::pool::PooledConnectionExt;
        let latest = self.latest_version()?;
        let current = pool
            .with_conn(|c| {
                if table_exists(c, "migrations_applied")? {
                    let value: i64 = c.query_row(
                        "SELECT COALESCE(MAX(version), 0) FROM migrations_applied ",
                        [],
                        |row| row.get(0),
                    )?;
                    Ok::<_, super::pool::DbError>(value.max(0) as u32)
                } else {
                    Ok::<_, super::pool::DbError>(0)
                }
            })
            .await?;
        if current == 0 || current >= latest || !database_path.exists() {
            return Ok(None);
        }

        // Flush WAL pages before the filesystem copy. No application state
        // is globally published at this boot stage, so there are no writers.
        pool.with_conn(|c| {
            c.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
            Ok::<_, super::pool::DbError>(())
        })
        .await?;

        let source = database_path.to_path_buf();
        let timestamp = chrono::Utc::now().format("%Y%m%dT%H%M%SZ");
        let backup_path = source.with_extension(format!(
            "premigration-v{current}-to-v{latest}-{timestamp}.bak"
        ));
        let backup_for_copy = backup_path.clone();
        tokio::task::spawn_blocking(move || -> std::io::Result<()> {
            std::fs::copy(&source, &backup_for_copy)?;
            std::fs::OpenOptions::new()
                .read(true)
                .open(&backup_for_copy)?
                .sync_all()?;
            Ok(())
        })
        .await
        .map_err(|error| MukeiError::BlockingJoinFailed(error.to_string()))?
        .map_err(|error| MukeiError::Io(error.to_string()))?;

        Ok(Some(MigrationBackup {
            path: backup_path,
            from_version: current,
            to_version: latest,
        }))
    }

    /// Read all migration files in the directory and return them in
    /// strict ascending order of `V{n}`.
    pub fn list_available(&self) -> Result<Vec<(u32, String, String)>> {
        if matches!(&self.source, MigrationSource::Embedded) {
            return Ok(EMBEDDED_MIGRATIONS
                .iter()
                .map(|(id, name, body)| (*id, (*name).to_string(), (*body).to_string()))
                .collect());
        }

        let MigrationSource::Directory(dir) = &self.source else {
            unreachable!("embedded migrations returned above");
        };
        let mut out = Vec::new();
        let entries = std::fs::read_dir(dir).map_err(|e| MukeiError::Io(e.to_string()))?;
        let mut sorted: Vec<_> = entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                p.extension().and_then(|e| e.to_str()) == Some("sql")
                    && p.file_name()
                        .and_then(|n| n.to_str())
                        .map(|n| n.starts_with(MIGRATION_FILE_PREFIX) && !n.ends_with("__down.sql"))
                        .unwrap_or(false)
            })
            .collect();
        sorted.sort();

        for path in sorted {
            let fname = path.file_name().and_then(|n| n.to_str()).ok_or_else(|| {
                MukeiError::Invariant(format!("migration file {:?} has no name", path))
            })?;
            let id: u32 = fname
                .split('_')
                .next()
                .and_then(|s| s.trim_start_matches('V').parse().ok())
                .ok_or_else(|| {
                    MukeiError::Invariant(format!(
                        "migration file {:?} has no V-prefix numeric id",
                        fname
                    ))
                })?;
            let body = std::fs::read_to_string(&path).map_err(|e| MukeiError::Io(e.to_string()))?;
            let name = fname.trim_end_matches(".sql").to_string();
            // Compute SHA-256 of the body for `migrations_applied.checksum`.
            use sha2::{Digest, Sha256};
            let mut h = Sha256::new();
            h.update(body.as_bytes());
            let checksum = crate::diagnostics::crash_logger::hex_helper(&h.finalize());
            out.push((id, name, body));
            // checksum is recorded by the bridge when it inserts the row
            // (consume-friendly API). Drop here to silence unused warning.
            let _ = checksum;
        }
        Ok(out)
    }

    /// Given the list of already-applied migrations, return the
    /// migrations from `available` that should still be applied.
    pub fn pending(
        available: &[(u32, String, String)],
        applied: &[MigrationRecord],
    ) -> Vec<(u32, String, String)> {
        let max_applied = applied.iter().map(|r| r.id).max().unwrap_or(0);
        available
            .iter()
            .filter(|(id, _, _)| *id > max_applied)
            .cloned()
            .collect()
    }

    /// Apply every pending migration inside its own SQLite transaction
    /// and record the row in `migrations_applied`. Returns the list of
    /// migrations that were applied in this call.
    ///
    /// PRD REQ-DB-04 (Versioned Migration Engine) — this is the single
    /// public entry point that centralises migration execution. The
    /// bridge crate MUST NOT issue ad-hoc DDL outside this path.
    pub async fn apply_pending(
        &self,
        pool: &super::pool::DatabasePool,
    ) -> Result<Vec<MigrationRecord>> {
        use super::pool::PooledConnectionExt;

        // Discover all bundled migrations and compute immutable checksums
        // before entering the blocking database closure.
        let mut bundle = Vec::new();
        for (id, name, body) in self.list_available()? {
            use sha2::{Digest, Sha256};
            let mut h = Sha256::new();
            h.update(body.as_bytes());
            let checksum = crate::diagnostics::crash_logger::hex_helper(&h.finalize());
            bundle.push((id, name, body, checksum));
        }

        pool.with_conn(move |c| {
            // Bootstrap the ledger and the lease table outside numbered
            // migrations. The lock must exist before V001 can safely run,
            // otherwise two first-boot processes could both apply V001.
            c.execute_batch(
                "CREATE TABLE IF NOT EXISTS migrations_applied ( \
                    version INTEGER PRIMARY KEY, \
                    name TEXT NOT NULL UNIQUE, \
                    applied_at TEXT NOT NULL, \
                    checksum TEXT, \
                    execution_ms INTEGER, \
                    success INTEGER NOT NULL DEFAULT 1 CHECK (success IN (0, 1)) \
                 ); \
                 CREATE TABLE IF NOT EXISTS migration_lock ( \
                    id INTEGER PRIMARY KEY CHECK (id = 1), \
                    holder TEXT NOT NULL, \
                    acquired_at TEXT NOT NULL \
                 );",
            )?;

            let lock_holder = acquire_migration_lock(c)?;
            let migration_result =
                (|| -> std::result::Result<Vec<MigrationRecord>, super::pool::DbError> {
                    // Read the already-applied set.
                    let mut stmt = c.prepare(
                        "SELECT version, name, applied_at, checksum \
                     FROM migrations_applied ORDER BY version",
                    )?;
                    let mut applied: Vec<MigrationRecord> = stmt
                        .query_map([], |row| {
                            let applied_at: String = row.get(2)?;
                            Ok(MigrationRecord {
                                id: row.get::<_, i64>(0)? as u32,
                                name: row.get(1)?,
                                applied_at: chrono::DateTime::parse_from_rfc3339(&applied_at)
                                    .map(|d| d.with_timezone(&chrono::Utc))
                                    .unwrap_or_else(|_| chrono::Utc::now()),
                                checksum: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                            })
                        })?
                        .collect::<rusqlite::Result<Vec<_>>>()?;
                    drop(stmt);

                    // Compatibility repair for the short-lived v0.9 lineage
                    // that accidentally reused V008 for tombstones. Only the
                    // exact known legacy body is accepted; unknown history
                    // remains fail-closed.
                    if let Some(record) = applied.iter_mut().find(|record| {
                        record.id == 8 && record.name == "V008__schema_metadata_and_rag_tombstones"
                    }) {
                        use sha2::{Digest, Sha256};
                        let mut legacy_hasher = Sha256::new();
                        legacy_hasher.update(LEGACY_V09_V008_TOMBSTONES.as_bytes());
                        let legacy_checksum =
                            crate::diagnostics::crash_logger::hex_helper(&legacy_hasher.finalize());

                        if record.checksum.is_empty() || record.checksum == legacy_checksum {
                            let (canonical_name, canonical_body, canonical_checksum) = bundle
                                .iter()
                                .find(|(id, _, _, _)| *id == 8)
                                .map(|(_, name, body, checksum)| {
                                    (name.clone(), body.clone(), checksum.clone())
                                })
                                .ok_or_else(|| {
                                    super::pool::DbError::Manager(
                                        "canonical V008 settings migration is missing".to_string(),
                                    )
                                })?;

                            let tx = c.transaction()?;
                            tx.execute_batch(&canonical_body)?;
                            tx.execute(
                                "UPDATE migrations_applied \
                             SET name = ?1, checksum = ?2 WHERE version = 8",
                                rusqlite::params![&canonical_name, &canonical_checksum],
                            )?;
                            tx.commit()?;

                            record.name = canonical_name;
                            record.checksum = canonical_checksum;
                        }
                    }

                    let available_brief: Vec<(u32, String, String)> = bundle
                        .iter()
                        .map(|(id, name, body, _)| (*id, name.clone(), body.clone()))
                        .collect();
                    let max_supported = available_brief
                        .iter()
                        .map(|(id, _, _)| *id)
                        .max()
                        .unwrap_or(0);

                    // Reject a DB explicitly marked as newer before any DDL.
                    if table_exists(c, "schema_metadata")? {
                        let last_migration: Option<i64> = c
                            .query_row(
                                "SELECT last_migration FROM schema_metadata WHERE id = 1",
                                [],
                                |row| row.get(0),
                            )
                            .optional()?;
                        if let Some(found) = last_migration {
                            let found = found.max(0) as u32;
                            if found > max_supported {
                                return Err(super::pool::DbError::Domain(
                                    MukeiError::SchemaTooNew {
                                        found,
                                        supported: max_supported,
                                    },
                                ));
                            }
                        }
                    }

                    Self::verify_order(&available_brief, &applied)
                        .map_err(super::pool::DbError::Domain)?;
                    Self::verify_checksums(&bundle, &applied)
                        .map_err(super::pool::DbError::Domain)?;
                    let pending_ids: Vec<u32> = Self::pending(&available_brief, &applied)
                        .iter()
                        .map(|(id, _, _)| *id)
                        .collect();

                    let mut applied_now = Vec::new();
                    for (id, name, body, checksum) in &bundle {
                        if !pending_ids.contains(id) {
                            continue;
                        }
                        let started = std::time::Instant::now();
                        let tx = c.transaction()?;
                        tx.execute_batch(body)?;
                        let now = chrono::Utc::now();
                        tx.execute(
                            "INSERT INTO migrations_applied \
                            (version, name, applied_at, checksum, execution_ms, success) \
                         VALUES (?1, ?2, ?3, ?4, ?5, 1)",
                            rusqlite::params![
                                *id as i64,
                                name,
                                now.to_rfc3339(),
                                checksum,
                                started.elapsed().as_millis() as i64,
                            ],
                        )?;
                        tx.commit()?;
                        applied_now.push(MigrationRecord {
                            id: *id,
                            name: name.clone(),
                            applied_at: now,
                            checksum: checksum.clone(),
                        });
                    }

                    // Keep metadata truthful even when no migration was pending.
                    if table_exists(c, "schema_metadata")? {
                        c.execute(
                            "UPDATE schema_metadata \
                         SET app_version = ?1, last_migration = ?2, applied_at = ?3 \
                         WHERE id = 1",
                            rusqlite::params![
                                env!("CARGO_PKG_VERSION"),
                                max_supported as i64,
                                chrono::Utc::now().to_rfc3339(),
                            ],
                        )?;
                    }

                    Ok(applied_now)
                })();

            let release_result = release_migration_lock(c, &lock_holder);
            match (migration_result, release_result) {
                (Err(error), _) => Err(error),
                (Ok(_), Err(error)) => Err(error),
                (Ok(records), Ok(())) => Ok(records),
            }
        })
        .await
    }

    /// Boot path asks for the conflict check. Returned result:
    ///  - `Ok(())` iff the applied IDs form a contiguous prefix of
    ///    `[1, 2, ...]` AND the maximum applied id does not exceed the
    ///    maximum available id.
    ///  - `Err(MukeiError::MigrationOrderConflict)` otherwise.
    ///
    /// Issue #12: the previous implementation only ran the contiguity
    /// check inside the `max_applied == max_avail` branch. A database
    /// whose applied set already had a gap (e.g. `[1, 3]` from manual
    /// tampering) would happily receive new migrations on top, leaving
    /// the schema in an unknown intermediate state. The contiguity
    /// check now runs unconditionally, before the high-water comparison.
    pub fn verify_order(
        available: &[(u32, String, String)],
        applied: &[MigrationRecord],
    ) -> Result<()> {
        // (1) Applied list MUST be contiguous from 1. Run this check
        //     first — a gap is always an order conflict regardless of
        //     whether new migrations are pending.
        let mut sorted = applied.iter().map(|r| r.id).collect::<Vec<_>>();
        sorted.sort();
        for (idx, id) in sorted.iter().enumerate() {
            if *id as usize != idx + 1 {
                return Err(MukeiError::MigrationOrderConflict {
                    expected: idx as u32 + 1,
                    applied: sorted,
                });
            }
        }

        // (2) The maximum applied id must not exceed the highest
        //     available id (would imply we forgot to ship a migration
        //     file the DB knows about).
        let max_applied = applied.iter().map(|r| r.id).max();
        let max_avail = available.iter().map(|(id, _, _)| *id).max();
        if let (Some(a), Some(v)) = (max_applied, max_avail) {
            if a > v {
                return Err(MukeiError::MigrationOrderConflict {
                    expected: v,
                    applied: applied.iter().map(|r| r.id).collect(),
                });
            }
        }
        Ok(())
    }

    /// Verify that every already-applied migration still matches the
    /// bundled SQL body. This fails fast when a historical migration
    /// file is edited after shipping.
    pub fn verify_checksums(
        bundled: &[(u32, String, String, String)],
        applied: &[MigrationRecord],
    ) -> Result<()> {
        for record in applied {
            let Some((_, _, _, bundled_checksum)) =
                bundled.iter().find(|(id, _, _, _)| *id == record.id)
            else {
                continue;
            };
            if !record.checksum.is_empty() && record.checksum != *bundled_checksum {
                return Err(MukeiError::MigrationChecksumMismatch {
                    version: record.id,
                    applied: record.checksum.clone(),
                    bundled: bundled_checksum.clone(),
                });
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_dir_holds_no_migrations() {
        let dir = tempfile::tempdir().unwrap();
        let m = Migrator::new(dir.path());
        let list = m.list_available().unwrap();
        assert_eq!(list.len(), 0);
    }

    #[test]
    fn list_available_ignores_down_migrations() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("V001__schema.sql"),
            "CREATE TABLE t (id INTEGER);",
        )
        .unwrap();
        std::fs::write(dir.path().join("V001__down.sql"), "DROP TABLE t;").unwrap();

        let m = Migrator::new(dir.path());
        let list = m.list_available().unwrap();

        assert_eq!(list.len(), 1);
        assert_eq!(list[0].1, "V001__schema");
        assert!(list[0].2.contains("CREATE TABLE"));
    }

    #[test]
    fn embedded_migrations_are_available_without_source_tree_scan() {
        let list = Migrator::embedded().list_available().unwrap();
        let ids: Vec<_> = list.iter().map(|(id, _, _)| *id).collect();
        assert_eq!(
            ids,
            vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17]
        );
        assert!(list.iter().any(|(_, name, body)| {
            name == "V007__message_status" && body.contains("ALTER TABLE messages")
        }));
        assert!(list.iter().any(|(_, name, body)| {
            name == "V008__settings_and_secret_refs"
                && body.contains("CREATE TABLE IF NOT EXISTS preferences")
                && body.contains("CREATE TABLE IF NOT EXISTS secret_refs")
        }));
        assert!(list.iter().any(|(_, name, body)| {
            name == "V009__schema_metadata_and_rag_tombstones"
                && body.contains("CREATE TABLE IF NOT EXISTS schema_metadata")
                && body.contains("CREATE TABLE IF NOT EXISTS migration_lock")
                && body.contains("CREATE TABLE IF NOT EXISTS document_tombstone")
        }));
        assert!(list.iter().any(|(_, name, body)| {
            name == "V010__reliability_hardening"
                && body.contains("CREATE TABLE IF NOT EXISTS download_jobs")
                && body.contains("CREATE TABLE IF NOT EXISTS storage_reservations")
                && body.contains("chunk_ids_json")
        }));
        assert!(list.iter().any(|(_, name, body)| {
            name == "V011__ui_projection_sessions"
                && body.contains("CREATE TABLE IF NOT EXISTS ui_session_state")
                && body.contains("CREATE TABLE IF NOT EXISTS ui_drafts")
        }));
        assert!(list.iter().any(|(_, name, body)| {
            name == "V012__document_access_and_ingestion_jobs"
                && body.contains("document_ingestion_jobs")
                && body.contains("os_permission_state")
        }));
    }

    #[test]
    fn applied_migration_checksum_mismatch_is_rejected() {
        let bundled = vec![(
            1,
            "V001__schema".to_string(),
            "CREATE TABLE t (id INTEGER);".to_string(),
            "bundled-checksum".to_string(),
        )];
        let applied = vec![MigrationRecord {
            id: 1,
            name: "V001__schema".into(),
            applied_at: chrono::Utc::now(),
            checksum: "old-checksum".into(),
        }];

        let err = Migrator::verify_checksums(&bundled, &applied).unwrap_err();
        assert!(matches!(
            err,
            MukeiError::MigrationChecksumMismatch { version: 1, .. }
        ));
    }

    #[test]
    fn gap_in_applied_set_with_pending_is_still_conflict() {
        // Issue #12 regression: the old gap-check only ran when there
        // was nothing left to apply. A DB at applied=[1,3] with new
        // migrations pending (avail=[1,2,3,4]) used to slip through.
        let avail = vec![
            (1, "a".into(), String::new()),
            (2, "b".into(), String::new()),
            (3, "c".into(), String::new()),
            (4, "d".into(), String::new()),
        ];
        let applied = vec![
            MigrationRecord {
                id: 1,
                name: "a".into(),
                applied_at: chrono::Utc::now(),
                checksum: "x".into(),
            },
            MigrationRecord {
                id: 3, // gap at 2
                name: "c".into(),
                applied_at: chrono::Utc::now(),
                checksum: "y".into(),
            },
        ];
        let err = Migrator::verify_order(&avail, &applied).unwrap_err();
        assert!(matches!(err, MukeiError::MigrationOrderConflict { .. }));
    }

    #[test]
    fn non_contiguous_applied_is_conflict() {
        let avail = vec![
            (1, "a".into(), String::new()),
            (2, "b".into(), String::new()),
        ];
        let applied = vec![
            MigrationRecord {
                id: 1,
                name: "a".into(),
                applied_at: chrono::Utc::now(),
                checksum: "x".into(),
            },
            MigrationRecord {
                id: 3, // skip 2
                name: "c".into(),
                applied_at: chrono::Utc::now(),
                checksum: "y".into(),
            },
        ];
        let err = Migrator::verify_order(&avail, &applied).unwrap_err();
        assert!(matches!(err, MukeiError::MigrationOrderConflict { .. }));
    }

    #[test]
    fn pending_list_filters_to_greater_than_applied() {
        let avail = vec![
            (1, "a".into(), String::new()),
            (2, "b".into(), String::new()),
            (3, "c".into(), String::new()),
        ];
        let applied = vec![MigrationRecord {
            id: 1,
            name: "a".into(),
            applied_at: chrono::Utc::now(),
            checksum: "x".into(),
        }];
        let pending = Migrator::pending(&avail, &applied);
        let ids: Vec<_> = pending.iter().map(|(i, _, _)| *i).collect();
        assert_eq!(ids, vec![2, 3]);
    }

    /// Architect review GH #37 — end-to-end rollback test.
    ///
    /// Verifies that when the DB is at `applied = [1, 3]` (a gap at
    /// version 2, simulating manual tampering or partial recovery) and
    /// the filesystem ships migrations `[1, 2, 3, 4]`, `apply_pending`:
    ///
    /// 1. Returns `MukeiError::MigrationOrderConflict` (not a partial
    ///    success, not silent forward progress on top of the gap).
    /// 2. Does NOT apply migration 4 — i.e. the post-conflict DB still
    ///    has exactly two rows in `migrations_applied`, the same two it
    ///    had before the boot path tried to apply pending.
    ///
    /// The order check runs BEFORE any DDL transaction (cf. line 166 in
    /// `apply_pending`), so a clean rollback is the contract this test
    /// pins. Issue #12 + GH #37.
    #[tokio::test]
    async fn apply_pending_with_gap_in_applied_set_rolls_back_cleanly() {
        use crate::storage::pool::{DatabasePool, PooledConnectionExt};

        // Write four numbered SQL files so the migrator's directory
        // scan sees `[1, 2, 3, 4]` as available.
        let mig_dir = tempfile::tempdir().unwrap();
        for (id, body) in [
            (1u32, "CREATE TABLE t1 (x INTEGER);"),
            (2u32, "CREATE TABLE t2 (x INTEGER);"),
            (3u32, "CREATE TABLE t3 (x INTEGER);"),
            (4u32, "CREATE TABLE t4 (x INTEGER);"),
        ] {
            let fname = format!("V{id:03}__test.sql");
            std::fs::write(mig_dir.path().join(fname), body).unwrap();
        }

        // Open a fresh SQLite pool and pre-populate `migrations_applied`
        // with [1, 3] (the gap scenario).
        let db_dir = tempfile::tempdir().unwrap();
        let db_path = db_dir.path().join("mukei-mig-test.db");
        let pool = DatabasePool::open(&db_path).unwrap();
        pool.with_conn(|c| {
            c.execute_batch(
                "CREATE TABLE migrations_applied ( \
                    version INTEGER PRIMARY KEY, \
                    name TEXT NOT NULL UNIQUE, \
                    applied_at TEXT NOT NULL, \
                    checksum TEXT, \
                    execution_ms INTEGER, \
                    success INTEGER NOT NULL DEFAULT 1 CHECK (success IN (0, 1)) \
                 );\
                 INSERT INTO migrations_applied (version, name, applied_at, checksum, execution_ms, success) \
                    VALUES (1, 'V001__test.sql', '2026-01-01T00:00:00Z', 'x', 1, 1); \
                 INSERT INTO migrations_applied (version, name, applied_at, checksum, execution_ms, success) \
                    VALUES (3, 'V003__test.sql', '2026-01-01T00:00:00Z', 'z', 1, 1);",
            )?;
            Ok::<_, crate::storage::pool::DbError>(())
        })
        .await
        .unwrap();

        // Boot path: `apply_pending` must surface the conflict and NOT
        // touch any DDL.
        let migrator = Migrator::new(mig_dir.path());
        let err = migrator.apply_pending(&pool).await.unwrap_err();
        // The MigrationOrderConflict bubbles up through DbError::Manager;
        // the message is the only stable surface for the assertion.
        let msg = format!("{err:?}");
        assert!(
            msg.contains("MigrationOrderConflict") || msg.contains("order"),
            "expected migration-order conflict, got: {msg}"
        );

        // Rollback contract: the table must still hold exactly the two
        // rows we pre-inserted, no row for migration 2 or 4, and table
        // `t2` / `t4` must NOT have been created.
        let post_state: (i64, i64, i64) = pool
            .with_conn(|c| {
                let applied_count: i64 =
                    c.query_row("SELECT COUNT(*) FROM migrations_applied", [], |r| r.get(0))?;
                let t2_exists: i64 = c.query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='t2'",
                    [],
                    |r| r.get(0),
                )?;
                let t4_exists: i64 = c.query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='t4'",
                    [],
                    |r| r.get(0),
                )?;
                Ok::<_, crate::storage::pool::DbError>((applied_count, t2_exists, t4_exists))
            })
            .await
            .unwrap();
        assert_eq!(
            post_state.0, 2,
            "migrations_applied must still hold exactly the two pre-existing rows"
        );
        assert_eq!(
            post_state.1, 0,
            "migration 2's DDL must not have run (rollback contract)"
        );
        assert_eq!(
            post_state.2, 0,
            "migration 4's DDL must not have run (rollback contract)"
        );
    }

    #[tokio::test]
    async fn real_migrations_enforce_branch_message_constraints() {
        use crate::storage::pool::{DatabasePool, PooledConnectionExt};

        let db_dir = tempfile::tempdir().unwrap();
        let db_path = db_dir.path().join("mukei-constraints-test.db");
        let pool = DatabasePool::open(&db_path).unwrap();

        Migrator::embedded().apply_pending(&pool).await.unwrap();

        pool.with_conn(|c| {
            c.execute_batch(
                "PRAGMA foreign_keys = ON;
                 INSERT INTO conversations (id, external_id, title, created_at, updated_at, archived)
                    VALUES (1, 'conv-a', '', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', 0);
                 INSERT INTO conversations (id, external_id, title, created_at, updated_at, archived)
                    VALUES (2, 'conv-b', '', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', 0);
                 INSERT INTO branches (id, external_id, conversation_id, title, created_at, updated_at, is_active)
                    VALUES (10, 'branch-a', 1, '', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', 1);",
            )?;

            let mismatched_branch = c.execute(
                "INSERT INTO messages (
                    id, external_id, conversation_id, role, content, created_at, branch_id
                 ) VALUES (
                    100, 'msg-bad-branch', 2, 'user', 'bad', '2026-01-01T00:00:00Z', 10
                 )",
                [],
            );
            assert!(
                mismatched_branch.is_err(),
                "message branch_id must belong to the message conversation"
            );

            let duplicate_active_branch = c.execute(
                "INSERT INTO branches (
                    id, external_id, conversation_id, title, created_at, updated_at, is_active
                 ) VALUES (
                    11, 'branch-a-2', 1, '', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', 1
                 )",
                [],
            );
            assert!(
                duplicate_active_branch.is_err(),
                "only one active branch is allowed per conversation"
            );

            Ok::<_, crate::storage::pool::DbError>(())
        })
        .await
        .unwrap();
    }
}
