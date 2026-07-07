# Mukei Bridge Development

## Dependencies

Install a stable Rust toolchain with `rustfmt` and `clippy`, plus CMake, Ninja,
pkg-config, Qt 6 development packages, Qt Quick, Qt QML, Qt Quick Controls 2,
Qt Quick Test, Qt Quick Layouts, Qt Quick Effects, Qt SVG, SQLite development
headers, and OpenSSL development headers when dependency resolution requires
native TLS crates.

On this workspace filesystem, Cargo build scripts may fail with `Permission
denied` if artifacts are written under `rust/target`. Use an executable target
directory:

```sh
export CARGO_TARGET_DIR=/tmp/mukei-target
```

## Rust

```sh
cd rust
cargo fmt --all -- --check
cargo test -p mukei-core --no-default-features --features std,tokio,rusqlite --lib --tests
cargo clippy -p mukei-core --no-default-features --features std,tokio,rusqlite --lib --tests -- -D warnings
```

The workspace feature set includes optional `candle` and `usearch_hnsw` paths.
On aarch64 hosts those dependencies may require an fp16-capable Rust target
configuration before `cargo test --workspace --all-features` can compile.

## QML

```sh
cmake -S qml -B /tmp/mukei-qml-build -G Ninja
cmake --build /tmp/mukei-qml-build
ctest --test-dir /tmp/mukei-qml-build --output-on-failure
```

## Feature Flags

`mukei-core` keeps storage, networking, model, and vector-search integrations
behind explicit feature flags such as `rusqlite`, `network`, `candle`,
`usearch_hnsw`, and `llama_cpp`. Test-only hooks must use a clearly named
non-default feature such as `test-hooks`; none are currently required for the
bridge dispatcher tests.

## Event Envelope

Rust emits JSON objects with this shape:

```json
{
  "schema_version": 1,
  "timestamp": "2026-07-04T13:00:00Z",
  "category": "chat_chunk",
  "sequence": 1
}
```

Optional correlation fields are `conversation_id`, `turn_id`, and `message_id`.
QML accepts only known `category` values and validates each category's required
payload before emitting `EventDispatcher.eventReceived`.

Lifecycle states are represented by typed Rust enums and serialized as
snake_case: app lifecycle, chat turn state, download state, capability
snapshots, and typed UI errors.
