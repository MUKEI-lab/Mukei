//! Durable model-download jobs and quota reservations.
//!
//! Filesystem quota checks alone are racy: two downloads can both inspect
//! the same free space before either writes a byte. This repository adds a
//! SQL-backed reservation ledger so concurrent starts are serialized by an
//! `IMMEDIATE` transaction and collectively respect the model-storage cap.

use std::path::Path;

use crate::error::{MukeiError, Result};

use super::pool::{DatabasePool, DbError, PooledConnectionExt};
use super::quota::DEFAULT_MAX_MODEL_STORAGE_BYTES;

const RESERVATION_TTL_HOURS: i64 = 24;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DownloadJobStatus {
    Queued,
    Downloading,
    Completed,
    Failed,
    Cancelled,
}

impl DownloadJobStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Downloading => "downloading",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DownloadReservation {
    pub job_id: String,
    pub reservation_id: String,
    pub reserved_bytes: u64,
}


#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct DownloadJobRecord {
    pub job_id: String,
    pub model_id: Option<String>,
    pub destination_token: String,
    pub expected_bytes: u64,
    pub bytes_downloaded: u64,
    pub status: String,
    pub last_error_code: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

pub struct DownloadJobRepository;

impl DownloadJobRepository {
    /// Atomically create a job and reserve its expected final size.
    /// `accounted_storage_bytes` is the current filesystem quota usage
    /// excluding only this job's own resumable prefix. Other partials stay
    /// visible, while this job's full final size is represented by the new
    /// reservation.
    pub async fn reserve(
        pool: &DatabasePool,
        model_id: Option<String>,
        destination_token: String,
        destination_path: &Path,
        expected_sha256: String,
        expected_bytes: u64,
        accounted_storage_bytes: u64,
    ) -> Result<DownloadReservation> {
        let job_id = uuid::Uuid::new_v4().to_string();
        let reservation_id = uuid::Uuid::new_v4().to_string();
        let destination_path = destination_path.to_string_lossy().into_owned();
        let job_id_for_db = job_id.clone();
        let reservation_id_for_db = reservation_id.clone();

        pool.with_conn(move |connection| {
            let tx = connection
                .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            let now = chrono::Utc::now();
            let now_string = now.to_rfc3339();
            let expires_at = (now + chrono::Duration::hours(RESERVATION_TTL_HOURS)).to_rfc3339();

            // A process restart cannot keep a download task alive. Reclaim
            // expired leases before calculating the aggregate reservation.
            tx.execute(
                "DELETE FROM storage_reservations WHERE expires_at <= ?1",
                [&now_string],
            )?;

            let reserved: i64 = tx.query_row(
                "SELECT COALESCE(SUM(reserved_bytes), 0) FROM storage_reservations",
                [],
                |row| row.get(0),
            )?;
            let reserved = u64::try_from(reserved.max(0)).unwrap_or(u64::MAX);
            let projected = accounted_storage_bytes
                .saturating_add(reserved)
                .saturating_add(expected_bytes);
            if projected > DEFAULT_MAX_MODEL_STORAGE_BYTES {
                return Err(DbError::Domain(MukeiError::StorageQuotaExceeded {
                    max_bytes: DEFAULT_MAX_MODEL_STORAGE_BYTES,
                    requested_bytes: expected_bytes,
                    used_bytes: accounted_storage_bytes.saturating_add(reserved),
                }));
            }

            tx.execute(
                "INSERT INTO download_jobs (\
                    job_id, model_id, destination_token, destination_path, expected_sha256, \
                    expected_bytes, bytes_downloaded, status, last_error_code, created_at, updated_at\
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, 'queued', NULL, ?7, ?7)",
                rusqlite::params![
                    &job_id_for_db,
                    &model_id,
                    &destination_token,
                    &destination_path,
                    &expected_sha256,
                    i64::try_from(expected_bytes).unwrap_or(i64::MAX),
                    &now_string,
                ],
            )?;
            tx.execute(
                "INSERT INTO storage_reservations (\
                    reservation_id, job_id, storage_class, reserved_bytes, created_at, expires_at\
                 ) VALUES (?1, ?2, 'model', ?3, ?4, ?5)",
                rusqlite::params![
                    &reservation_id_for_db,
                    &job_id_for_db,
                    i64::try_from(expected_bytes).unwrap_or(i64::MAX),
                    &now_string,
                    &expires_at,
                ],
            )?;
            tx.commit()?;
            Ok::<_, DbError>(())
        })
        .await?;

        Ok(DownloadReservation {
            job_id,
            reservation_id,
            reserved_bytes: expected_bytes,
        })
    }

    /// Reconcile the catalog reservation against the server-reported total
    /// before response bytes are accepted. The resize and quota check happen
    /// in one IMMEDIATE transaction, preventing parallel jobs from jointly
    /// expanding beyond the app model-storage budget.
    pub async fn reconcile_started(
        pool: &DatabasePool,
        reservation: &DownloadReservation,
        total_bytes: u64,
        accounted_storage_bytes: u64,
    ) -> Result<DownloadReservation> {
        let job_id = reservation.job_id.clone();
        let reservation_id = reservation.reservation_id.clone();
        let now = chrono::Utc::now();
        let now_string = now.to_rfc3339();
        let expires_at = (now + chrono::Duration::hours(RESERVATION_TTL_HOURS)).to_rfc3339();
        pool.with_conn(move |connection| {
            let tx = connection
                .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            tx.execute(
                "DELETE FROM storage_reservations WHERE expires_at <= ?1 AND job_id <> ?2",
                rusqlite::params![&now_string, &job_id],
            )?;
            let aggregate: i64 = tx.query_row(
                "SELECT COALESCE(SUM(reserved_bytes), 0) FROM storage_reservations",
                [],
                |row| row.get(0),
            )?;
            let current: i64 = tx.query_row(
                "SELECT reserved_bytes FROM storage_reservations                  WHERE reservation_id = ?1 AND job_id = ?2",
                rusqlite::params![&reservation_id, &job_id],
                |row| row.get(0),
            )?;
            let aggregate = u64::try_from(aggregate.max(0)).unwrap_or(u64::MAX);
            let current = u64::try_from(current.max(0)).unwrap_or(u64::MAX);
            let other_reservations = aggregate.saturating_sub(current);
            let used = accounted_storage_bytes.saturating_add(other_reservations);
            let projected = used.saturating_add(total_bytes);
            if projected > DEFAULT_MAX_MODEL_STORAGE_BYTES {
                return Err(DbError::Domain(MukeiError::StorageQuotaExceeded {
                    max_bytes: DEFAULT_MAX_MODEL_STORAGE_BYTES,
                    requested_bytes: total_bytes,
                    used_bytes: used,
                }));
            }
            tx.execute(
                "UPDATE storage_reservations SET reserved_bytes = ?3, expires_at = ?4                  WHERE reservation_id = ?1 AND job_id = ?2",
                rusqlite::params![
                    &reservation_id,
                    &job_id,
                    i64::try_from(total_bytes).unwrap_or(i64::MAX),
                    &expires_at,
                ],
            )?;
            tx.execute(
                "UPDATE download_jobs SET status = 'downloading', expected_bytes = ?2,                     updated_at = ?3 WHERE job_id = ?1",
                rusqlite::params![
                    &job_id,
                    i64::try_from(total_bytes).unwrap_or(i64::MAX),
                    &now_string,
                ],
            )?;
            tx.commit()?;
            Ok::<_, DbError>(DownloadReservation {
                job_id,
                reservation_id,
                reserved_bytes: total_bytes,
            })
        })
        .await
    }

    pub async fn mark_started(pool: &DatabasePool, job_id: &str, total_bytes: u64) -> Result<()> {
        let job_id = job_id.to_string();
        pool.with_conn(move |connection| {
            connection.execute(
                "UPDATE download_jobs SET status = 'downloading', expected_bytes = ?2, \
                    updated_at = ?3 WHERE job_id = ?1",
                rusqlite::params![
                    job_id,
                    i64::try_from(total_bytes).unwrap_or(i64::MAX),
                    chrono::Utc::now().to_rfc3339(),
                ],
            )?;
            Ok::<_, DbError>(())
        })
        .await
    }

    pub async fn update_progress(pool: &DatabasePool, job_id: &str, bytes: u64) -> Result<()> {
        let job_id = job_id.to_string();
        pool.with_conn(move |connection| {
            connection.execute(
                "UPDATE download_jobs SET bytes_downloaded = ?2, updated_at = ?3 \
                 WHERE job_id = ?1 AND status IN ('queued', 'downloading')",
                rusqlite::params![
                    job_id,
                    i64::try_from(bytes).unwrap_or(i64::MAX),
                    chrono::Utc::now().to_rfc3339(),
                ],
            )?;
            Ok::<_, DbError>(())
        })
        .await
    }

    /// Set the terminal job state and release its quota reservation in one
    /// transaction. This is safe to call more than once.
    pub async fn finish(
        pool: &DatabasePool,
        job_id: &str,
        status: DownloadJobStatus,
        last_error_code: Option<String>,
    ) -> Result<()> {
        debug_assert!(matches!(
            status,
            DownloadJobStatus::Completed | DownloadJobStatus::Failed | DownloadJobStatus::Cancelled
        ));
        let job_id = job_id.to_string();
        pool.with_conn(move |connection| {
            let tx = connection.transaction()?;
            tx.execute(
                "UPDATE download_jobs SET status = ?2, last_error_code = ?3, updated_at = ?4 \
                 WHERE job_id = ?1",
                rusqlite::params![
                    &job_id,
                    status.as_str(),
                    &last_error_code,
                    chrono::Utc::now().to_rfc3339(),
                ],
            )?;
            tx.execute(
                "DELETE FROM storage_reservations WHERE job_id = ?1",
                [&job_id],
            )?;
            tx.commit()?;
            Ok::<_, DbError>(())
        })
        .await
    }

    /// Return recent durable jobs for UI projection. Paths are intentionally
    /// omitted; QML receives only the opaque destination token.
    pub async fn list_recent(pool: &DatabasePool, limit: usize) -> Result<Vec<DownloadJobRecord>> {
        let limit = i64::try_from(limit.clamp(1, 200)).unwrap_or(200);
        pool.with_conn(move |connection| {
            let mut stmt = connection.prepare(
                "SELECT job_id, model_id, destination_token, expected_bytes, \
                        bytes_downloaded, status, last_error_code, created_at, updated_at \
                 FROM download_jobs ORDER BY updated_at DESC LIMIT ?1",
            )?;
            let rows = stmt
                .query_map([limit], |row| {
                    let expected: Option<i64> = row.get(3)?;
                    let downloaded: i64 = row.get(4)?;
                    Ok(DownloadJobRecord {
                        job_id: row.get(0)?,
                        model_id: row.get(1)?,
                        destination_token: row.get(2)?,
                        expected_bytes: u64::try_from(expected.unwrap_or(0).max(0)).unwrap_or(u64::MAX),
                        bytes_downloaded: u64::try_from(downloaded.max(0)).unwrap_or(u64::MAX),
                        status: row.get(5)?,
                        last_error_code: row.get(6)?,
                        created_at: row.get(7)?,
                        updated_at: row.get(8)?,
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok::<_, DbError>(rows)
        })
        .await
    }

    /// Boot recovery: no queued/downloading task survives a process death.
    /// Mark them failed and release every stale reservation before new jobs
    /// are accepted. Existing `.partial` files remain visible to the normal
    /// filesystem quota guard and resumable downloader.
    pub async fn recover_interrupted(pool: &DatabasePool) -> Result<usize> {
        pool.with_conn(|connection| {
            let tx = connection.transaction()?;
            let changed = tx.execute(
                "UPDATE download_jobs SET status = 'failed', \
                    last_error_code = 'ERR_DOWNLOAD_INTERRUPTED', updated_at = ?1 \
                 WHERE status IN ('queued', 'downloading')",
                [chrono::Utc::now().to_rfc3339()],
            )?;
            tx.execute("DELETE FROM storage_reservations", [])?;
            tx.commit()?;
            Ok::<_, DbError>(changed)
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn pool() -> DatabasePool {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("downloads.db");
        let pool = DatabasePool::open(&path).unwrap();
        crate::storage::Migrator::embedded()
            .apply_pending(&pool)
            .await
            .unwrap();
        std::mem::forget(dir);
        pool
    }

    #[tokio::test]
    async fn server_total_resizes_reservation_atomically() {
        let pool = pool().await;
        let reservation = DownloadJobRepository::reserve(
            &pool,
            Some("model-resize".into()),
            "model:model-resize".into(),
            Path::new("/private/models/resize.gguf"),
            "b".repeat(64),
            1024,
            0,
        )
        .await
        .unwrap();
        let resized = DownloadJobRepository::reconcile_started(&pool, &reservation, 2048, 0)
            .await
            .unwrap();
        assert_eq!(resized.reserved_bytes, 2048);
        let stored: i64 = pool
            .with_conn(move |connection| {
                Ok::<_, DbError>(connection.query_row(
                    "SELECT reserved_bytes FROM storage_reservations WHERE job_id = ?1",
                    [&resized.job_id],
                    |row| row.get(0),
                )?)
            })
            .await
            .unwrap();
        assert_eq!(stored, 2048);
    }

    #[tokio::test]
    async fn recent_job_projection_omits_private_destination_path() {
        let pool = pool().await;
        let reservation = DownloadJobRepository::reserve(
            &pool,
            Some("model-private".into()),
            "model:model-private".into(),
            Path::new("/private/app/models/private.gguf"),
            "c".repeat(64),
            1024,
            0,
        )
        .await
        .unwrap();

        let rows = DownloadJobRepository::list_recent(&pool, 10).await.unwrap();
        let row = rows
            .iter()
            .find(|row| row.job_id == reservation.job_id)
            .unwrap();
        assert_eq!(row.destination_token, "model:model-private");
        let json = serde_json::to_string(row).unwrap();
        assert!(!json.contains("/private/app/models"));
    }

    #[tokio::test]
    async fn reservations_are_released_with_terminal_job() {
        let pool = pool().await;
        let reservation = DownloadJobRepository::reserve(
            &pool,
            Some("model-a".into()),
            "model:model-a".into(),
            Path::new("/private/models/a.gguf"),
            "a".repeat(64),
            1024,
            0,
        )
        .await
        .unwrap();
        DownloadJobRepository::finish(
            &pool,
            &reservation.job_id,
            DownloadJobStatus::Completed,
            None,
        )
        .await
        .unwrap();

        let remaining: i64 = pool
            .with_conn(|connection| {
                Ok::<_, DbError>(connection.query_row(
                    "SELECT COUNT(*) FROM storage_reservations",
                    [],
                    |row| row.get(0),
                )?)
            })
            .await
            .unwrap();
        assert_eq!(remaining, 0);
    }
}
