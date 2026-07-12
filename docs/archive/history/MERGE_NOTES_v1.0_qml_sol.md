# Mukei v1.0 QML Solution — Production Merge Notes

## Inputs

- `mukei_v0.6_sol.zip`: latest available Rust compile/test-fix branch.
- `Mukei_v0.15_qml_phase6.zip`: current persistent reactive QML architecture branch.
- `mukei_v0.9.zip`: common ancestor used for three-way classification.

## Merge policy

1. Preserve v0.15 as the QML, CMake, Qt adapter, migration V011/V012, UI-contract, and documentation baseline.
2. Import v0.6 source and test changes that were not independently changed by v0.15.
3. Three-way merge files changed on both branches against v0.9.
4. Exclude `rust/target`, caches, temporary merge copies, and repository metadata.
5. Do not claim native release certification without a fresh combined Cargo/Qt/Android run.

## Three-way result

- Identical across both branches: 1,609 files.
- v0.15-only architecture additions/changes: preserved.
- v0.6-only Rust fixes/tests: imported.
- Files independently changed by both branches:
  - `rust/crates/mukei-bridge/src/lib.rs`
  - `rust/crates/mukei-core/src/storage/settings.rs`
- Textual merge conflicts: **0**.

## v0.6 changes retained

- `AgentRunRequest` replaces the Clippy-triggering eight-argument `AgentLoop::run` interface.
- Bridge call site updated to the typed request while retaining conversation/branch scoped UI events.
- Conversation history isolation tests for conversation + branch boundaries.
- Interrupted-turn recovery tests for resume and regenerate behavior.
- Vector-store iterator Clippy cleanup.
- Desktop bridge cfg/dead-code cleanup.
- FFI raw-pointer functions marked `unsafe`, documented, and tests updated.
- Settings Clippy cleanups.

## v0.15 architecture retained

- Persistent reactive QML architecture and scoped stores.
- Fail-closed QML/Rust contract negotiation.
- Lifecycle-derived routing and capability gating.
- UI session and per-branch draft persistence.
- Native timeline adapter, snapshot hydration, pagination, and batched streaming.
- Conversation, recovery, model, download, document, settings, diagnostics, operation, storage, accessibility, and responsive projections.
- Android document permission boundary and durable ingestion jobs.
- Migrations V011 and V012.
- QML architecture/security/contract analyzers and behavioral tests.

## Additional merge hardening

- Removed four redundant `map_err(Into::into)` calls introduced in post-v0.6 storage paths, matching the Clippy fix pattern already applied to settings.
- Added an explicit unsafe block in the FFI stop-generation function.
- Corrected README status and badges so historical Rust results are not represented as certification of the combined source.
- The earlier v0.6 command transcript was retained during the original merge process for provenance, but raw command logs are not part of the current repository taxonomy.

## Native release gates

The combined tree still requires these commands in a toolchain-equipped workspace:

```bash
cd rust
cargo fmt --all -- --check
cargo check --workspace
cargo check -p mukei-core --no-default-features --features "std,tokio"
cargo check -p mukei-core --no-default-features --features "std,tokio,rusqlite"
cargo check -p mukei-core --no-default-features --features "std,tokio,sqlcipher"
cargo check -p mukei-core --no-default-features --features "std,tokio,network"
cargo clippy -p mukei-core --all-targets --all-features -- -D warnings
cargo clippy -p mukei-bridge --all-targets --features "sqlcipher,network" -- -D warnings
cargo test -p mukei-core --all-features
cargo test -p mukei-ffi-shim
```

Then build and test the QML application with Qt 6.5+, run QuickTest/CTest, build Android JNI/Gradle targets, and validate SAF, lifecycle recovery, downloads, accessibility, and model-session behavior on a physical device.
