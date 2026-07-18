# ADR-005 — Authoritative State and Process-Death Recovery

Status: **Proposed**  
Priority: **Critical before Conversation persistence/recovery**

## Context

Android process death can destroy Activities, ViewModels, Kotlin singletons, and the native runtime process state while durable SQLCipher/storage data remains.

The current Rust runtime persists projections for operations, models, documents, and conversations. On hydration, operations previously marked Accepted/Running are converted to failed/interrupted state.

The product also needs UI drafts/navigation restoration without letting stale Compose/Kotlin state override durable truth.

## Proposed decision

Use a layered authority model:

```text
1. Durable Rust/SQLCipher domain projection
2. Current native runtime snapshot/session state
3. Repository reconciliation projection in Kotlin
4. SavedState/UI draft/navigation state
5. Last rendered Compose state (never authoritative)
```

Recovery order:

```text
Process recreated
  ↓
restore safe UI-only draft/navigation hints
  ↓
start/reconnect secure runtime
  ↓
load durable domain projections
  ↓
load current runtime snapshots/capabilities
  ↓
repository reconciles active/transient state
  ↓
render truthful UiState
```

## Rules

- SavedState MUST NOT be authoritative for messages/files/artifacts/model installation.
- A previously rendered `Running` state must not remain Running without an authoritative active operation.
- Runtime session ID changes invalidate stale transient event assumptions.
- Event sequence gaps trigger authoritative snapshot/query reconciliation.
- User-authored unsent drafts may be restored independently from backend state.

## Alternatives considered

### A. Kotlin/ViewModel persistence is authoritative

Rejected: vulnerable to stale/divergent native/durable state.

### B. Rust runtime snapshot only

Insufficient: runtime may be gone; durable SQLCipher data and UI drafts have different lifecycles.

### C. Layered reconciliation — proposed

Matches Android lifecycle and durable/runtime separation.

## Consequences

- repositories must implement reconciliation, not only event collection;
- public bounded conversation/model/storage snapshots/queries are required;
- operation records need stable interrupted/recovery semantics;
- UI state models include `Recovering` where reconciliation is user-visible.

## Security / privacy impact

SavedState should avoid storing unnecessary sensitive full document/chat content. Persist durable sensitive data through encrypted domain stores.

## Product / UX impact

After crash/process death the user sees what is actually saved and what was interrupted, not stale spinners or fabricated progress.
