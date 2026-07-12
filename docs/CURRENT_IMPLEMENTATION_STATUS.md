# Current Implementation Status

**Snapshot:** `Mukei_v1.0_qml_sol_plus_hardening_13plans_merged_syntax_fixed`  
**Documentation refresh:** 2026-07-12  
**State:** source-integrated hardening snapshot; native release validation still required.

This document is the implementation-facing status page for the current merged source tree. Historical PRD/TRD/phase/patch documents remain useful design and provenance records, but they do not by themselves certify that the current archive has compiled, passed every test matrix, or shipped on Android.

## Current architecture

The primary runtime boundary remains:

`Qt 6 + QML` → `mukei-bridge` (CXX-Qt/JNI) → `mukei-core` (pure Rust) → storage, inference, RAG, tools, diagnostics, and optional network integrations.

A separate `mukei-ffi-shim` remains the manual C-ABI fallback. The workspace also contains `llama-cpp-stub`, which is a guarded development/build placeholder rather than a production inference backend.

## Source-integrated hardening

### 1. Truthful model activation and failure safety

`mukei-core::engine::activation` is now the authoritative activation owner.

It distinguishes:

- model selected but missing;
- model verification in progress;
- verified artifact;
- activation in progress;
- active backend ready;
- explicit activation failure;
- deactivation.

Readiness is no longer inferred from “model file exists” or “interface exists”. `product_ready` requires an actually active production backend. Activation is generation-guarded so an older activation cannot overwrite a newer model selection, and a production activation failure does not silently fall back to a mock backend.

### 2. Command/event Protocol V2

The current UI protocol is version `2.0`.

The source includes:

- bounded command envelopes;
- immediate accepted/rejected acknowledgements;
- stable command/request/correlation/operation identities;
- bounded idempotent replay protection;
- globally unique event identities;
- monotonic sequencing per logical stream;
- operation lifecycle projection;
- scoped chat cancellation;
- fail-closed handling for unsupported protocol majors.

Production bridge events are wrapped in V2 envelopes. The standalone desktop compatibility implementation may remain in an explicitly negotiated legacy-event mode; that mode is not presented as equivalent to V2 reliability.

See [PROTOCOL_V2_ARCHITECTURE.md](PROTOCOL_V2_ARCHITECTURE.md).

### 3. Non-chat asynchronous bridge boundary

`mukei-bridge::async_bridge` tracks non-chat asynchronous requests by domain and generation.

SQLite/filesystem-backed recovery, UI-session/draft, download, settings, storage, and private-document work can return an accepted request immediately and complete later. A stale completion cannot replace a newer last-known-good projection.

### 4. Secure database bootstrap and provenance

`mukei-bridge::bootstrap` models explicit secure-bootstrap states for wrapping-key creation, database-key creation/wrapping/unwrapping, database open, key invalidation, wrapped-key corruption, and reset-required failure.

Database key material is held in zeroizing memory and is not serialized or sent to QML.

`mukei-bridge::provenance` keeps these concepts distinct:

- product version;
- protocol version;
- database schema version;
- build identifier;
- compiler profile;
- runtime environment mode;
- hardening mode;
- feature flags.

This prevents one version or build label from being incorrectly used as proof of another compatibility boundary.

### 5. Privacy-bounded local observability

`mukei-core::diagnostics::observability` provides a local-first, sink-neutral observability foundation with:

- bounded event queues;
- bounded metric cardinality;
- health signals;
- SLO state;
- explicit field-sensitivity classification;
- privacy modes and telemetry policy;
- privacy-epoch invalidation of already queued data;
- failure-isolated sink handling.

The module does not create its own network client or remote exporter. Default policy does not imply remote telemetry.

### 6. Scope-safe, budget-aware RAG

RAG retrieval now carries explicit tenant/workspace scope and context-budget policy.

The source includes:

- scoped vector indexing/search;
- resolver-side scope revalidation;
- structured retrieval capability snapshots;
- persisted index compatibility checks;
- explicit degraded reasons;
- result count/byte budget enforcement;
- context assembly with an explicit model-context budget.

Legacy unscoped compatibility adapters are treated conservatively and do not prove multi-tenant scope safety.

### 7. Local-first SaaS domain and persistence foundation

Migration `V013__saas_tenancy_entitlements_usage_ledger.sql` is registered in the migrator and adds provider-neutral tables for:

- tenants;
- workspaces;
- actors;
- workspace memberships;
- subscription snapshots;
- entitlement snapshots/grants;
- append-only usage events;
- versioned quota policies.

The Rust domain and repository layers implement revision ordering, idempotency, history retention, stale marking, entitlement/subscription fail-closed decisions, usage-ledger correction rules, and quota decisions.

A production-oriented SaaS HTTP transport boundary also exists under `mukei-core::network::saas`, with endpoint validation, authentication injection, bounded retries/concurrency, circuit state, cancellation, and common JSON envelope handling.

This is **not** a complete SaaS product. The current snapshot does not claim finished identity-provider integration, server-side authorization, billing-provider integration, production cloud endpoints, fleet operations, or QML SaaS product flows.

See [../rust/docs/SAAS_FOUNDATION.md](../rust/docs/SAAS_FOUNDATION.md).

### 8. Remote feature privacy policy

Remote features are governed by `RemoteFeaturePolicy`.

The default is `local_only`; other explicit states are `ask_before_remote`, `remote_allowed`, and `enterprise_disabled`. Remote-capable code must not equate “network feature compiled” with “remote use permitted”.

## Database schema

The migrator currently registers **V001 through V013**.

The last migration adds the SaaS tenancy/entitlement/usage/quota foundation. Historical validation proving V001–V012 on a fresh SQLite database predates V013 and therefore does not validate the full current migration chain.

## Test and validation evidence

Source inventory in this archive:

- **467** Rust `#[test]` / `#[tokio::test]` annotations under `rust/crates/`;
- **23** QML `tst_*.qml` files;
- Protocol V2 integration tests and hardening-specific unit tests are present.

These numbers are **test inventory, not pass counts**.

This documentation refresh did not have a Rust/Cargo or Qt toolchain available, so no new Cargo, Clippy, Qt, JNI, Gradle, or physical-device pass is claimed.

Historical logs in `reports/` remain evidence for the older source state only.

## Current CI truth

The archive currently contains one workflow: `.github/workflows/ci.yml`.

It covers:

- `cargo fmt --check`;
- narrow `mukei-core` Clippy (`std,tokio`);
- narrow `mukei-core` unit tests (`std,tokio`);
- QML architecture, contract, and security analyzers.

It does **not** currently provide a full bridge/Qt build, all-feature Cargo matrix, Android build, `cargo-audit`, or `cargo-deny` workflow.

## Known dependency-security release gate

The checked-in lockfile contains:

- `crossbeam-epoch 0.9.18`;
- `cxx 1.0.194`.

The latest recorded dependency-security run for this project reported those versions as vulnerable and required upgrades to at least `crossbeam-epoch 0.9.20` and `cxx 1.0.195`.

Do not force-upgrade `cxx` blindly: it participates in the CXX-Qt dependency graph and must be resolved compatibly. Dependency-tree inspection, a targeted lock/manifest update, and re-running both `cargo-audit` and `cargo-deny` remain release gates.

## Release gates

The current source should not be described as release-certified until at least:

1. `cargo fmt --all -- --check`;
2. `cargo check --workspace`;
3. required feature-matrix checks;
4. Clippy with warnings denied for core and bridge;
5. core and FFI-shim tests;
6. `cargo-audit` and `cargo-deny`;
7. fresh V001–V013 migration application against a clean database;
8. Qt 6.5+ configure/build, `qmllint`, QuickTest, and CTest;
9. CXX-Qt/JNI/Gradle Android build;
10. real per-ABI llama.cpp linkage and model activation;
11. physical-device validation for SQLCipher bootstrap, lifecycle recovery, SAF, downloads, low-memory behavior, accessibility, and cancellation.

## Verdict

The current archive is a substantially hardened, source-integrated implementation snapshot. It contains important production-oriented foundations, but it is still a **NO-GO for release certification** until native validation and the unresolved dependency-security gates pass.
