# Mukei Rust Core

Pure-Rust kernel for the Mukei on-device AI agent.

This workspace implements [TRD v0.7.5](docs/TRD.md), the
[Backend Schema v1.2](docs/BS.md), and the
[Application Flow v1.2](docs/AF.md).

See also the [PRD v0.7.5](docs/PRD.md) and [UX Brief v2.1](docs/UXB.md).

## Crates

| Crate | Role |
|---|---|
| `mukei-core` | Pure-Rust agent / engine / RAG / storage / diagnostics kernel. Never links Qt — testable on any host. |
| `mukei-bridge` | CXX-Qt bridge exposing `MukeiAgent`, `MukeiBridge`, `SafRegistry` + JNI helpers (Qt build only). Owns the `BusyGuard` re-entrancy guard for `send_message` and the per-destination `DownloadSlotGuard` registry for `download_model`. |
| `mukei-ffi-shim` | Manual `extern "C"` escape-hatch FFI. Hand-maintained `include/mukei_ffi_shim.h` is the canonical C ABI (drift-detector test enforces parity with `extern "C"` exports in `src/lib.rs`). |
| `llama-cpp-stub` | Workspace placeholder while the `llama-cpp-prebuilt/CMakeLists.txt` one-shot static archive is the production link target. Has a release-hardening tripwire (`compile_error!`) so it cannot ship in a release build with the `llama_cpp` feature on. |

## Sandbox build (no Qt, no SQLite, no candle)

```bash
cd rust
cargo check -p mukei-core --no-default-features --features "std,tokio"
cargo test  -p mukei-core --no-default-features --features "std,tokio"
cargo check -p mukei-ffi-shim
cargo test  -p mukei-ffi-shim
```

## Full build (with SQLite + RAG + on-device downloader)

```bash
cd rust
cargo check -p mukei-core --features "tokio,rusqlite"
cargo check -p mukei-core --features "tokio,rusqlite,candle"
cargo check -p mukei-core --features "tokio,network"   # enables real reqwest model downloader
```

> Note: `usearch`, `candle`, and `llama-cpp-rs` need per-target setup.
> `llama-cpp-prebuilt/CMakeLists.txt` produces a one-shot `libllama.a` per ABI.

## Test coverage

Mirror of `.github/workflows/sandbox-check.yml` — the CI gate runs this
exact set on every push to the codex review branch and to `main`.

```
mukei-core  (std,tokio)     203 unit + 12 integration + 6 proptest + 3 grammar
                            + 4 sentinel proptest        ──► 228 passed
mukei-ffi-shim                                                  3 passed
                                                          ──────────────
                                                          231 passed total
```

When the `rusqlite` feature is enabled the lib-test count rises to 217
(SQLite-only suites unlock). The `network` feature additionally enables
the 416-restart loopback integration test
(`http_416_on_resume_triggers_restart_and_succeeds`). The
`sandbox-check.yml` workflow runs the `(std,tokio)` matrix only.

Documentation index (under `docs/`):

- [TRD v0.7.5](docs/TRD.md) — Technical Reference Document (build-this-exactly)
- [PRD v0.7.5](docs/PRD.md) — Product Requirements Document
- [Backend Schema v1.2](docs/BS.md) — SQL schema, migrations, retention policy
- [Application Flow v1.2](docs/AF.md) — Boot, model acquisition, tool pipeline
- [UX Brief v2.1](docs/UXB.md) — Editorial-luxury design system

### Verified invariants

- **Runtime (TRD §2.2).** `MAX_BLOCKING_THREADS = 6` on
  `target_os = "android"`, `8` on every other target. `TOOL_BLOCKING_SLOTS = 2`.
  A `const _: () = assert!(TOOL_BLOCKING_SLOTS < MAX_BLOCKING_THREADS, …)`
  in `crate::runtime` makes any future refactor that violates the ordering
  fail `cargo check`, not just CI (architect review GH #33).
- **FFI generation guard (TRD §1.3, REQ-ARCH-05).** `CallbackGuard` is a
  `#[repr(transparent)] u64`. `Inner` holds an `AtomicU64` generation
  counter and a process-monotonic `instance_id` (architect review GH #53
  ABA defence). The legacy `from_ptr(usize)` constructor is
  `#[deprecated]`; new code uses `from_non_null(NonNull<Inner>)`
  (architect review GH #10). Re-bind path is `Inner::bump()`; permanent
  destroy is `Inner::tombstone()`; the previous blanket `invalidate()`
  is retained as a deprecated alias (architect review GH #9).
- **Re-entrancy guards (bridge crate).**
  - `send_message` holds an `AtomicBool` flipped on entry; a second
    call returns `MukeiError::BridgeBusy` (`ERR_BRIDGE_BUSY`). Release
    is RAII via `BusyGuard::Drop` so a panic anywhere in the streaming
    task still clears the flag (`panic = "unwind"` is a workspace
    invariant — see §1.3 / PRD G1).
  - `download_model` holds a per-destination slot in a global
    `Arc<Mutex<HashSet<PathBuf>>>`. Two downloads of *different*
    models run in parallel; two downloads of the *same* dest fail
    fast with `MukeiError::DownloadBusy { dest }`
    (`ERR_DOWNLOAD_BUSY`). Release is RAII via `DownloadSlotGuard::Drop`.
- **Separate cancellation tokens.** `MukeiAgent` carries two independent
  `CancellationToken`s — `cancel_token` (chat) and `download_cancel`
  (downloads). `stop_generation()` rotates only the chat token;
  `stop_download()` rotates only the download token. The chat Stop
  button no longer silently kills a model download.
- **Adaptive Search Planner (TRD §5.1).** `SearchEngineKind` is the
  closed set `{ Brave, Tavily }`; a compile-time `compile_error!` in
  `search/engines/mod.rs` rejects DDG re-introduction. `SearchSelector`
  is pure: Fact/News/Local/Shopping → `[Brave]`,
  Research/Compare/Academic/MultiStep → `[Tavily, Brave]`. Defaults
  follow `PlannerPolicy` — Brave 3 s, Tavily 5 s,
  `max_parallel_engines = 2`, `hits_per_engine = 5`,
  `enable_cache = true`. A per-engine timeout surfaces as an empty hit
  set, not an error, so the planner keeps whatever the faster engine
  produced. Trust-gated sources (`SourceTrust::Unsafe`) are dropped
  before ranking; citations are enforced in the response builder.
- **Tool validator (TRD §13.3).** `tools::validator::validate` returns
  `Vec<TypedToolCall { id: ToolCallId, name, arguments }>` and rolls
  rejections into `MukeiError::ToolArgsRejected { tool_name: "validator",
  reason: format_for_llm(errors) }`. Whitelist:
  `web_search.query`, `read_file.path` (must be `saf://...`),
  `get_hardware_info` (no args), `math_eval.expression`. The agent
  executor never sees raw LLM JSON.
- **Thinking-tag detector (TRD §1.2.5).** `ffi::tags::TagsStreaming`
  uses a `TAG_WINDOW = 64`-byte sliding window with `truncate_safe`
  (asserts UTF-8 char boundaries) so multi-byte localised tags never
  trip the detector. `push()` returns a `TagEvents` bitflag the bridge
  forwards as `thinking_started` / `thinking_completed` qsignals.
- **Engine contract (TRD §3).** `LlamaEngine::load_model` streams the
  full-file SHA-256 in 1 MiB windows through `spawn_blocking` *before*
  mmap whenever `EngineConfig::expected_sha256` is set (REQ-SEC-01).
  `contains_tool_call` is GBNF-aware and only delegates to the
  streaming-prefix heuristic for partial JSON. `InferenceOutcome::stop_reason`
  is the typed `StopReason` enum; the bridge / agent loop read the
  stop tag verbatim.
- **Token streaming.** `engine::streaming::Drainer` coalesces upstream
  `mpsc::Receiver<String>` tokens into 50 ms / 1024-byte batches
  before they reach the bridge’s `chunk_generated` signal.
- **GPU strategy.** `GpuStrategy::detect()` reads `/proc/cpuinfo`
  (Linux/Android) + `/system/build.prop` and `uname -m` on macOS;
  `pick_layers_with_thermal()` follows the Android `ThermalStatus`
  enum (>= 3 → CPU; == 2 → halve layers).
- **Gemma 4 catalogue (TRD §8.0 / REQ-MOD-01).** `engine::model_registry`
  exposes two descriptors (`Gemma4E2bIt` / `Gemma4E4bIt`) with
  commit-pinned URLs, full SHA-256 digests, RAM minimums, and
  recommended `n_ctx`. The bridge surfaces them via
  `model_catalogue_json` / `recommended_model_id`; downloads write to
  `<dest>.partial` and atomically rename only after the full-file
  hash matches.
- **RAG storage hardening.** `StoreHeader` carries
  `format_version + embedder_id + embedding_dim`; the
  `release-hardening` feature requires both `candle` and `usearch_hnsw`
  (architect-review tripwires GH #15 / #16) so a release build can
  never silently fall back to the mock embedder or the flat-scan
  backend.
- **Silent bootstrap tracing.** `diagnostics::logger::initialize_tracing`
  uses `std::io::sink()` until the embedder installs its own file-backed
  sink, preventing privacy leaks into Android logcat during early boot.
- **Thinking-tag streaming (TRD §1.2.5).** `TagsStreaming` is a 64-byte
  sliding-window detector with multi-transition support (open → close →
  open in the same chunk).
- **Vector store atomic-rename save (TRD §4.4).** Never blocks the async
  runtime.
- **Sandboxed `math_eval` (TRD §5.5).** Identifier whitelist + 8 s timeout
  with `JoinHandle::abort` on overrun so the tool semaphore is reaped.
- **Strict TOML config (TRD §12.5).** Unknown root keys are rejected on
  boot.
- **Post-parse tool validator + SAF (TRD §5.2 / §13.3).** `read_file`
  refuses every non-SAF URI scheme.
- **Crash diagnostics sink (TRD §36.1).** The panic hook computes a
  stable SHA-256 fingerprint from `location || 0x00 || reason`, writes a
  `CrashRecord` JSON file into the installed app-internal crash
  directory, and can be reclaimed with `reinstall_panic_hook()` if a
  host framework overwrites `std::panic::set_hook`.
- **Model integrity (REQ-SEC-01).** Every GGUF artifact is streamed
  through a SHA-256 hasher in 1 MiB windows *before* `mmap` whenever
  `EngineConfig::expected_sha256` is set; the model registry pins both
  the upstream commit-sha and the digest. See
  [`crate::engine::model_registry`] and `LlamaEngine::verify_full_sha256_stream`.
- **Typed stop reasons (TRD §3.0).** `InferenceOutcome { assistant_text,
  used_tokens, stop_reason }` exposes `StopReason::{Completed,
  UserStopped, ThermalKill, OutOfMemory, WatchdogTripped}` so the
  bridge picks the right UI chip without parsing free-form strings.
- **Token-stream drainer (TRD §3.0 / engine::streaming).** Tokens flow
  through `Drainer` with a 50 ms / 1 KiB `TokenStreamConfig` so the
  bridge emits coalesced batches; the inference worker never touches a
  CXX-Qt signal directly.
- **Thermal-aware GPU strategy (TRD §3.2 / engine::gpu_strategy).**
  `GpuStrategy::pick_layers_with_thermal()` halves offload at Android
  thermal level 2 and drops to CPU at level ≥ 3. `GpuKind` covers
  `Mali | Adreno | Sugarloaf | CpuOnly | Unknown` with stable ASCII
  tags.
- **RAG embedder + vector store (TRD §4).** `Embedder` impls always
  return L2-normalised vectors; `StoreHeader` carries `format_version`
  + `embedder_id` + `embedding_dim` and boot refuses any persisted
  file that disagrees with the wired embedder. A `release-hardening`
  build without `candle` or without `usearch_hnsw` is a compile-time
  error so production cannot silently ship the mock embedder or the
  flat-scan backend (architect-review GH #15 / #16).
- **Indexer transactional safety (TRD §4.3).**
  `rag::indexer::IndexingTransaction` wraps SQL inserts AND the
  vector-store snapshot in a single SQLite write transaction; `Drop`
  without an explicit `commit()` rolls back staged vectors so a
  mid-flight SAF revoke cannot leave orphan rows.
- **Downloader 416-restart.** If the upstream file shrinks between
  resume attempts a stale `.partial` produces a `416 Range Not
  Satisfiable`; the downloader wipes `.partial` and restarts from byte
  0 (`http_416_on_resume_triggers_restart_and_succeeds` covers it).

## Directory layout

```
rust/
├── Cargo.toml                         (panic = "unwind" pinned, MSRV 1.78)
├── crates/
│   ├── mukei-core/        (lib)       (pure-Rust, no Qt)
│   ├── mukei-bridge/      (cdylib + staticlib)  (CXX-Qt + JNI; Qt host only)
│   └── mukei-ffi-shim/    (staticlib) (manual extern "C" escape hatch)
├── llama-cpp-stub/        (lib)       (release-hardened placeholder)
├── llama-cpp-prebuilt/                (one-shot libllama.a per ABI, CMake)
├── migrations/
│   ├── 000_default_config.toml
│   ├── V001__schema.sql           (conversations, messages, chunks)
│   ├── V002__recovery_state.sql   (crash-safe stream resume)
│   ├── V003__tooling_and_saf.sql  (audit log + SAF token table)
│   └── V004__branching.sql        (branch graph)
├── grammars/
│   └── tool_calling.gbnf
└── docs/
    ├── PRD.md   (v0.7.5)
    ├── TRD.md   (v0.7.5)
    ├── BS.md    (v1.2)
    ├── AF.md    (v1.2)
    └── UXB.md   (v2.1)
```

## Error taxonomy (excerpt)

`MukeiError` is the single enum crossing the FFI boundary; every
variant maps to a stable `ERR_*` code that QML can localise.

| Category | Variants (codes) |
|---|---|
| FFI / Bridge | `FFIPanic` (`ERR_FFI_PANIC`), `CallbackGuardExpired` (`ERR_CALLBACK_GUARD_EXPIRED`), `BlockingJoinFailed` (`ERR_BLOCKING_JOIN`), `BridgeBusy` (`ERR_BRIDGE_BUSY`), `DownloadBusy { dest }` (`ERR_DOWNLOAD_BUSY`) |
| Resource | `OOM`, `MemoryPreflightRejected`, `ThermalThrottle` |
| Inference | `ModelLoadFailed`, `ModelCorrupted`, `ContextCreationFailed`, `ContextOverflow`, `GrammarLoadFailed` |
| Storage | `DatabaseInitFailed`, `DatabaseCorruption`, `MigrationFailed`, `MigrationOrderConflict` |
| Config | `ConfigMissingField`, `ConfigInvalid { field, reason }`, `ConfigUnknownField`, `SafeStorageUnavailable` |
| Crypto | `WrappedKeyMalformed`, `UnwrapFailed`, `SecretLeaked(redacted_len)` |
| Agent | `ToolLoopDetected`, `ToolTimeout`, `UnknownTool`, `ToolArgsRejected`, `ToolAbuseBlocked`, `ToolPermanentlyDisabled`, `ToolParseFailed`, `ToolArgumentInvalid`, `ToolExecutionFailed`, `WebSearchFailed`, `HttpClientFailed`, `FileReadFailed`, `BinaryFile`, `SandboxViolation` |
| Permission | `PermissionDenied`, `SafRevoked`, `SafRequired` |
| Network | `NetworkError`, `Io`, `DownloadHashMismatch` |
| Domain | `PromptLeakage`, `WatchdogExceeded { kind }`, `CrashLoopDetected { fingerprint }`, `Cancelled`, `Invariant`, `Internal` |

Every variant is mapped to an `ErrorClass` bucket
(`Resource | Device | Inference | Storage | Config | Agent | Permission | Network | Security | Unknown`)
for telemetry-free FMEA tracking (§36.1). The match in
`MukeiError::classification` is exhaustive — adding a new variant
without choosing a class fails `cargo build` via E0004.

## Doc-comment policy

`crate::lib.rs` carries `#![warn(missing_docs)]`. Three modules enforce
it strictly: `error`, `ffi`, `guard`. Every other module is `#[allow(missing_docs)]`
pending the per-item documentation sweep, but **adding a new module
without doc-comments on every `pub` item is disallowed** — only the
existing modules are grandfathered.
