#![cfg(feature = "rusqlite")]

//! Verifies that V015 remains immutable and existing V015 databases advance
//! through the normal migration engine by applying V016 only.

use mukei_core::storage::{migrations::Migrator, DatabasePool, DbError, PooledConnectionExt};

const V015: &str = include_str!("../../../migrations/V015__workspace_scope_isolation_guards.sql");
const V016: &str = include_str!("../../../migrations/V016__storage_identity_and_recovery_hardening.sql");

#[test]
fn canonical_v015_is_frozen_and_hardening_lives_only_in_v016() {
    assert!(V015.contains("storage_nodes_parent_same_scope_insert"));
    assert!(V015.contains("import_target_same_scope_insert"));
    assert!(!V015.contains("storage_node_identity_immutable"));
    assert!(!V015.contains("operation_journal_terminal_evidence_immutable"));

    assert!(V016.contains("storage_node_identity_immutable"));
    assert!(V016.contains("import_terminal_state_immutable"));
    assert!(V016.contains("operation_journal_terminal_evidence_immutable"));
    assert!(V016.contains("PRAGMA user_version = 16"));
}

#[tokio::test]
async fn canonical_v015_database_applies_only_v016_on_next_boot() {
    let bundled = Migrator::embedded()
        .list_available()
        .expect("embedded migration bundle");
    assert_eq!(bundled.last().map(|entry| entry.0), Some(16));

    let v15_dir = tempfile::tempdir().expect("temporary V015 migration directory");
    for (version, name, body) in bundled.iter().filter(|(version, _, _)| *version <= 15) {
        let filename = format!("V{version:03}__{}.sql", name.trim_start_matches(&format!("V{version:03}__")));
        std::fs::write(v15_dir.path().join(filename), body).expect("write canonical migration");
    }

    let database_dir = tempfile::tempdir().expect("temporary database directory");
    let database_path = database_dir.path().join("v15-upgrade.db");
    let pool = DatabasePool::open(&database_path).expect("open database pool");

    let applied_through_v15 = Migrator::new(v15_dir.path())
        .apply_pending(&pool)
        .await
        .expect("apply canonical migrations through V015");
    assert_eq!(applied_through_v15.last().map(|record| record.id), Some(15));

    pool.with_conn(|connection| {
        let max_version: i64 = connection.query_row(
            "SELECT MAX(version) FROM migrations_applied",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(max_version, 15);

        let hardening_trigger_count: i64 = connection.query_row(
            "SELECT COUNT(*) FROM sqlite_master
             WHERE type = 'trigger' AND name = 'storage_node_identity_immutable'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(hardening_trigger_count, 0);
        Ok::<_, DbError>(())
    })
    .await
    .unwrap();

    let upgraded = Migrator::embedded()
        .apply_pending(&pool)
        .await
        .expect("upgrade V015 database through embedded V016");
    assert_eq!(upgraded.len(), 1, "only V016 should be pending");
    assert_eq!(upgraded[0].id, 16);
    assert_eq!(
        upgraded[0].name,
        "V016__storage_identity_and_recovery_hardening"
    );

    pool.with_conn(|connection| {
        let max_version: i64 = connection.query_row(
            "SELECT MAX(version) FROM migrations_applied",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(max_version, 16);

        let hardening_trigger_count: i64 = connection.query_row(
            "SELECT COUNT(*) FROM sqlite_master
             WHERE type = 'trigger' AND name = 'storage_node_identity_immutable'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(hardening_trigger_count, 1);

        let user_version: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        assert_eq!(user_version, 16);
        Ok::<_, DbError>(())
    })
    .await
    .unwrap();

    let no_op = Migrator::embedded()
        .apply_pending(&pool)
        .await
        .expect("repeated boot after V016");
    assert!(no_op.is_empty(), "V016 must be idempotent at migration-engine level");
}
