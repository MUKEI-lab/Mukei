# Mukei v1.0 QML Solution — Post-Merge Hardening Notes

Base lineage: `Mukei_v1.0_qml_sol`  
Current snapshot: `Mukei_v1.0_qml_sol_plus_hardening_13plans_merged_syntax_fixed`  
Documentation refresh: 2026-07-12

This package preserves the v1.0 Rust/QML merge and adds a broad production-oriented hardening layer. The notes below describe source-observed capabilities in the current archive; they are not a claim that the full native release matrix has passed.

## Source-integrated additions

### Inference activation and failure safety

- Authoritative `ModelActivationState` lifecycle.
- Explicit separation between model presence, verification, activation, and backend readiness.
- Generation-guarded activation commits.
- Stale activation completion cannot overwrite a newer selection.
- Production activation failure remains explicit and does not silently fall back to a mock.
- Product readiness is tied to an active production backend rather than interface/file presence.

### Protocol scope and event reliability

- Protocol V2 command envelopes and immediate acknowledgements.
- Stable command/request/correlation/operation identity.
- Bounded idempotent replay protection and replay-conflict rejection.
- Stable event identity and per-stream sequencing.
- Operation lifecycle projection and scoped cancellation semantics.
- Fail-closed major-version handling.
- Explicit legacy-event compatibility mode rather than pretending equivalent reliability.

### Async bridge, secure bootstrap, and provenance

- Per-domain request generations for non-chat asynchronous results.
- Stale completion protection.
- Explicit SQLCipher bootstrap state machine.
- Android wrapping-key/database-key lifecycle with zeroizing plaintext buffers.
- Provenance separates product, protocol, schema, build, runtime environment, hardening mode, and feature flags.

### Observability, privacy, memory, and SLO foundation

- Bounded event queue and metric cardinality.
- Privacy/sensitivity classification.
- Privacy-epoch invalidation of queued data.
- Local-first, sink-neutral recorder model.
- Health registry and SLO state.
- No observability-owned network exporter is created implicitly.

### RAG scope, context budget, and capability truthfulness

- Explicit tenant/workspace retrieval scope.
- Scope metadata in indexing/vector search.
- Resolver-side scope validation.
- Index compatibility state.
- Structured RAG capability snapshots and degraded reasons.
- Explicit result/context budgeting.
- Legacy unscoped adapters treated conservatively.

### SaaS domain, persistence, and transport foundations

- Registered V013 migration for tenant/workspace/actor/membership state.
- Versioned subscription, entitlement, and quota snapshots/policies.
- Append-only usage ledger with idempotency and correction rules.
- Local deterministic installation scope.
- Provider-neutral entitlement/quota decisions that fail closed on stale authority.
- Generic SaaS transport boundary with endpoint validation, auth injection, retries, concurrency limits, cancellation, and circuit state.
- No claim of complete identity, server authorization, billing, or QML SaaS product integration.

### Remote feature privacy policy

- Default `local_only` policy.
- Explicit `ask_before_remote`, `remote_allowed`, and `enterprise_disabled` states.
- Compiled network capability is not treated as permission to use a remote service.

## Preserved earlier v1.0 merge hardening

- Typed `AgentRunRequest` and request-scope preservation.
- Recovery single-use/claim semantics.
- Conversation/branch history isolation.
- FFI generation/instance guarding.
- Durable storage, migrations, model download state, QML scoped projections, document permissions/ingestion projection, accessibility, and responsive shell work.

## Validation status

The current archive contains 467 Rust test annotations and 23 QML behavioural test files, but those are inventory counts rather than pass counts.

This documentation refresh did not have Cargo/Rust or Qt available, so no new native validation is claimed.

The lockfile still contains `crossbeam-epoch 0.9.18` and `cxx 1.0.194`; the latest recorded project security run flagged those versions for upgrade. Dependency remediation and re-running `cargo-audit`/`cargo-deny` remain release gates.

See `docs/CURRENT_IMPLEMENTATION_STATUS.md` for the current release-gate list.
