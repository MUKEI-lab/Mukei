#![cfg(feature = "rusqlite")]

//! Regression coverage for the packaged/mobile migration source.
//!
//! Universal Storage is unusable in release builds unless the append-only
//! storage migrations are bundled by `Migrator::embedded()`.

use mukei_core::storage::migrations::Migrator;

#[test]
fn embedded_migrations_include_universal_storage_and_forward_hardening() {
    let migrations = Migrator::embedded()
        .list_available()
        .expect("embedded migrations must be readable");

    let versions: Vec<u32> = migrations.iter().map(|(version, _, _)| *version).collect();
    assert_eq!(
        versions,
        (1..=16).collect::<Vec<_>>(),
        "embedded migrations must be contiguous through V016"
    );

    let universal_storage = migrations
        .iter()
        .find(|(version, _, _)| *version == 14)
        .expect("V014 Universal Storage migration must be embedded");
    assert_eq!(
        universal_storage.1,
        "V014__universal_storage_and_workspaces"
    );
    assert!(universal_storage
        .2
        .contains("CREATE TABLE IF NOT EXISTS storage_scopes"));
    assert!(universal_storage
        .2
        .contains("CREATE TABLE IF NOT EXISTS import_transactions"));

    let isolation_guards = migrations
        .iter()
        .find(|(version, _, _)| *version == 15)
        .expect("V015 workspace isolation migration must be embedded");
    assert_eq!(isolation_guards.1, "V015__workspace_scope_isolation_guards");
    assert!(isolation_guards
        .2
        .contains("storage_nodes_parent_same_scope_insert"));
    assert!(
        !isolation_guards
            .2
            .contains("storage_node_identity_immutable"),
        "V015 must remain frozen; post-V015 hardening belongs in V016"
    );

    let hardening = migrations
        .iter()
        .find(|(version, _, _)| *version == 16)
        .expect("V016 storage hardening migration must be embedded");
    assert_eq!(hardening.1, "V016__storage_identity_and_recovery_hardening");
    assert!(hardening.2.contains("storage_node_identity_immutable"));
    assert!(hardening
        .2
        .contains("operation_journal_terminal_evidence_immutable"));
}
