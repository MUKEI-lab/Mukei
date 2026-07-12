# Mukei Security and Privacy Engineering

This document describes the security posture of the current source snapshot. It
is not a release certification.

## Current security status

**Release status: not security-cleared.**

The checked-in `rust/Cargo.lock` currently contains:

- `crossbeam-epoch 0.9.18`;
- `cxx 1.0.194`.

The latest recorded project dependency-security run flagged those versions and
required upgrades to at least `crossbeam-epoch 0.9.20` and `cxx 1.0.195`.

`cxx` must be resolved with the CXX-Qt dependency graph rather than blindly
forced to a top-level version. Re-run `cargo-audit` and `cargo-deny` after the
targeted dependency resolution.

The archive currently contains only `.github/workflows/ci.yml`; it does not
contain a dedicated automated `cargo-audit`/`cargo-deny` workflow.

## Security model

Mukei is local-first, not “network impossible”.

The intended default posture is:

- local storage and local inference path;
- no default remote observability export;
- explicit policy before remote-capable features are used;
- strict separation between compiled capability and permission to exercise it;
- fail-closed behavior when authority, scope, protocol, or entitlement state is
  stale or untrusted.

Optional network features exist for model download, web search, and future SaaS
transport. Those surfaces must remain explicit and policy-controlled.

## 1. FFI and bridge safety

The project uses two native boundaries:

- `mukei-bridge`: CXX-Qt/JNI integration;
- `mukei-ffi-shim`: manual C ABI fallback.

Safety controls include:

- `panic = "unwind"` in shipped Rust profiles so `catch_unwind` boundaries can
  contain panics;
- generation/instance guarding for callback lifetime and ABA defense;
- explicit unsafe FFI entry points and safety contracts;
- ABI drift checks for the manual FFI header;
- re-entrancy guards around chat and per-destination download operations.

These controls reduce risk but still require native compilation and runtime
testing on the current snapshot.

## 2. Secure database bootstrap

Android production-oriented storage uses a wrapping-key pattern:

1. a non-exportable Android Keystore key protects wrapped database-key material;
2. Rust receives plaintext database-key bytes only long enough to open
   SQLCipher;
3. plaintext key buffers use zeroizing memory;
4. key material is not serialized, logged, or exposed to QML.

Bootstrap state distinguishes first-install creation, unwrap, database open, key
invalidation, wrapped-key corruption, and reset-required states.

Plain `DatabasePool::open()` remains a deliberate non-cipher development path
when the `sqlcipher` feature is omitted. It must not be mistaken for the Android
production posture.

## 3. Model integrity and activation truthfulness

Model catalogue entries carry commit-pinned artifact URLs and expected SHA-256
digests. Model bytes are verified before the engine treats an artifact as
loadable.

The activation layer separately represents:

- missing;
- verifying;
- verified;
- activating;
- ready;
- activation failed;
- deactivating.

A downloaded or verified model is not automatically “ready”. Product readiness
requires an active production backend. Production activation failure does not
silently fall back to a mock backend.

## 4. Protocol and event reliability

Protocol V2 is fail-closed across unknown major versions and validates bounded
command envelopes before execution.

Security/reliability properties include:

- bounded command and identifier sizes;
- typed command registry and payload validation;
- scoped command preflight;
- bounded idempotent replay protection;
- stable event identity;
- per-stream sequence tracking;
- operation correlation;
- stale/duplicate event rejection;
- explicit separation of accepted, running, completed, failed, cancelled, and
  rejected states.

The desktop compatibility event mode is isolated and explicitly negotiated; it
must not be represented as equivalent to the production V2 event contract.

## 5. Input validation and tool isolation

Tool calls are parsed into typed calls before execution.

Important controls include:

- post-parse argument validation;
- rejection of unknown/extra/wrongly typed arguments;
- SAF-only file references for `read_file`;
- no raw filesystem path authority through the tool contract;
- sandboxed math expression evaluation;
- failure fingerprints and loop/abuse containment;
- timeout/cancellation containment.

Fuzzing and property-based tests are useful defense layers, but the presence of a
harness is not evidence that a continuous fuzz campaign has run on this exact
snapshot.

## 6. RAG scope and context safety

Current RAG code carries explicit tenant/workspace retrieval scope.

Controls include:

- scope metadata on indexed vectors;
- scoped vector search;
- resolver-side scope revalidation;
- index compatibility checks;
- explicit degraded capability states;
- bounded result and context budgets.

Legacy unscoped adapters are conservatively degraded and do not prove
multi-tenant authorization safety.

## 7. Observability privacy

The observability subsystem is local-first and sink-neutral.

It provides:

- bounded event queues;
- bounded metric series/cardinality;
- sensitivity classification;
- privacy policy and privacy modes;
- privacy-epoch invalidation of already queued data;
- health and SLO state;
- failure-isolated sinks.

The module does not create a network exporter by itself. A compiled diagnostics
export capability must not be equated with user consent or active remote export.

## 8. Remote feature policy

`RemoteFeaturePolicy` defaults to `local_only`.

Supported policy states are:

- `local_only`;
- `ask_before_remote`;
- `remote_allowed`;
- `enterprise_disabled`.

Remote-capable features must pass policy rather than inferring permission from
network availability.

## 9. SaaS foundation security boundaries

The current source contains provider-neutral tenant, workspace, actor,
membership, subscription, entitlement, usage-ledger, and quota primitives.

Important invariants include:

- opaque validated public identifiers;
- tenant/workspace relational consistency;
- immutable historical snapshots with revision ordering;
- stale/untrusted entitlement state fails closed;
- append-only usage accounting;
- idempotency constraints;
- bounded scalar metadata;
- separate correction events rather than mutation of prior usage.

A hardened transport boundary exists, but this snapshot does not claim complete
production identity, server authorization, billing integration, or cloud
operations.

## 10. Dependency security

Run from `rust/`:

```bash
cargo audit
cargo deny check advisories sources licenses bans
```

Treat failures as release blockers unless an advisory is explicitly evaluated,
documented, and accepted through a controlled risk process.

Duplicate `windows-sys` or `windows-targets` versions are not automatically
vulnerabilities. Investigate them when they contribute to an advisory, material
binary impact, or safely resolvable dependency fragmentation.

## 11. Compiler and linker hardening

The Cargo profile itself is stable-Cargo compatible:

```toml
[profile.release-hardening]
inherits = "release"
lto = "fat"
codegen-units = 1
panic = "unwind"
strip = "symbols"
opt-level = 3
```

Android-specific hardening flags live in `rust/.cargo/config.toml`, not in the
profile:

```toml
[target.'cfg(all(target_arch = "aarch64", target_os = "android"))']
rustflags = [
    "-C", "target-feature=+stack-protector-all",
    "-C", "link-arg=-Wl,-z,relro,-z,now",
]
```

This avoids the unstable `profile-rustflags` Cargo feature while applying the
flags to the Android aarch64 build target.

## 12. CI truth

The current `.github/workflows/ci.yml` covers:

- Rust formatting;
- narrow `mukei-core` Clippy for `std,tokio`;
- narrow `mukei-core` unit tests for `std,tokio`;
- QML architecture, cross-language contract, and QML security analyzers.

It does not currently certify:

- all features;
- CXX-Qt bridge compilation;
- Qt Quick tests;
- Android JNI/Gradle packaging;
- cargo-audit;
- cargo-deny;
- per-ABI llama.cpp linkage;
- physical-device behavior.

## 13. Security validation checklist

Before release certification:

- [ ] Resolve the recorded `crossbeam-epoch` and `cxx` advisories compatibly.
- [ ] Run `cargo-audit` and `cargo-deny` on the resolved graph.
- [ ] Run full Cargo check/Clippy/test matrices.
- [ ] Apply V001–V013 to a fresh database and test upgrade paths.
- [ ] Build CXX-Qt/Qt 6.5+ surfaces.
- [ ] Run QML QuickTest/CTest and static guards.
- [ ] Build Android JNI/Gradle artifacts.
- [ ] Verify SQLCipher bootstrap and Keystore invalidation/reset behavior.
- [ ] Verify real llama.cpp activation for each shipped ABI.
- [ ] Exercise cancellation, OOM/low-memory, lifecycle recovery, SAF revoke,
      download resume, and protocol resynchronization on physical devices.
- [ ] Confirm remote features remain policy-gated and observability export is
      consistent with the configured privacy policy.

## Reporting vulnerabilities

Use the repository's private vulnerability reporting mechanism when available.
Include the affected revision, reproduction conditions, impact, and any relevant
logs with secrets and user content removed.
