# Mukei Rust Core

Pure-Rust kernel for the Mukei on-device AI agent.

This workspace implements [TRD v0.7.5](../TRD_v0.7.5_architect_pass.md), the
[Backend Schema v1.2](../MUKEI-BS_v1.2_BackendSchema.md), and the
[Application Flow v1.2](../MUKEI-AF_v1.2_ApplicationFlow.md).

## Crates

| Crate | Role |
|---|---|
| `mukei-core` | Pure-Rust agent / engine / RAG / storage / diagnostics kernel |
| `mukei-bridge` | CXX-Qt bridge exposing `MukeiAgent`, `MukeiBridge`, `SafRegistry` + JNI helpers (Qt build only) |
| `mukei-ffi-shim` | Manual `extern "C"` escape-hatch FFI paired with CXX-Qt |
| `llama-cpp-stub` | Workspace placeholder until the `llama-cpp-prebuilt/CMakeLists.txt` one-shot build is wired |

## Sandbox build (no Qt, no SQLite, no candle)

```bash
cd rust
cargo check -p mukei-core --no-default-features --features "std,tokio"
cargo test  -p mukei-core --no-default-features --features "std,tokio"
cargo check -p mukei-ffi-shim
cargo test  -p mukei-ffi-shim
```

## Full build (with SQLite + RAG)

```bash
cd rust
cargo check -p mukei-core --features "tokio,rusqlite"
cargo check -p mukei-core --features "tokio,rusqlite,candle"
```

> Note: `usearch`, `candle`, and `llama-cpp-rs` need per-target setup.
> `llama-cpp-prebuilt/CMakeLists.txt` produces a one-shot `libllama.a` per ABI.

## Test coverage

```
mukei-core      67 tests passing
mukei-ffi-shim   1 test  passing
```

Verified invariants include:
- `MAX_BLOCKING_THREADS=6` on Android, `TOOL_BLOCKING_SLOTS=2` (TRD §2.2)
- `CallbackGuard` u64 ABI + `catch_unwind` (TRD §1.3)
- Thinking-tag streaming detector with `TAG_WINDOW=64` (TRD §1.2.5)
- Atomic-rename vector store save (TRD §4.4)
- Sandboxed `math_eval` with whitelist + 8 s timeout (TRD §5.5)
- Strict TOML config validator rejecting unknown fields (TRD §12.5)
- Post-parse tool validator + SAF-token enforcement (TRD §5.2, §13.3)
- Crash-loop fingerprint sink (TRD §36.1)

## Directory layout

```
rust/
├── Cargo.toml
├── crates/
│   ├── mukei-core/        (lib)
│   ├── mukei-bridge/      (cdylib + staticlib)
│   ├── mukei-ffi-shim/    (staticlib)
│   └── llama-cpp-stub/    (lib)
├── migrations/
│   ├── 000_default_config.toml
│   ├── V001__schema.sql           (conversations, messages, chunks)
│   ├── V002__recovery_state.sql   (crash-safe stream resume)
│   ├── V003__tooling_and_saf.sql  (audit log + SAF token table)
│   └── V004__branching.sql        (branch graph)
├── grammars/
│   └── tool_calling.gbnf
└── llama-cpp-prebuilt/
    └── CMakeLists.txt
```
