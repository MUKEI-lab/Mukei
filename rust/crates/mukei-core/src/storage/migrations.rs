//! `mukei_core::storage::migrations` — TRD §6.1.
//!
//! Strict append-only migration framework. The boot path runs
//! `Migrator::apply_pending` after opening the SQLCipher database.
//! Each `V###__name.sql` adds rows to `migrations_applied` and bumps
//! `PRAGMA user_version`. The order is *strictly* monotonic; conflict
//! produces `MukeiError::MigrationOrderConflict`.

use std::path::{Path, PathBuf};

use crate::error::{MukeiError, Result};

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

/// Pure-data migrator — does *not* open a SQLite handle. The core
/// migrator owns the schema; the bridge crate opens the real
/// connection and invokes `apply_one` on each row.
pub struct Migrator {
    dir: PathBuf,
}

impl Migrator {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Read all migration files in the directory and return them in
    /// strict ascending order of `V{n}`.
    pub fn list_available(&self) -> Result<Vec<(u32, String, String)>> {
        let mut out = Vec::new();
        let entries = std::fs::read_dir(&self.dir).map_err(|e| MukeiError::Io(e.to_string()))?;
        let mut sorted: Vec<_> = entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                p.extension().and_then(|e| e.to_str()) == Some("sql")
                    && p.file_name()
                        .and_then(|n| n.to_str())
                        .map(|n| n.starts_with(MIGRATION_FILE_PREFIX))
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

        // Discover all migrations on disk + their checksums.
        let mut bundle = Vec::new();
        for (id, name, body) in self.list_available()? {
            use sha2::{Digest, Sha256};
            let mut h = Sha256::new();
            h.update(body.as_bytes());
            let checksum = crate::diagnostics::crash_logger::hex_helper(&h.finalize());
            bundle.push((id, name, body, checksum));
        }

        pool.with_conn(move |c| {
            // Bootstrap the migrations_applied table itself if absent.
            c.execute_batch(
                "CREATE TABLE IF NOT EXISTS migrations_applied ( \
                    version INTEGER PRIMARY KEY, \
                    name TEXT NOT NULL UNIQUE, \
                    applied_at TEXT NOT NULL, \
                    checksum TEXT, \
                    execution_ms INTEGER, \
                    success INTEGER NOT NULL DEFAULT 1 CHECK (success IN (0, 1)) \
                 )",
            )?;

            // Read the already-applied set.
            let mut stmt = c.prepare("SELECT version, name, applied_at, checksum FROM migrations_applied ORDER BY version")?;
            let applied: Vec<MigrationRecord> = stmt
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

            // Determine pending.
            let available_brief: Vec<(u32, String, String)> = bundle
                .iter()
                .map(|(id, name, body, _)| (*id, name.clone(), body.clone()))
                .collect();
            // Order check first — surface conflict BEFORE running any DDL.
            Self::verify_order(&available_brief, &applied)
                .map_err(|e| super::pool::DbError::Manager(e.to_string()))?;
            let pending_ids: Vec<u32> = Self::pending(&available_brief, &applied)
                .iter()
                .map(|(id, _, _)| *id)
                .collect();

            // Apply each pending migration inside its own SQL transaction.
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
                    "INSERT INTO migrations_applied (version, name, applied_at, checksum, execution_ms, success) \
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

            Ok::<_, super::pool::DbError>(applied_now)
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
}
