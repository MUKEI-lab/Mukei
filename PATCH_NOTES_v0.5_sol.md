# Mukei v0.5 Sol — Recovery, Secrets, Migration and Runtime Patch

Base: `Mukei_v0.4_sol` (therefore includes every v0.4 durability fix)

## Implemented

### Interrupted-turn recovery

- Added bridge APIs to inspect, resume, or regenerate the interrupted turn.
- Recovery creates a new durable assistant row and never overwrites the old
  partial response.
- Resume seeds the agent with the interrupted prefix; regenerate creates a
  sibling response from the original user prompt.
- Recovered turns stream, persist partial output, support cancellation, and
  finalize through the same durable message graph as normal turns.

### Runtime coordination

- Added formal runtime phases: Uninitialized, Initializing, DatabaseOpened,
  AuditVerified, Ready, and Quarantined.
- Send/recovery operations are rejected unless runtime is Ready.
- Failed initialization enters Quarantined state.
- Concurrent/repeated initialization is rejected instead of racing global
  state publication.

### Migration safety

- Creates a byte-for-byte encrypted pre-migration database backup after WAL
  checkpoint and before numbered migrations run.
- Existing migration lease, checksum, schema-version and legacy v0.9 repair
  protections remain enabled.

### Provider SecretStore

- Added AndroidKeyStore AES-256-GCM wrapping with a non-exportable key.
- Only ciphertext is stored in the app-private files directory.
- Provider secrets are hydrated at boot and kept in zeroizing Rust memory.
- Empty setters securely delete persisted secret blobs.
- JNI class resolution uses the app Context class loader, including from
  Tokio-created native threads.
- SQLite stores only opaque `secret_refs`, never plaintext provider keys.

### RAG revoke/delete guarantees

- Revoke remains DB-first and vector cleanup remains crash-retryable.
- Tombstones without a linked mandatory audit event are repaired at boot.
- Audit arguments use token fingerprints instead of raw SAF tokens.
- Cleanup failure logs no longer expose raw document tokens.

### Download and diagnostics hardening

- Server-reported model size now atomically resizes the durable reservation
  and rechecks aggregate quota before response bytes are accepted.
- Bridge error strings are passed through the privacy redactor before QML
  signals receive them.

## Dependency-lock note

v0.5 adds Android-target dependencies `jni = 0.21` and `ndk-context = 0.1`.
Because this patching environment had no Cargo executable, `Cargo.lock` could
not be regenerated. The first normal Cargo command will update the lockfile.
Do not use `--locked` until that update is committed.

## Manual compile verification required

Run from `rust/`:

```bash
cargo fmt --all -- --check
cargo check -p mukei-core --all-features
cargo test -p mukei-core --all-features
cargo check -p mukei-bridge --all-features
```

For Android, configure the NDK linker/toolchain and then check the real target:

```bash
rustup target add aarch64-linux-android
cargo check -p mukei-bridge --target aarch64-linux-android --all-features
```
