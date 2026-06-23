<div align="center">

# Mukei

**A zero-telemetry, fault-tolerant, on-device AI agent.**
Built in Rust, fronted by Qt 6 + QML, accelerated by llama.cpp.

[![tests](https://img.shields.io/badge/tests-157%20passing-success)](#tests)
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
mukei-core      160 unit + 12 integration
mukei-ffi-shim    2 unit
                ────────────────────
                174 passed total
```

Verified invariants:

- **Issue #1 (REQ-SEC-04 strengthened).** Every untrusted string interpolated into a `<external_data>` wrapper is passed through [`crate::tools::sentinel::escape_untrusted`](rust/crates/mukei-core/src/tools/sentinel.rs). A hostile web page / file / RAG snippet cannot forge a closing tag and impersonate a `trust="trusted"` block.
- **Issue #9 / #10 (REQ-AGT-04 strengthened).** The agent loop never hard-aborts on tool-call parse / validation errors. Malformed calls become structured `tool_error` envelopes; valid calls in a mixed batch still execute. Abuse-blocked fingerprints are caught BEFORE dispatch so a maxed-out tool never burns network / disk again.
- **Issues #4 / #5 / #6 / #7 (per-turn reset).** `AgentLoop::run` rearms the wall-clock watchdog, clears the failure tracker + same-output ring, and resets `HardwareTool` cache generation at the top of every invocation. State no longer leaks across turns / conversations.
- **Issue #2 (SAF + audit log persistence).** SAF grants persist through `saf_tokens` via [`SafRegistry::persist_upsert`](rust/crates/mukei-core/src/storage/saf.rs); tool-call rows are written through the hash-chained [`AuditLogWriter`](rust/crates/mukei-core/src/storage/audit_log.rs).
- **Issue #3 (wrapped-secrets key delivery).** Brave / Tavily API keys flow `Bridge → with_keys → SearchPlanner` directly. No process env vars; a typo is a compile error.
- **Issue #11 (RAG reconciliation).** Boot-time [`rag::reconcile`](rust/crates/mukei-core/src/rag/indexer.rs) walks `chunks` rows and the vector store and reports orphans created by mid-commit kills.
- **Issue #12 (migration gap check).** `Migrator::verify_order` runs the contiguity check **unconditionally**; a `[1,3]` applied set cannot quietly receive migration 4 anymore.
- **Issue #13 / #14 (config wiring).** `MukeiConfig::load_and_validate` runs in `MukeiAgent::initialize`; `AgentCfg → ToolExecutionPolicy` conversion lets `config.toml::[agent]` actually take effect.
- **Issue #15 (O(n) context trim).** Per-message tokens are computed once and the running total is decremented on each trim; the RAG block's own tokens count against `max_tokens`.
- **Issue #16 (math timeout slot release).** On timeout, `JoinHandle::abort()` is called so `TOOL_BLOCKING_SLOTS` is reaped instead of leaked.
- **Issue #17 (embedder soundness).** `CandleMiniLmEmbedder::embed` no longer casts `&self` to `usize` for `spawn_blocking`; an explicit `clone_for_blocking` keeps the model alive via candle's internal Arc-sharing.
- **Issue #18 / #19 / #20 (hygiene).** Duplicate `MAX_FAILURES_PER_TOOL` constant deleted; `MukeiError::classification` is exhaustive (no silent `_ => Unknown`); the stale `ToolResult::ok` hard-abort doc comment is gone.
- **Issue #8 (TagsStreaming).** Close-tag branch no longer wipes text following `</think>` in the same chunk; tail is preserved so a same-chunk follow-up open is still detected and visible answer text survives.
- **PermissionMatrix scaffold.** [`crate::tools::permission::PermissionMatrix`](rust/crates/mukei-core/src/tools/permission.rs) declares per-tool `Capability` requirements (Internet / DiskRead / DeviceState / SandboxEval / …). Covers every tool in `ALLOWED_TOOLS` (enforced by unit test).
- **Migration §2 — No DuckDuckGo.** Production search uses Brave + Tavily only, picked per task by the adaptive [`SearchPlanner`](rust/crates/mukei-core/src/search/planner.rs). A compile-time tripwire + CI guard reject any reintroduction.
- **Migration §3-13 — Adaptive Search Planner.** Intent analysis → task split → classification → engine selection → ranking → trust gating → cache. No unconditional fan-out, per-engine timeouts (Brave 3s / Tavily 5s).
- **PRD REQ-RAG-01 / -02 / -03.** Real candle-backed MiniLM embedder, optional usearch HNSW backend, embedder-swap detection (`StoreHeader`), shred / forget functionality.
- **PRD REQ-SEC-01.** Full-file SHA-256 verification of GGUF models BEFORE `mmap`.
- **PRD REQ-SEC-19.** SQLCipher key handling with `PRAGMA key` + zeroisation.
- **PRD REQ-AGT-04** — Tool Execution Policy with configurable threshold (default 5), failure-kind classification (`Transient` / `Validation` / `Cancelled` / `Timeout` / `Permanent` / `Abuse`), structured feedback envelopes, and no-progress detection (see [`crates/mukei-core/src/agent/tools/`](rust/crates/mukei-core/src/agent/tools/)).
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
│   │   ├── mukei-core/          ← pure-Rust kernel · 160 unit + 12 integration tests
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
