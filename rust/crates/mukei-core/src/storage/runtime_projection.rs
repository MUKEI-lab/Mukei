//! Durable JSON projections used by the Android application runtime.
//!
//! Domain objects remain typed inside `application_runtime`; this repository
//! only persists their serialized authoritative projections in SQLCipher.

use crate::error::Result;
use crate::storage::pool::{DatabasePool, DbError, PooledConnectionExt};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeProjectionRow {
    pub domain: String,
    pub projection_key: String,
    pub payload_json: String,
    pub updated_at: String,
}

pub struct RuntimeProjectionRepository;

impl RuntimeProjectionRepository {
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
            && value
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.' | ':' | '/'))
    };
    if valid(domain) && valid(key) {
        Ok(())
    } else {
        Err(crate::error::MukeiError::ConfigInvalid {
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
