//! Crash-safe import transaction and operation-journal repository.
//!
//! Imports are resumable state machines. Every state transition is validated
//! before it is persisted, and incomplete work is discoverable through a
//! deterministic recovery queue after process death or power loss.

use crate::error::{MukeiError, Result};
use crate::storage::pool::{DatabasePool, DbError, PooledConnectionExt};
use crate::storage::universal::{ImportTransactionId, StorageNodeId, StorageScopeId};
use rusqlite::{OptionalExtension, TransactionBehavior};
use uuid::Uuid;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ImportState {
    Created,
    Validating,
    Copying,
    Hashing,
    Encrypting,
    CommittingObject,
    CommittingNode,
    Indexing,
    Completed,
    CancelRequested,
    Cancelled,
    Failed,
    Recovering,
}

impl ImportState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Validating => "validating",
            Self::Copying => "copying",
            Self::Hashing => "hashing",
            Self::Encrypting => "encrypting",
            Self::CommittingObject => "committing_object",
            Self::CommittingNode => "committing_node",
            Self::Indexing => "indexing",
            Self::Completed => "completed",
            Self::CancelRequested => "cancel_requested",
            Self::Cancelled => "cancelled",
            Self::Failed => "failed",
            Self::Recovering => "recovering",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "created" => Ok(Self::Created),
            "validating" => Ok(Self::Validating),
            "copying" => Ok(Self::Copying),
            "hashing" => Ok(Self::Hashing),
            "encrypting" => Ok(Self::Encrypting),
            "committing_object" => Ok(Self::CommittingObject),
            "committing_node" => Ok(Self::CommittingNode),
            "indexing" => Ok(Self::Indexing),
            "completed" => Ok(Self::Completed),
            "cancel_requested" => Ok(Self::CancelRequested),
            "cancelled" => Ok(Self::Cancelled),
            "failed" => Ok(Self::Failed),
            "recovering" => Ok(Self::Recovering),
            other => Err(MukeiError::Invariant(format!(
                "unknown import transaction state: {other}"
            ))),
        }
    }

    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Cancelled | Self::Failed)
    }

    pub const fn can_transition_to(self, next: Self) -> bool {
        use ImportState::*;
        matches!(
            (self, next),
            (Created, Validating)
                | (Created, CancelRequested)
                | (Created, Failed)
                | (Validating, Copying)
                | (Validating, CancelRequested)
                | (Validating, Failed)
                | (Copying, Hashing)
                | (Copying, CancelRequested)
                | (Copying, Failed)
                | (Hashing, Encrypting)
                | (Hashing, CancelRequested)
                | (Hashing, Failed)
                | (Encrypting, CommittingObject)
                | (Encrypting, CancelRequested)
                | (Encrypting, Failed)
                | (CommittingObject, CommittingNode)
                | (CommittingObject, Recovering)
                | (CommittingObject, Failed)
                | (CommittingNode, Indexing)
                | (CommittingNode, Recovering)
                | (CommittingNode, Failed)
                | (Indexing, Completed)
                | (Indexing, CancelRequested)
                | (Indexing, Failed)
                | (CancelRequested, Cancelled)
                | (CancelRequested, Recovering)
                | (Recovering, Validating)
                | (Recovering, Copying)
                | (Recovering, Hashing)
                | (Recovering, Encrypting)
                | (Recovering, CommittingObject)
                | (Recovering, CommittingNode)
                | (Recovering, Indexing)
                | (Recovering, Cancelled)
                | (Recovering, Failed)
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportTransactionRecord {
    pub transaction_id: ImportTransactionId,
    pub target_scope_id: StorageScopeId,
    pub target_parent_node_id: StorageNodeId,
    pub original_filename: String,
    pub staging_relative_path: String,
    pub expected_size: Option<u64>,
    pub bytes_written: u64,
    pub state: ImportState,
    pub error_code: Option<String>,
    pub error_details: Option<String>,
    pub updated_at: String,
}

pub struct ImportJournalRepository;

impl ImportJournalRepository {
    pub async fn create(
        pool: &DatabasePool,
        target_scope_id: StorageScopeId,
        target_parent_node_id: StorageNodeId,
        original_filename: String,
        staging_relative_path: String,
        expected_size: Option<u64>,
        source_uri_fingerprint: Option<String>,
    ) -> Result<ImportTransactionId> {
        let transaction_id = ImportTransactionId::new();
        let transaction_id_for_db = transaction_id;
        pool.with_conn(move |connection| {
            let now = chrono::Utc::now().to_rfc3339();
            connection.execute(
                "INSERT INTO import_transactions (\
                    transaction_id, target_scope_id, target_parent_node_id, source_uri_fingerprint, \
                    original_filename, staging_relative_path, expected_size, bytes_written, state, \
                    created_at, updated_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, 'created', ?8, ?8)",
                rusqlite::params![
                    transaction_id_for_db.to_string(),
                    target_scope_id.to_string(),
                    target_parent_node_id.to_string(),
                    source_uri_fingerprint,
                    original_filename,
                    staging_relative_path,
                    expected_size.map(|value| value as i64),
                    now,
                ],
            )?;
            Ok::<_, DbError>(())
        })
        .await?;
        Ok(transaction_id)
    }

    pub async fn record_progress(
        pool: &DatabasePool,
        transaction_id: ImportTransactionId,
        bytes_written: u64,
    ) -> Result<()> {
        pool.with_conn(move |connection| {
            let changed = connection.execute(
                "UPDATE import_transactions \
                 SET bytes_written = ?2, updated_at = ?3 \
                 WHERE transaction_id = ?1 \
                   AND state IN ('copying', 'hashing', 'encrypting', 'recovering') \
                   AND (expected_size IS NULL OR ?2 <= expected_size)",
                rusqlite::params![
                    transaction_id.to_string(),
                    bytes_written as i64,
                    chrono::Utc::now().to_rfc3339(),
                ],
            )?;
            if changed != 1 {
                return Err(DbError::Domain(MukeiError::Invariant(format!(
                    "progress update rejected for import transaction {transaction_id}"
                ))));
            }
            Ok(())
        })
        .await
    }

    pub async fn transition(
        pool: &DatabasePool,
        transaction_id: ImportTransactionId,
        next: ImportState,
        error_code: Option<String>,
        error_details: Option<String>,
    ) -> Result<()> {
        pool.with_conn(move |connection| {
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let current: Option<String> = transaction
                .query_row(
                    "SELECT state FROM import_transactions WHERE transaction_id = ?1",
                    [transaction_id.to_string()],
                    |row| row.get(0),
                )
                .optional()?;
            let current = current.ok_or_else(|| {
                DbError::Domain(MukeiError::Invariant(format!(
                    "missing import transaction {transaction_id}"
                )))
            })?;
            let current = ImportState::parse(&current).map_err(DbError::Domain)?;
            if !current.can_transition_to(next) {
                return Err(DbError::Domain(MukeiError::Invariant(format!(
                    "illegal import transition {} -> {} for {transaction_id}",
                    current.as_str(),
                    next.as_str()
                ))));
            }

            let now = chrono::Utc::now().to_rfc3339();
            transaction.execute(
                "UPDATE import_transactions \
                 SET state = ?2, error_code = ?3, error_details = ?4, updated_at = ?5, \
                     completed_at = CASE WHEN ?2 = 'completed' THEN ?5 ELSE completed_at END \
                 WHERE transaction_id = ?1",
                rusqlite::params![
                    transaction_id.to_string(),
                    next.as_str(),
                    error_code,
                    error_details,
                    now,
                ],
            )?;
            transaction.commit()?;
            Ok::<_, DbError>(())
        })
        .await
    }

    pub async fn recovery_queue(pool: &DatabasePool) -> Result<Vec<ImportTransactionRecord>> {
        pool.with_conn(|connection| {
            let mut statement = connection.prepare(
                "SELECT transaction_id, target_scope_id, target_parent_node_id, original_filename, \
                        staging_relative_path, expected_size, bytes_written, state, error_code, \
                        error_details, updated_at \
                 FROM import_transactions \
                 WHERE state NOT IN ('completed', 'cancelled', 'failed') \
                 ORDER BY updated_at ASC, transaction_id ASC",
            )?;
            let rows = statement
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, Option<i64>>(5)?,
                        row.get::<_, i64>(6)?,
                        row.get::<_, String>(7)?,
                        row.get::<_, Option<String>>(8)?,
                        row.get::<_, Option<String>>(9)?,
                        row.get::<_, String>(10)?,
                    ))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;

            rows.into_iter()
                .map(|row| {
                    Ok(ImportTransactionRecord {
                        transaction_id: ImportTransactionId(parse_uuid(&row.0)?),
                        target_scope_id: StorageScopeId(parse_uuid(&row.1)?),
                        target_parent_node_id: StorageNodeId(parse_uuid(&row.2)?),
                        original_filename: row.3,
                        staging_relative_path: row.4,
                        expected_size: row.5.map(|value| value.max(0) as u64),
                        bytes_written: row.6.max(0) as u64,
                        state: ImportState::parse(&row.7).map_err(DbError::Domain)?,
                        error_code: row.8,
                        error_details: row.9,
                        updated_at: row.10,
                    })
                })
                .collect::<std::result::Result<Vec<_>, DbError>>()
        })
        .await
    }

    pub async fn mark_recovery_required(pool: &DatabasePool) -> Result<usize> {
        pool.with_conn(|connection| {
            let changed = connection.execute(
                "UPDATE import_transactions \
                 SET state = 'recovering', updated_at = ?1 \
                 WHERE state NOT IN ('completed', 'cancelled', 'failed', 'recovering')",
                [chrono::Utc::now().to_rfc3339()],
            )?;
            Ok::<_, DbError>(changed)
        })
        .await
    }
}

fn parse_uuid(value: &str) -> std::result::Result<Uuid, DbError> {
    Uuid::parse_str(value).map_err(|error| {
        DbError::Domain(MukeiError::Invariant(format!(
            "invalid storage UUID {value}: {error}"
        )))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forward_import_path_is_accepted() {
        let path = [
            ImportState::Created,
            ImportState::Validating,
            ImportState::Copying,
            ImportState::Hashing,
            ImportState::Encrypting,
            ImportState::CommittingObject,
            ImportState::CommittingNode,
            ImportState::Indexing,
            ImportState::Completed,
        ];
        for pair in path.windows(2) {
            assert!(pair[0].can_transition_to(pair[1]));
        }
    }

    #[test]
    fn terminal_states_cannot_restart() {
        for state in [
            ImportState::Completed,
            ImportState::Cancelled,
            ImportState::Failed,
        ] {
            assert!(state.is_terminal());
            assert!(!state.can_transition_to(ImportState::Created));
            assert!(!state.can_transition_to(ImportState::Recovering));
        }
    }

    #[test]
    fn recovery_can_resume_only_at_safe_phases() {
        assert!(ImportState::Recovering.can_transition_to(ImportState::Hashing));
        assert!(ImportState::Recovering.can_transition_to(ImportState::CommittingNode));
        assert!(!ImportState::Recovering.can_transition_to(ImportState::Completed));
    }
}
