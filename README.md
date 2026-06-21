<div align="center">

# Mukei

**A zero-telemetry, fault-tolerant, on-device AI agent.**
Built in Rust, fronted by Qt 6 + QML, accelerated by llama.cpp.

[![tests](https://img.shields.io/badge/tests-70%20passing-success)](#tests)
[![rust](https://img.shields.io/badge/rust-1.78%2B-orange)](#requirements)
[![license](https://img.shields.io/badge/license-Proprietary-lightgrey)](#license)
[![status](https://img.shields.io/badge/status-architecture%20pass-blue)](#project-status)

</div>

---

## What is Mukei?

Mukei is a privacy-first AI agent designed to run **entirely on the user's device** — no telemetry, no cloud round-trips, no opaque background calls. It targets mid-range Android phones and Linux/macOS desktops, with a clean separation between a pure-Rust kernel and the Qt/QML user interface.

| | |
|---|---|
| 🎯 **Mission** | A trustworthy on-device agent that survives OOM kills, thermal throttling, KV-cache corruption, and tool-loop death spirals. |
| 🛡️ **Privacy** | Zero telemetry. Local inference via `llama.cpp`. All secrets wrapped with Android Keystore. |
| 🧱 **Architecture** | Rust core (`mukei-core`) + CXX-Qt bridge (`mukei-bridge`) + manual C-FFI fallback (`mukei-ffi-shim`). |
| 🪪 **Provenance** | Every requirement traces to the [TRD](rust/docs/TRD.md), [PRD](rust/docs/PRD.md), [Backend Schema](rust/docs/BS.md), [Application Flow](rust/docs/AF.md), or [UX Brief](rust/docs/UXB.md). |

---

## Architecture at a glance

```
┌──────────────────────────────────────────────────────────────────┐
│                         Qt 6 + QML  (editorial-luxury UI)        │
└──────────────┬───────────────────────────────────────────────────┘
               │ CXX-Qt signals / invokables  +  manual extern "C"
┌──────────────▼──────────────────┐   ┌─────────────────────────────┐
│        mukei-bridge             │   │      mukei-ffi-shim         │
│  MukeiAgent · MukeiBridge       │   │  callback_with_guard!       │
│  SafRegistry · JNI hooks        │   │  generation counter ABI     │
└──────────────┬──────────────────┘   └──────────────┬──────────────┘
               │                                     │
               └─────────────────┬───────────────────┘
                                 │
                  ┌──────────────▼──────────────┐
                  │         mukei-core          │
                  │ (pure-Rust, Qt-free)        │
                  ├─────────────────────────────┤
                  │ agent · engine · rag        │
                  │ tools · storage · config    │
                  │ diagnostics · ffi-types     │
                  └──────────────┬──────────────┘
                                 │
                  ┌──────────────▼──────────────┐
                  │  llama.cpp  +  candle       │
                  │  SQLite (WAL / SQLCipher)   │
                  │  usearch HNSW               │
                  └─────────────────────────────┘
```

Three crates, one direction of dependency. `mukei-core` never links Qt — that keeps the agent kernel testable on any host while the bridge/shim layers handle the platform integration.

---

## Project status

| Area | Status | Notes |
|---|---|---|
| **Rust kernel** | ✅ Stable scaffold | 68 unit tests, zero failures |
| **C-FFI shim** | ✅ Stable scaffold | 2 unit tests, zero failures |
| **CXX-Qt bridge** | 🟡 Compiles under Qt | Requires a Qt 6.5+ install on the host |
| **Migrations V001–V004** | ✅ Authored | Conversations, messages, chunks, recovery, audit, SAF tokens, branches |
| **GBNF tool grammar** | ✅ Per-tool schema | `grammars/tool_calling.gbnf` |
| **llama.cpp integration** | 🟡 Stubbed in core | Real load lives in the bridge; prebuilt `libllama.a` per ABI |
| **Candle MiniLM embedder** | 🟡 Behind feature flag | Default build uses a deterministic mock embedder |
| **QML editorial-luxury UI** | ⏳ Out of scope for this repo | Tracked in the UX Brief |

---

## Tests

```bash
cd rust
cargo test -p mukei-core      --no-default-features --features "std,tokio" --lib
cargo test -p mukei-ffi-shim  --lib
```

Current run:

```
mukei-core      68 passed · 0 failed
mukei-ffi-shim   2 passed · 0 failed
                ────────────────────
                70 passed total
```

Verified invariants:

- **TRD §1.3 / REQ-ARCH-05** — `CallbackGuard` + `catch_unwind` wrap for every FFI callback.
- **TRD §1.2.5** — Thinking-tag streaming detector with 64-byte sliding window, multi-transition per push.
- **TRD §2.2** — Bounded tokio runtime: `MAX_BLOCKING_THREADS=6` on Android, `TOOL_BLOCKING_SLOTS=2`.
- **TRD §2.4** — Spawn-blocking enforced for every SQLite/disk path (the “Golden Rule”).
- **TRD §4.4** — Vector store atomic-rename save; never blocks the async runtime.
- **TRD §5.5** — Sandboxed `math_eval` with identifier whitelist + 8 s timeout.
- **TRD §12.5** — Strict TOML config rejecting unknown root fields.
- **TRD §13.3** — Post-parse tool-call validator (SAF token enforcement, per-tool schema).
- **TRD §36.1** — Crash-loop fingerprint sink with stable SHA-256.

---

## Build matrix

| Feature | Purpose | Sandbox? |
|---|---|---|
| `std`, `tokio` | Always-on. Bounded runtime + types. | ✅ |
| `rusqlite` | SQLite + r2d2 pool (TRD §6). | ✅ (with SQLite installed) |
| `sqlcipher` | Encrypted backend; superset of `rusqlite`. | ⚠️ host setup |
| `network` | `reqwest` + `scraper` for web search. | ⚠️ rustc ≥ 1.86 (icu deps) |
| `candle` | Local MiniLM embedder. | ⚠️ host setup |
| `usearch_hnsw` | usearch-backed vector store. | ⚠️ host setup |
| `llama_cpp` | Real `llama-cpp-rs` binding. | ❌ needs Qt + prebuilt libllama |
| `android_keystore` | Wrapped-key secrets path. | ❌ Android only |

A single typical sandbox check:

```bash
cargo check -p mukei-core --no-default-features --features "std,tokio,rusqlite"
```

---

## Repository layout

```
Mukei/
├── README.md                    ← you are here
├── rust/                        ← Rust workspace (build target)
│   ├── Cargo.toml               ← panic = "unwind" pinned on every profile
│   ├── README.md                ← engineering README
│   ├── crates/
│   │   ├── mukei-core/          ← pure-Rust kernel · 68 tests
│   │   ├── mukei-bridge/        ← CXX-Qt + JNI surface (Qt host required)
│   │   ├── mukei-ffi-shim/      ← manual extern "C" escape hatch · 2 tests
│   │   └── llama-cpp-stub/      ← workspace placeholder for the vendor build
│   ├── migrations/              ← V001–V004 · 000_default_config.toml
│   ├── grammars/                ← tool_calling.gbnf (strict per-tool)
│   └── llama-cpp-prebuilt/      ← one-shot libllama.a per ABI (CMake)
└── rust/docs/                   ← TRD / PRD / BS / AF / UXB design pass v0.7.5
```

The full design pass — TRD v0.7.5 (≈6,400 lines), PRD v0.7.5, Backend Schema v1.2, Application Flow v1.2, UX Brief v2.1 — lives under [`rust/docs/`](rust/docs/). They are intentionally large; the engineering README in [`rust/README.md`](rust/README.md) is the right entry point for contributors.

---

## Requirements

- **Rust 1.78+** (workspace MSRV). Pinned via `rust-toolchain` semantics.
- **Qt 6.5+** for the `mukei-bridge` build only.
- **SQLite 3.40+** with WAL support (bundled when `rusqlite` feature is on).
- **Android NDK r26+** for on-device builds (target `aarch64-linux-android`).
- A vendored `llama.cpp` checkout under `rust/llama-cpp-prebuilt/vendor/llama.cpp` for real inference builds; the workspace falls back to a stub crate otherwise.

---

## Quick start (Linux/macOS sandbox)

```bash
git clone https://github.com/MUKEI-lab/Mukei.git
cd Mukei/rust

# Pure-Rust kernel (no Qt, no llama.cpp)
cargo test -p mukei-core --no-default-features --features "std,tokio" --lib
cargo test -p mukei-ffi-shim --lib

# With SQLite-backed storage
cargo check -p mukei-core --features "tokio,rusqlite"
```

For the Qt-integrated bridge build, follow the dedicated guide in [`rust/README.md`](rust/README.md).

---

## Documentation

| Document | Purpose |
|---|---|
| [Engineering README](rust/README.md) | Per-crate layout, feature matrix, test commands |
| [TRD v0.7.5](rust/docs/TRD.md) | Technical Reference Document — the build-this-exactly spec |
| [PRD v0.7.5](rust/docs/PRD.md) | Product Requirements Document — what + why |
| [Backend Schema v1.2](rust/docs/BS.md) | SQL schema, migrations, retention policy |
| [Application Flow v1.2](rust/docs/AF.md) | Boot, model acquisition, tool execution pipeline |
| [UX Brief v2.1](rust/docs/UXB.md) | Editorial-luxury design system |

---

## License

Proprietary. © 2026 Mukei. All rights reserved.
