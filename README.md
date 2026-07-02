<div align="center">

# Mukei

**A zero-telemetry, fault-tolerant, on-device AI agent.**
Built in Rust, fronted by Qt 6 + QML, accelerated by llama.cpp.

[![tests](https://img.shields.io/badge/tests-251%20passing-success)](#tests)
[![rust](https://img.shields.io/badge/rust-1.78%2B-orange)](#requirements)
[![license](https://img.shields.io/badge/license-Proprietary-lightgrey)](#license)
[![status](https://img.shields.io/badge/status-main%20CI%20green-blue)](#project-status)

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
| 🤖 **Models** | Gemma 4 E2B (3.46 GB Q4_K_M) for 4-6 GB devices, Gemma 4 E4B (5.41 GB Q4_K_M) for 8 GB+ devices, downloaded on-device and SHA-256 verified before mmap. |

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
│  SafRegistry · JNI hooks        │   │  generation + instance_id   │
│  BusyGuard · DownloadSlotGuard  │   │  ABI drift-detector test    │
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
| **Rust kernel** | ✅ Verified on main | All-features verification restored: 248 `mukei-core` tests passing (219 unit + 12 integration + 6 + 3 + 3 + 4 suites + 1 doc-test) |
| **C-FFI shim** | ✅ Stable scaffold | 3 unit tests; checked-in C header with drift-detector test |
| **CXX-Qt bridge** | 🟡 Compiles under Qt | Qt 6.5+ required on the host; sandbox CI skips this crate |
| **Migrations V001–V005** | ✅ Authored | Conversations, messages, chunks, recovery, audit, SAF tokens, branches, audit-chain checks |
| **GBNF tool grammar** | ✅ Per-tool schema | `grammars/tool_calling.gbnf` |
| **llama.cpp integration** | ✅ Prebuilt path merged | Real load lives in the bridge; the vendored/prebuilt `libllama.a` per-ABI pipeline is now merged into `main` |
| **Gemma 4 downloader** | ✅ Wired | Commit-pinned HF URLs, full-file SHA-256 verify, resumable, 416-restart safe |
| **Candle MiniLM embedder** | ✅ Verified on main | Default build still uses the deterministic mock embedder, but the real candle path now compiles and tests cleanly under `--all-features` after the `half = =2.4.1` compatibility repin |
| **QML editorial-luxury UI** | 🟡 Scaffold on `codex/create-qml-directory-structure-and-files` | 10 screens + 34 components + 4 theme singletons; real SIL OFL variable fonts and 27 unique Phosphor icons; `qml-check` CI enforces qmllint / SVG uniqueness / TTF magic / DDG regression / cmake configure |

---

## Tests

```bash
cd rust
cargo test -p mukei-core      --no-default-features --features "std,tokio"
cargo test -p mukei-ffi-shim
```

Current run on **`main`**:

> **Verification Pass — 2026-06-30.** The post-merge mainline is green end-to-end: `cargo fmt --all -- --check`, the three-way clippy matrix (`std,tokio`, `std,tokio,rusqlite`, `std,tokio,network`), sandbox invariants, `cargo test -p mukei-core --no-default-features --features "std,tokio,rusqlite"`, `cargo test -p mukei-ffi-shim`, `cargo test -p mukei-core --all-features`, `cargo deny --all-features -D warnings`, and `cargo audit` all pass. The previous candle/half/rand verification blocker is closed on `main`; explicit root `[search]` config is now admitted by `MukeiConfig::known_keys()`.

```
mukei-core  (--all-features) 219 unit + 12 integration + 6 fingerprint
                             + 3 grammar + 3 migrator + 4 sentinel
                             + 1 doc-test                 ──► 248 passed
mukei-ffi-shim                                                 3 passed
                                                           ─────────
                                                           251 passed total
```

### Verified invariants

- **Re-entrancy guards.** `MukeiAgent::send_message` flips an
  `AtomicBool` on entry; a second call returns `MukeiError::BridgeBusy`
  (`ERR_BRIDGE_BUSY`). Release is RAII via `BusyGuard::Drop` so even a
  panic in the streaming task clears the flag. `MukeiAgent::download_model`
  keys a global `Arc<Mutex<HashSet<PathBuf>>>` by destination path; a
  second download targeting the same dest is rejected with
  `MukeiError::DownloadBusy { dest }` (`ERR_DOWNLOAD_BUSY`) before any
  I/O happens, so the SHA-256 integrity check cannot be defeated by
  interleaved writes on a single `.partial` file. Two downloads of
  *different* models still run in parallel.
- **Independent cancellation tokens.** `MukeiAgent` carries
  `cancel_token` (chat) and `download_cancel` (downloads). The chat
  Stop button never kills a model download running in the background,
  and vice-versa.
- **Commit-pinned model catalogue.** `engine::model_registry` pins
  `download_url` to a Hugging Face `/resolve/<40-char-sha>/` revision,
  *not* `/resolve/main/`. The catalogue currently ships two Gemma 4
  GGUF variants: `gemma-4-e2b-it` (≈3.46 GB, recommended for ≥6 GB RAM
  devices) and `gemma-4-e4b-it` (≈5.41 GB, recommended for ≥8 GB RAM
  devices). Deprecated `gemma-3n-*` aliases still resolve for one
  migration window. CI tests `manifest_urls_pin_a_commit_sha_not_a_branch`
  and `manifest_hashes_are_full_sha256_hex` fail the build if anyone
  re-introduces a bare branch ref or a malformed digest.
- **416-restart resumable downloader.** When the upstream file shrinks
  between resume attempts, a stale `.partial` produces `416 Range Not
  Satisfiable`; the downloader wipes `.partial` and restarts from byte
  0, just like the existing `200 OK` ignored-`Range` path. Backed by
  an integration test that hand-rolls a `tokio::net::TcpListener`
  HTTP/1.1 responder so CI runs without external network.
- **FFI generation guard with ABA defence (TRD §1.3, REQ-ARCH-05).**
  `CallbackGuard` is a `#[repr(transparent)] u64`. `Inner` carries an
  `AtomicU64` generation counter *and* a process-monotonic `instance_id`
  assigned at construction time (architect review GH #53). Heap-address
  reuse across release/acquire cycles is detected because the new
  `Inner`'s `instance_id` does not match the snapshot the caller
  captured. `from_ptr(usize)` is `#[deprecated]`; new code uses
  `from_non_null(NonNull<Inner>)` (architect review GH #10). Re-bind is
  `Inner::bump()`; permanent destroy is `Inner::tombstone()`
  (architect review GH #9).
- **Storage contract (TRD §6 / BS §10).** Single `DatabasePool` opened
  via `DatabasePool::open` or `open_with_cipher_key` (SQLCipher gated
  on `feature = "sqlcipher"`). All async DB work routes through
  `PooledConnectionExt::with_conn` which wraps the synchronous
  `rusqlite` closure in `spawn_blocking`. Pool defaults:
  `max_size = 8`, `WAL`, `synchronous = NORMAL`, `foreign_keys = ON`,
  `busy_timeout = 5000`. SQLCipher key bytes are bound via `PRAGMA key`
  inside `with_init` and `Zeroize`d before the closure exits.
- **Migrator (TRD §6.1).** `Migrator::list_available()` parses
  `migrations/V{nnn}__{name}.sql`, computes a SHA-256 over each body,
  and `apply_pending(pool)` runs each pending migration in its own
  transaction. A non-contiguous applied set surfaces
  `MukeiError::MigrationOrderConflict { expected, applied }` so the
  bridge cannot silently DDL around the schema.
- **AgentLoop graceful degrade (TRD §2.3).** `agent/loop_.rs` is the
  single inference caller. Parse failures, validator rejections,
  no-progress backoff, and abuse-blocked tools all surface as
  injected `Role::Tool` envelopes (via `render_tool_error_envelope`
  and `render_supervisor_directive`) instead of hard-aborting the
  turn. Wall-clock containment via `tokio::select!` over the chat
  cancel token AND `WatchdogHandle::remaining_wall_clock()` (architect
  review GH #46 / #47).
- **FailureTracker fingerprints (TRD §2.5).** Failures key on
  `SHA-256(tool_name || 0x00 || canonical_json(args))` with sorted
  JSON keys, so reordering arguments cannot evade the blocker.
  `Cancelled` is benign, `Permanent` / `Abuse` bypass the threshold,
  everything else advances the per-fingerprint counter with default
  `max_failures_per_tool = 5`. `OutputRepeatTracker` flags
  byte-identical tool output for the same fingerprint and injects a
  no-progress notice.
- **Adaptive Search Planner (TRD §5.1).** `SearchEngineKind` is
  closed at `{ Brave, Tavily }` and a compile-time `compile_error!`
  blocks DDG re-introduction. `SearchSelector::select(kind)` is a
  pure matrix: Fact/News/Local/Shopping → `[Brave]`, Research /
  Compare / Academic / MultiStep → `[Tavily, Brave]`. `PlannerPolicy`
  defaults: Brave 3 s, Tavily 5 s, `max_parallel_engines = 2`,
  `hits_per_engine = 5`, `enable_cache = true`. Per-engine timeouts
  surface as empty hit sets so the planner returns whatever the
  faster engine produced; `SourceTrust::Unsafe` is dropped before
  ranking; citations are required in the response builder.
- **Tool validator (TRD §13.3).** `tools::validator::validate` returns
  `Vec<TypedToolCall { id, name, arguments }>` and aggregates
  rejections into a single structured
  `MukeiError::ToolArgsRejected { tool_name: "validator", reason: … }`
  envelope so a mixed batch still re-prompts the LLM cleanly.
  Whitelist: `web_search.query`, `read_file.path` (must be
  `saf://...`), `get_hardware_info` (no args), `math_eval.expression`.
- **Thinking-tag streaming (TRD §1.2.5).** `ffi::tags::TagsStreaming`
  uses a 64-byte sliding window with `truncate_safe` UTF-8 boundary
  asserts; `push()` returns `TagEvents` flags the bridge forwards as
  `thinking_started` / `thinking_completed` qsignals.
- **Engine contract (TRD §3).** `LlamaEngine::load_model` streams the
  full-file SHA-256 in 1 MiB windows through `spawn_blocking` *before*
  mmap whenever `EngineConfig::expected_sha256` is set (REQ-SEC-01).
  Tool-call detection is grammar-aware via
  `crate::tools::validator::parse_gbnf_output`; prose / bare arrays
  never trip the detector. `InferenceOutcome::stop_reason` is typed
  `Completed | UserStopped | ThermalKill | OutOfMemory | WatchdogTripped`.
  Token streaming uses `engine::streaming::Drainer` with a 50 ms /
  1024-byte flush window, so CXX-Qt signals always carry coalesced
  batches instead of per-token chunks.
- **GPU + thermal strategy.** `GpuStrategy::detect()` is side-effect
  free (`/proc/cpuinfo` + Android `build.prop` + macOS `uname -m`).
  `pick_layers_with_thermal()` mirrors Android `PowerManager.ThermalStatus`:
  `>= 3` drops to CPU, `== 2` halves the offload count.
- **Gemma 4 model catalogue (TRD §8.0 / REQ-MOD-01).**
  `engine::model_registry::MODELS` ships exactly the `Gemma4E2bIt`
  (≈3.46 GB Q4_K_M) and `Gemma4E4bIt` (≈5.41 GB Q4_K_M) descriptors
  with commit-pinned Hugging Face URLs, full 64-hex SHA-256 digests,
  RAM thresholds, and recommended `n_ctx` values; QML reads the list
  through `MukeiBridge::model_catalogue_json` and lets the device-tier
  picker (`recommended_model_id`) downgrade to E2B below 7168 MiB.
- **RAG storage hardening.** `vector_store::StoreHeader` records the
  embedder id + dimension; boot refuses any file whose embedder /
  dimension differs from the wired backend (REQ-RAG-01 / -02).
  Persistence is atomic-rename through a `.swap` sibling, invoked only
  from `spawn_blocking`. Two architect-review tripwires fail the build
  if `release-hardening` is enabled without `candle` (GH #15) or
  without `usearch_hnsw` (GH #16).
- **Local-only diagnostics pipeline.** `diagnostics::initialize_tracing()`
  boots with `std::io::sink()` so Android stdout/stderr never leak into
  logcat. The embedder installs a `CrashSink` for app-internal storage,
  and `panic_hook::{install_panic_hook,reinstall_panic_hook}` persists a
  `CrashRecord { fingerprint, location, reason, ts }` JSON file per
  fingerprint while notifying the bridge-facing `PanicSink`. Crash sinks
  resolving to `/sdcard/...`, `/storage/emulated/...`, `/storage/self/...`,
  or `content://media/...` are rejected at open time.
- **Bounded runtime (TRD §2.2).** `MAX_BLOCKING_THREADS = 6` on
  `target_os = "android"`, `8` elsewhere. `TOOL_BLOCKING_SLOTS = 2`. A
  module-level `const _: () = assert!(TOOL_BLOCKING_SLOTS < MAX_BLOCKING_THREADS, …)`
  fails `cargo check` if a future refactor inverts the ordering
  (architect review GH #33).
- **Storage audit-log hash chain.** `AuditLogWriter` serialises every
  append under a writer mutex so the SHA-256 chain that links
  consecutive rows cannot be torn by concurrent writers (codex
  follow-up — fix on this branch).
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
- **PRD REQ-AGT-04** — Tool Execution Policy with configurable threshold (default 5), failure-kind classification (`Transient` / `Validation` / `Cancelled` / `Timeout` / `Permanent` / `Abuse`), structured feedback envelopes, and no-progress detection.
- **TRD §1.2.5** — Thinking-tag streaming detector with 64-byte sliding window, multi-transition per push.
- **TRD §2.4** — Spawn-blocking enforced for every SQLite/disk path (the "Golden Rule").
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
| `network` | `reqwest`-backed model downloader + web search engines. | ⚠️ rustc ≥ 1.86 (icu deps) |
| `candle` | Local MiniLM embedder. | ⚠️ host setup |
| `usearch_hnsw` | usearch-backed vector store. | ⚠️ host setup |
| `llama_cpp` | Real `llama-cpp-rs` binding via the prebuilt static archive. | ❌ needs Qt + prebuilt libllama |
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
│   │   ├── mukei-core/          ← pure-Rust kernel · 248 tests passing on `main` (--all-features)
│   │   ├── mukei-bridge/        ← CXX-Qt + JNI surface (Qt host required)
│   │   └── mukei-ffi-shim/      ← manual extern "C" escape hatch · 3 tests
│   ├── llama-cpp-stub/          ← workspace placeholder (release-hardened)
│   ├── llama-cpp-prebuilt/      ← one-shot libllama.a per ABI (CMake)
│   ├── migrations/              ← V001–V005 · 000_default_config.toml
│   ├── grammars/                ← tool_calling.gbnf (strict per-tool)
│   └── docs/                    ← TRD / PRD / BS / AF / UXB design pass v0.7.5
```

The full design pass — TRD v0.7.5 (≈6,400 lines), PRD v0.7.5, Backend Schema v1.2, Application Flow v1.2, UX Brief v2.1 — lives under [`rust/docs/`](rust/docs/). They are intentionally large; the engineering README in [`rust/README.md`](rust/README.md) is the right entry point for contributors.

---

## Requirements

- **Rust 1.78+** (workspace MSRV). Pinned via `rust-toolchain.toml`.
- **Qt 6.5+** for the `mukei-bridge` build only.
- **SQLite 3.40+** with WAL support (bundled when `rusqlite` feature is on).
- **Android NDK r26+** for on-device builds (target `aarch64-linux-android`).
- A vendored `llama.cpp` checkout under `rust/llama-cpp-prebuilt/vendor/llama.cpp` for real inference builds; the workspace falls back to the release-hardened stub crate otherwise.

---

## Quick start (Linux/macOS sandbox)

```bash
git clone https://github.com/MUKEI-lab/Mukei.git
cd Mukei/rust

# Pure-Rust kernel (no Qt, no llama.cpp)
cargo test -p mukei-core --no-default-features --features "std,tokio"
cargo test -p mukei-ffi-shim

# With SQLite-backed storage
cargo check -p mukei-core --features "tokio,rusqlite"

# With on-device downloader (reqwest) for Gemma 4 GGUF artifacts
cargo check -p mukei-core --features "tokio,network"
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
