# Mukei Bridge Development

This document describes the current bridge boundary in the post-merge hardening
snapshot.

## Dependencies

For Rust-only work, install a stable Rust toolchain with `rustfmt` and `clippy`.

For native bridge/QML work, also install CMake, Ninja, a Qt 6.5+ development
toolchain (Qt Quick, QML, Controls, Layouts, Effects, SVG, Quick Test), and the
Android toolchain when targeting Android.

If the workspace filesystem cannot execute build artifacts from `rust/target`,
use an executable target directory:

```sh
export CARGO_TARGET_DIR=/tmp/mukei-target
```

## Rust verification

Representative core checks:

```sh
cd rust
cargo fmt --all -- --check
cargo check --workspace
cargo clippy -p mukei-core --all-targets --all-features -- -D warnings
cargo test -p mukei-core --all-features
cargo test -p mukei-ffi-shim
```

The bridge itself requires the CXX-Qt/Qt toolchain:

```sh
cargo check -p mukei-bridge --features "sqlcipher,network"
cargo clippy -p mukei-bridge --all-targets --features "sqlcipher,network" -- -D warnings
```

The current archive is not documented as passing these commands until they are
rerun on this exact snapshot.

## QML verification

```sh
cmake -S qml -B /tmp/mukei-qml-build -G Ninja
cmake --build /tmp/mukei-qml-build
ctest --test-dir /tmp/mukei-qml-build --output-on-failure
```

Static guards are also available:

```sh
python3 qml/scripts/qml_architecture_analyzer.py qml
python3 qml/scripts/qml_contract_guard.py qml
python3 qml/scripts/qml_security_analyzer.py qml
```

## Runtime deployment modes

`mukei-bridge` separates runtime environment from compiler profile.

Relevant feature families include:

- storage: `rusqlite`, `sqlcipher`;
- network: `network`;
- inference/RAG: `llama_cpp`, `candle`, `usearch_hnsw`;
- platform: `android_keystore`, `desktop`;
- environment: `runtime_development`, `runtime_test`, `runtime_production`;
- hardening policy: `runtime_hardening`;
- diagnostics capability: `diagnostics_export`.

A compiled capability is not automatically a policy grant. Remote use is also
subject to the core `RemoteFeaturePolicy`.

## Protocol V2 command boundary

The current command boundary uses `CommandEnvelopeV2` and returns an immediate
`CommandAcknowledgementV2`.

An accepted acknowledgement means only that the command was validated and
accepted for processing. It does not mean the operation completed.

The bridge validates bounded envelope size, protocol major version, identifiers,
command type, scope, payload, capability/policy preflight, and idempotency before
dispatch.

Idempotent replays return the original operation identity when the replay is
equivalent. Conflicting reuse of an idempotency key is rejected.

## Protocol V2 event boundary

Production Rust bridge events are wrapped in `EventEnvelopeV2`.

Conceptual shape:

```json
{
  "protocol": { "major": 2, "minor": 0 },
  "event_id": "opaque-event-id",
  "stream_id": "conversation:<conversation>:branch:<branch>",
  "sequence": 42,
  "event_type": "chat_state",
  "emitted_at": "2026-07-12T00:00:00Z",
  "correlation_id": "opaque-correlation-id",
  "operation_id": "opaque-operation-id",
  "payload": {}
}
```

Sequencing is compared only within a `stream_id`. Duplicate event IDs, stale
sequence values, malformed envelopes, and unsupported protocol majors are
rejected by the QML event boundary.

The standalone desktop compatibility implementation can remain in an explicitly
negotiated legacy-event mode. See
[`PROTOCOL_V2_ARCHITECTURE.md`](PROTOCOL_V2_ARCHITECTURE.md).

## Non-chat asynchronous results

`async_bridge` coordinates non-chat work that may touch SQLite, filesystem, or
another potentially blocking service.

Each request receives a request ID plus a per-domain generation. QML must apply a
completion only when it is current for that domain. This prevents an old
completion from replacing a newer projection.

The current asynchronous ownership covers recovery, UI-session/draft, download,
settings, storage, and private-document surfaces. Chat protocol and other
separately owned projections keep their own lifecycle contracts.

## Secure bootstrap

The bridge models an explicit database bootstrap lifecycle around Android
Keystore-backed wrapping and SQLCipher database-key use.

Plaintext database-key bytes are held only in zeroizing memory and are never
serialized, logged, or sent to QML.

Bootstrap failure states distinguish key invalidation, wrapped-key corruption,
database-open failure, and reset-required conditions.

## Runtime provenance

Diagnostics-facing provenance keeps product version, protocol version, database
schema version, build ID, compiler profile, runtime environment, hardening mode,
and feature flags separate.

Do not use one of these fields as a proxy for another compatibility or security
claim.
