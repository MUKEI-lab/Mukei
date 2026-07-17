//! Atomic publication of encrypted imported files into Universal Storage or a chat workspace.
//!
//! The object-store publication happens before this repository is called. This layer records a
//! durable `applied_filesystem` journal entry, then commits the verified object metadata, initial
//! immutable version, logical file node, and import-state transition in one IMMEDIATE transaction.
//! A retry after process death is idempotent and returns the already-committed node.

include!("import_commit_repository/types.rs");
include!("import_commit_repository/commit.rs");
include!("import_commit_repository/recovery.rs");
include!("import_commit_repository/naming.rs");
