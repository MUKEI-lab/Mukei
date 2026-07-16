# Mukei Rust workspace

This workspace is shared by the Kotlin Android production application.

## Crates

- `mukei-core`: platform-neutral product engine.
- `mukei-android-jni`: Android JNI boundary, added by the bridge scaffold.
- `llama-cpp-stub`: guarded development placeholder for the native inference adapter.

Qt and CXX-Qt are intentionally not part of the `Kotlin` branch.

## Validation

From `rust/`:

```bash
cargo fmt --all -- --check
cargo check --workspace
cargo test -p mukei-core --all-features
```

A clean `Cargo.lock` will be regenerated after the new workspace dependency graph is validated.
