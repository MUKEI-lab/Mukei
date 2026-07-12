# Mukei v0.2 Sol — Rust/backend hardening batch

Base: the icons-restored `Mukei_v0.1_sol.zip` package.

## Implemented

1. **Android-safe first-run storage configuration**
   - Runtime default config is generated below the directory containing `mukei.toml`.
   - Model, vector, database, crash and log directories are created before database startup.
   - Server-style `/var/mukei` paths are no longer written during first-run bootstrap.

2. **Conversation isolation and current-prompt de-duplication**
   - Context history is selected by both `conversation_id` and `branch_id`.
   - Only completed, non-deleted messages are loaded.
   - The active user message uses the same durable message ID written by `ConversationRepository` and is removed from loaded history, preventing the current prompt from appearing twice.

3. **Operational migration safety**
   - Added a transactional migration lease using `migration_lock`.
   - Stale/bootstrap locks are reclaimed; a live lock returns `ERR_MIGRATION_LOCKED`.
   - Databases marked newer than the bundled schema return `ERR_SCHEMA_TOO_NEW`.
   - Existing v0.8 and the accidental v0.9 V008 migration lineages remain supported.
   - Added `V010__reliability_hardening.sql`.

4. **Crash-retryable SAF/RAG deletion**
   - SAF revoke remains DB-first.
   - SQL chunks are deleted transactionally.
   - Vector chunk IDs are persisted in `document_tombstone`.
   - Failed vector cleanup remains pending with a redacted error and is retried at boot.
   - Tombstoned SAF tokens cannot be silently re-added.

5. **Durable model-download jobs and quota reservations**
   - Added `download_jobs` and `storage_reservations` tables/repository.
   - Concurrent download starts are serialized by an `IMMEDIATE` transaction.
   - Reservations are released on complete/fail/cancel and stale jobs are recovered at boot.
   - Resume quota calculation no longer double-counts the current `.partial` prefix.

6. **Network retry wiring**
   - Bounded exponential full-jitter retry is now used by Brave, Tavily and model-download request setup.
   - Retries are limited to typed transient failures.
   - Model-download cancellation interrupts both an in-flight request and retry sleep.

7. **Secret-memory hardening**
   - SQLCipher key material is held in `Zeroizing<Vec<u8>>` across bridge/pool startup.
   - Provider registry rebuilds move zeroizing strings into provider engines and reduce ordinary long-lived key copies.

8. **Typed database-domain errors**
   - Database closures can preserve `MukeiError` instead of flattening every failure into `ERR_DB_INIT`.
   - Migration lock/newer-schema errors receive stable UI mappings.

## Static validation performed

- ZIP/source structure and local Cargo path checks
- TOML and XML parsing
- QML resource check: 36 entries, 0 missing (27 restored SVG icons retained)
- `include_str!`/`include_bytes!` local path checks
- Fresh SQLite migration simulation: V001 through V010
- Legacy v0.8 and accidental v0.9 V008 upgrade simulations
- Rust delimiter/comment/string structural scan
- Native vendored llama CMake configuration

## Not claimed

Rust compilation, Clippy and Rust tests were not executed because this environment does not contain `cargo`/`rustc`. Qt/QML and Android builds were not executed because the matching Qt/Android toolchains are unavailable. Compile manually before release.

## Suggested manual compile order

```bash
cd rust
cargo fmt --all -- --check
cargo check -p mukei-core --all-features
cargo test -p mukei-core --all-features
cargo clippy -p mukei-core --all-targets --all-features -- -D warnings
cargo check -p mukei-bridge --all-features
cargo clippy -p mukei-bridge --all-targets --all-features -- -D warnings
cargo check --workspace --all-features
cargo test --workspace --all-features
```

Then configure/build the Qt and Android targets with the project’s intended Qt 6 Android kit and NDK.

## Still open after this batch

- True interrupted-turn `resume_turn` / `regenerate_turn` API
- Durable first-class persistence of tool calls, tool outputs and intermediate assistant states
- Android Keystore-backed provider `SecretStore`
- Single coordinated `AppState` state machine instead of scattered globals
- Audit-event linkage for document deletion tombstones
- Reconciliation of a server-reported model size when it differs from catalog reservation metadata
- Open-source licensing decision/metadata, if the project is intended to be open source
