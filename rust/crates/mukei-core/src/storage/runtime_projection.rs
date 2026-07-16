//! Durable JSON projections used by the Android application runtime.
//!
//! Domain objects remain typed inside `application_runtime`; this repository
//! only persists their serialized authoritative projections in SQLCipher.

use crate::error::{MukeiError, Result};
use crate::storage::pool::{DatabasePool, DbError, PooledConnectionExt};
use rusqlite::OptionalExtension;

const MIGRATION_VERSION: u32 = 14;
const MIGRATION_NAME: &str = "V014__runtime_projections";
const MIGRATION_BODY: &str =
    include_str!("../../../../migrations/V014__runtime_projections.sql");

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeProjectionRow {
    pub domain: String,
    pub projection_key: String,
    pub payload_json: String,
    pub updated_at: String,
}

pub struct RuntimeProjectionRepository;

impl RuntimeProjectionRepository {
    /// Append the projection schema to the encrypted migration ledger.
    ///
    /// This is kept beside the repository so mobile builds can add the Android
    /// projection table without depending on a source-tree migration directory.
    pub async fn ensure_schema(pool: &DatabasePool) -> Result<()> {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(MIGRATION_BODY.as_bytes());
        let bundled = crate::diagnostics::crash_logger::hex_helper(&hasher.finalize());
        pool.with_conn(move |connection| {
            let tx = connection.transaction()?;
            let applied: Option<String> = tx
                .query_row(
                    "SELECT checksum FROM migrations_applied WHERE version = ?1",
                    [MIGRATION_VERSION as i64],
                    |row| row.get::<_, Option<String>>(0),
                )
                .optional()?
                .flatten();
            if let Some(applied) = applied {
                if applied != bundled {
                    return Err(DbError::Domain(MukeiError::MigrationChecksumMismatch {
                        version: MIGRATION_VERSION,
                        applied,
                        bundled,
                    }));
                }
                tx.commit()?;
                return Ok::<_, DbError>(());
            }

            tx.execute_batch(MIGRATION_BODY).map_err(|error| {
                DbError::Domain(MukeiError::MigrationFailed(
                    MIGRATION_VERSION,
                    error.to_string(),
                ))
            })?;
            tx.execute(
                "INSERT INTO migrations_applied \
                    (version, name, applied_at, checksum, execution_ms, success) \
                 VALUES (?1, ?2, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), ?3, 0, 1)",
                rusqlite::params![MIGRATION_VERSION as i64, MIGRATION_NAME, bundled],
            )?;
            tx.commit()?;
            Ok::<_, DbError>(())
        })
        .await?;
        Ok(())
    }

    pub async fn upsert(
        pool: &DatabasePool,
        domain: impl Into<String>,
        projection_key: impl Into<String>,
        payload_json: impl Into<String>,
    ) -> Result<()> {
        let domain = domain.into();
        let projection_key = projection_key.into();
        let payload_json = payload_json.into();
        validate_identity(&domain, &projection_key)?;
        pool.with_conn(move |connection| {
            connection.execute(
                "INSERT INTO runtime_projections \
                    (domain, projection_key, payload_json, updated_at) \
                 VALUES (?1, ?2, ?3, strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) \
                 ON CONFLICT(domain, projection_key) DO UPDATE SET \
                    payload_json = excluded.payload_json, \
                    updated_at = excluded.updated_at",
                rusqlite::params![domain, projection_key, payload_json],
            )?;
            Ok::<_, DbError>(())
        })
        .await?;
        Ok(())
    }

    pub async fn delete(
        pool: &DatabasePool,
        domain: impl Into<String>,
        projection_key: impl Into<String>,
    ) -> Result<()> {
        let domain = domain.into();
        let projection_key = projection_key.into();
        validate_identity(&domain, &projection_key)?;
        pool.with_conn(move |connection| {
            connection.execute(
                "DELETE FROM runtime_projections WHERE domain = ?1 AND projection_key = ?2",
                rusqlite::params![domain, projection_key],
            )?;
            Ok::<_, DbError>(())
        })
        .await?;
        Ok(())
    }

    pub async fn list_domain(
        pool: &DatabasePool,
        domain: impl Into<String>,
    ) -> Result<Vec<RuntimeProjectionRow>> {
        let domain = domain.into();
        validate_identity(&domain, "list")?;
        pool.with_conn(move |connection| {
            let mut statement = connection.prepare(
                "SELECT domain, projection_key, payload_json, updated_at \
                 FROM runtime_projections WHERE domain = ?1 \
                 ORDER BY projection_key",
            )?;
            let rows = statement
                .query_map([domain], |row| {
                    Ok(RuntimeProjectionRow {
                        domain: row.get(0)?,
                        projection_key: row.get(1)?,
                        payload_json: row.get(2)?,
                        updated_at: row.get(3)?,
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok::<_, DbError>(rows)
        })
        .await
    }
}

fn validate_identity(domain: &str, key: &str) -> Result<()> {
    let valid = |value: &str| {
        !value.trim().is_empty()
            && value.len() <= 256
            && value == value.trim()
            && value.chars().all(|character| {
                character.is_ascii_alphanumeric()
                    || matches!(character, '-' | '_' | '.' | ':' | '/')
            })
    };
    if valid(domain) && valid(key) {
        Ok(())
    } else {
        Err(MukeiError::ConfigInvalid {
            field: "runtime_projection_identity".into(),
            reason: "domain and key must be bounded stable identifiers".into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Migrator;

    #[tokio::test]
    async fn projection_round_trip_is_durable() {
        let directory = tempfile::tempdir().unwrap();
        let pool = DatabasePool::open(directory.path().join("projection.db")).unwrap();
        Migrator::embedded().apply_pending(&pool).await.unwrap();
        RuntimeProjectionRepository::ensure_schema(&pool)
            .await
            .unwrap();
        RuntimeProjectionRepository::upsert(&pool, "setting", "theme_mode", "\"taupe\"")
            .await
            .unwrap();
        let rows = RuntimeProjectionRepository::list_domain(&pool, "setting")
            .await
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].projection_key, "theme_mode");
        RuntimeProjectionRepository::delete(&pool, "setting", "theme_mode")
            .await
            .unwrap();
        assert!(RuntimeProjectionRepository::list_domain(&pool, "setting")
            .await
            .unwrap()
            .is_empty());
    }
}
