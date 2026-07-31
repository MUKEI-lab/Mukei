# Android Product Implementation Status

Status: **Living, non-normative ledger**

This file records execution state against the normative product specification. It does not override the `00`–`10` specification documents or accepted ADRs.

Last synchronized implementation baseline: `Kotlin` commit `061ea5b943a45de55f1338e8278c51ab9f7023e5`.

## Status vocabulary

- **Merged** — integrated into `Kotlin` and validated by the applicable CI gate.
- **In review** — implemented on a focused branch/PR but not yet integrated.
- **Planned** — specified but no integration-ready implementation exists.
- **Blocked** — intentionally waiting for an ADR or prerequisite contract.

---

## M0 — Secure runtime baseline

Status: **Merged / regression baseline**

Proven on Android:

- secure native runtime bootstrap;
- Android Keystore / database-key bootstrap;
- SQLCipher-backed runtime initialization;
- required native runtime packaging including `libc++_shared.so`;
- dual-ABI release build;
- R8/JNI output verification;
- official Android SDK `apksigner` test-signing path;
- healthy backend with missing inference artifacts represented as capability-not-ready rather than total startup failure.

Permanent requirement: every later product milestone must preserve cold install → launch → secure backend/storage readiness.

---

## M1A / P1 — Typed runtime and Protocol V2 boundary

Status: **Merged**

Merged through PR #87 as:

`061ea5b943a45de55f1338e8278c51ab9f7023e5`

Implemented:

- typed multidimensional `AppReadiness`;
- typed runtime capability contract decoding;
- centralized Protocol V2 command JSON encoding;
- typed acknowledgement decoding;
- typed event-batch/event decoding and boundary validation;
- runtime-session/per-stream sequence tracking and gap detection;
- `BackendRuntimeHost` listeners receive typed event envelopes rather than feature-facing raw event JSON;
- `artifacts_required` maps to inference `ACTION_REQUIRED`, not backend failure.

Validation before merge:

- Android tests/lint/assemble — passed;
- offline APK permission verification — passed;
- Rust core/security matrix — passed;
- dual-ABI native release build — passed;
- R8 release APK build — passed;
- ABI/JNI/R8 output verification — passed;
- release artifact upload — passed.

### Remaining M1A architecture debt

The first P1 slice does **not** complete every original M1A aspiration.

Still planned/refinement:

- process-scoped coordinator/repository abstraction around `BackendRuntimeHost`;
- application composition root/dependency container;
- feature-facing observable state abstraction independent of the singleton's Compose `mutableStateOf`;
- first bounded query/snapshot request shape, which remains gated by ADR-007 review before product-domain expansion.

These remaining items must not be confused with a failure of the merged typed boundary; they are follow-on architecture refinement.

---

## M1B — Product shell + Home

Status: **In review**

Clean branch:

`impl/android-m1b-product-shell-v2`

PR:

#89 — `feat(android): add first product shell and Home surface`

The branch is based directly on merged P1 and contains only four shell/design-system files relative to `Kotlin`:

1. `MainActivity.kt` — launches product shell;
2. `MukeiProductShell.kt` — readiness-gated shell/Home/navigation;
3. `MukeiTheme.kt` — semantic blueprint palette + shape scale;
4. `MukeiTokens.kt` — spacing/radius/motion/layout tokens.

Implemented in the review branch:

- bounded startup and recovery surfaces;
- Home opening state with local-time greeting and `What’s on your mind?`;
- blueprint drawer hierarchy:
  - Mukei
  - Storage
  - Projects
  - Models
  - Chats
  - Settings at bottom;
- deterministic Back behavior;
- New Chat resets local draft/context and returns Home;
- optional capability context chips that do not act as mandatory modes;
- missing-model state routes conceptually to Models without treating the secure app shell as failed;
- reserved unfinished destinations are visibly non-functional rather than fake;
- small-screen/large-text scrolling;
- semantic light palette, spacing, shape, motion and layout tokens.

Intentionally not implemented:

- real conversation send/generation;
- model installation;
- storage/workspace functionality;
- Project/Artifact persistence;
- final production icon library integration.

### M1B merge gates

- Android tests/lint/assemble;
- offline APK verification;
- Rust/security matrix;
- release-hardening/R8/native verification;
- officially signed ARM64 test APK;
- real-device cold-launch/Home/drawer/back/large-text smoke test.

Old stacked PR #88 is closed and superseded by clean PR #89 after P1 was squash-merged.

---

## M2A — Conversation query/projection contract

Status: **Blocked intentionally**

Do not implement irreversible product-domain query/persistence assumptions until ADR review resolves at minimum:

- ADR-005 — authoritative state/process recovery;
- ADR-007 — Protocol V2 evolution and bounded query/snapshot contract.

Required next backend contract after acceptance:

- bounded chat index query;
- bounded conversation-detail projection;
- stable message/turn identities;
- model/inference readiness subset required by chat;
- repository reconciliation after event gap/runtime-session change.

No Compose conversation UI should become authoritative before this contract exists.

---

## M2B — Conversation MVP

Status: **Planned; depends on M2A**

Home composer currently remains non-sending by design.

The first real conversation slice must provide:

- Home send → create/resume conversation;
- durable user message projection;
- composed incremental assistant response updates;
- stop/cancel;
- truthful failure/retry;
- process-restart recovery without duplicates.

---

## M3 — Activity / operation visibility

Status: **Planned**

Depends on stable operation projection from the runtime/query contract.

---

## M4 — Universal Storage

Status: **Blocked on ADR/storage-port review**

Source candidate:

`temp/universal-storage-workspace-v0.1`

Policy:

- selectively port reviewed storage primitives;
- do not ancestry-merge the divergent branch;
- do not copy the one-workspace-per-chat durable constraint unchanged.

Valuable candidate areas:

- encrypted immutable object store;
- file versions;
- import transaction/journal/recovery;
- same-scope isolation guards;
- trash semantics;
- Android staged import pipeline.

---

## M5 — Workspace

Status: **Blocked on ADR-001/002 and M4 foundation**

Recommended product direction remains:

- stable workspace identity;
- simple primary-workspace UX for v0.1;
- no irreversible exactly-one-workspace-per-chat database invariant unless explicitly accepted.

---

## M6 — Artifacts

Status: **Planned / ADR-004 dependent**

---

## M7 — Models product surface

Status: **Planned**

Current shell may explain `artifacts_required`, but does not pretend model installation exists.

---

## M8 — Projects

Status: **Planned / ADR-003 dependent**

---

## M9 — Settings/privacy/personalization

Status: **Planned**

---

## M10 — Release hardening

Status: **Continuous + final milestone**

Already established permanent gates include:

- official APK signing verification;
- ABI/native dependency verification;
- real-device launch smoke test;
- process-death/recovery matrix;
- accessibility/large-text checks;
- offline/privacy behavior;
- migration compatibility.

---

# Current execution order

```text
P1 typed runtime boundary                          MERGED
        ↓
M1B clean product shell PR #89                    IN REVIEW
        ↓
M1A coordinator/repository refinement             REVERSIBLE / PLANNED
        ↓
ADR-005 + ADR-007 explicit decision
        ↓
M2A bounded conversation query/projection
        ↓
M2B real Conversation MVP
        ↓
ADR-001/002 + selective Storage port
        ↓
Workspace → Artifacts → Models → Projects
```

The execution rule remains: **never expose a polished UI that implies a backend capability which does not actually exist.**
