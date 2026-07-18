# 09 — Implementation Roadmap

Status: **Draft v0.2 — execution sequence**

This roadmap sequences Android work as vertical product slices. Every milestone must produce a coherent, installable, testable capability rather than isolated infrastructure.

## Governing rules

1. `Kotlin` remains the integration base.
2. `temp/universal-storage-workspace-v0.1` is a source of selectively reviewed storage work, not a branch to merge wholesale.
3. Security/privacy/data-integrity invariants outrank UI convenience.
4. Proposed ADRs must be reviewed before code that would make their decisions expensive to reverse.
5. Reads use authoritative bounded projections/queries; events provide incremental updates.
6. Compose does not call JNI or parse raw protocol JSON.
7. Each milestone ends with an officially signed APK and representative Android acceptance flow.
8. Missing model artifacts are a capability state, not a backend failure.

---

# M0 — Secure runtime baseline

Status: **Achieved; permanent regression coverage required**

## Proven baseline

- secure native runtime starts on Android;
- SQLCipher-backed runtime initializes;
- Android Keystore/key bootstrap works;
- JNI runtime/inference native dependencies package correctly;
- Protocol V2 foundation works;
- official Android SDK signing path has produced installable test APK;
- healthy backend with missing model artifacts reaches `Backend ready` rather than startup failure.

## Permanent regressions

`10_TEST_ACCEPTANCE_PLAN.md` requires coverage for:

- database key bootstrap not depending on premature JNI load;
- `libc++_shared.so`/native transitive dependency packaging;
- official `apksigner` verification;
- model artifacts missing ≠ backend unavailable;
- stable diagnostic specificity.

## Exit state

No further product milestone may regress cold install → launch → secure backend/storage readiness.

---

# M1A — Runtime coordinator and typed protocol foundation

## User value

Indirect but prerequisite: the product shell can trust typed readiness/capability state instead of raw bootstrap strings.

## Why before visual shell

The current `BackendRuntimeHost` exposes Compose state and raw JSON event strings directly. Building Home/Conversation on that surface would create migration debt immediately.

## Scope

### Runtime coordination

- evolve/wrap `BackendRuntimeHost` into process-scoped `MukeiRuntimeCoordinator`;
- preserve single native runtime per process;
- expose typed `StateFlow<AppReadiness>`;
- expose typed runtime capabilities;
- expose validated event flow;
- preserve platform request worker/broker behavior;
- remove feature-facing dependence on raw `JSONObject`/JSON strings.

### Protocol

- centralize command envelope construction in `:core:protocol`;
- typed acknowledgement decoding;
- typed event envelope decoding/validation;
- stable rejection/error mapping;
- runtime-session and sequence tracking;
- define first bounded query/snapshot request shape per ADR-007 direction.

### Repository seed

- `AppReadinessRepository` or equivalent;
- protocol/runtime adapter boundary;
- application-level dependency container/composition root.

## Explicit non-goals

- no full chat UI;
- no storage port;
- no empty feature modules created merely for structure.

## Exit criteria

```text
[ ] Main/UI layer no longer needs raw security-summary parsing
[ ] Runtime readiness is typed and multidimensional
[ ] Raw event JSON is decoded before feature layer
[ ] Runtime-session/sequence validation exists
[ ] Command creation uses shared protocol builder/codec
[ ] Activity recreation does not duplicate runtime/workers
[ ] Existing cold-start acceptance remains green
```

## Deliverable

Officially signed test APK showing temporary/product readiness shell driven by typed repository state.

---

# M1B — Product shell + design system

## User value

Mukei opens as the intended product instead of a backend diagnostic screen.

## Dependencies

- M1A typed readiness/capability state;
- ADR-006 direction reviewed enough to implement navigation without likely rewrite;
- `03_SCREEN_SPECIFICATIONS.md` S00–S02;
- `04_DESIGN_SYSTEM.md`.

## Scope

### Product shell

- single-Activity Compose root/NavHost;
- Home opening screen;
- modal navigation drawer;
- top-level routes;
- New Chat affordance;
- startup/recovery surface;
- model-unavailable routing affordance.

### Design system

- semantic light-theme tokens;
- typography roles;
- spacing/shape tokens;
- icon mapping abstraction;
- core buttons/chips/surfaces;
- motion/reduced-motion primitives;
- 48dp hit-target policy.

### Home

- GreetingBlock;
- PromptComposer shell;
- optional CapabilityChipRow;
- attachments entry affordance may remain disabled/placeholder until storage contract exists, but must not imply functionality that is absent.

## Exit criteria

```text
[ ] Cold launch reaches Home when secure shell is usable
[ ] Backend healthy + no model still reaches Home
[ ] Drawer routes work deterministically
[ ] Back closes drawer/transient UI correctly
[ ] Home does not require mode selection
[ ] Large text keeps primary actions reachable
[ ] TalkBack labels/state for primary controls
[ ] Rotation/recomposition preserves expected navigation/draft state
[ ] Officially signed APK installs and launches on ARM64 device
```

## Deliverable

**Mukei Shell APK v0.1-internal** — usable navigation/product opening, no fake conversation capability.

---

# M2A — Conversation query/projection contract

## User value

Prerequisite for durable real chat: conversations restore truthfully after restart rather than living only in event memory.

## Dependencies

- ADR-005 state authority direction;
- ADR-007 query/snapshot direction;
- M1A protocol adapters.

## Scope

### Bounded query infrastructure

Implement the first reusable product-domain query/snapshot contract for:

```text
chat_index
conversation detail
model/inference readiness or inventory subset required by chat
```

Requirements:

- bounded payload;
- selectors/pagination where needed;
- projection schema versions;
- runtime session identity;
- authorization/validation;
- Kotlin typed codecs.

### Conversation repository

- authoritative conversation projection;
- chat index/recent query;
- operation association;
- repository reconciliation after runtime restart/event gap;
- process-death interrupted-operation mapping.

### Structured response events

Freeze/implement minimum response projection semantics:

```text
response started
incremental composed content update
terminal completed/failed
```

Exact event names may vary but stable message/turn identity is required.

## Exit criteria

```text
[ ] Existing persisted conversation can be queried after process restart
[ ] Query is bounded and typed
[ ] Duplicate/stale events do not duplicate messages
[ ] Event gap triggers reconciliation
[ ] Runtime-session change invalidates stale transient assumptions
[ ] No Compose code parses response event JSON
```

## Deliverable

Repository/integration test evidence; APK may still expose only shell until M2B.

---

# M2B — Conversation MVP

## User value

User can start and continue a real conversation from Home.

## Dependencies

- M2A authoritative projection;
- usable local/remote inference capability configured for acceptance test.

## Scope

- Home send → create/resume conversation;
- user prompt rendering;
- document-like Mukei response rendering;
- hybrid streaming;
- stop/cancel;
- basic failure/retry where safe;
- scroll-follow/new-response-below behavior;
- conversation persistence/reopen;
- model-unavailable route to Models/provider setup;
- basic options only if backend contract exists (do not build dead menu items).

## State contract

Fully implement:

```text
ComposerState
ConversationOperationState
ResponseRenderState
basic ActivityState
RecoveryState
```

## Exit criteria

```text
[ ] Home → Send produces exactly one command/message
[ ] Accepted ≠ completed semantics preserved
[ ] Response streams/converges without duplication
[ ] Stop is deterministic and duplicate-stop suppressed
[ ] Navigate away/back does not silently cancel
[ ] Process kill/relaunch restores durable messages
[ ] Stale Running state cannot survive without authoritative operation
[ ] Missing model gives capability guidance, not backend failure
[ ] ARM64 real-device flow passes
```

## Deliverable

**Conversation MVP APK**.

---

# M3 — Activity and operation visibility

## User value

Long-running work is understandable and controllable without exposing a raw log console.

## Dependencies

- M2 operation/event projection;
- typed activity normalization contract from `06_UI_BACKEND_CONTRACT.md`.

## Scope

- collapsed/expanded `ActivityCard`;
- grouped categories: searching/reading/writing/editing/building/testing/packaging;
- real counts/progress only;
- cancellation/approval/waiting states;
- failure + preserved-work summary;
- activity details surface;
- accessibility announcement throttling.

## Backend work

Operation `detail` string alone is insufficient. Introduce typed/groupable activity events/projection.

## Exit criteria

During a long task user can answer:

1. What is happening?
2. Is it still running?
3. Can I stop/approve it?
4. What remains saved if it fails?

No fake percentages.

## Deliverable

Conversation APK with real progressive-disclosure Activity behavior.

---

# M4A — Universal Storage core selective port

## User value

Foundation for durable imported/generated files.

## Dependencies

- ADR-001/002 direction reviewed;
- `07_STORAGE_WORKSPACE_MODEL.md`;
- ADR-007 query infrastructure.

## Port strategy

Selectively port/reconcile from `temp/universal-storage-workspace-v0.1`:

- `storage/universal.rs` concepts, adjusted for accepted workspace cardinality;
- file admission policy;
- immutable encrypted object store;
- version repository;
- import journal/commit/recovery;
- staged cleanup;
- trash repository;
- isolation guards/migrations after schema review;
- storage integration tests.

Do **not** port:

- numbered migration/diagnostic workflows wholesale;
- one-workspace-per-chat database constraint without ADR resolution;
- stale runtime/native files that overwrite newer Kotlin hardening.

## Required security architecture

- SQLCipher DB remains encrypted;
- separate wrapped object-store encryption key;
- opaque object paths;
- same-scope persistence guards;
- bounded object reader/writer;
- crash-recoverable journaling.

## CI gate before UI

```text
[ ] clean migration applies
[ ] upgrade migration fixture passes
[ ] cross-scope isolation tests pass
[ ] import commit/retry/recovery tests pass
[ ] object-store bounds/integrity tests pass
[ ] full Rust suite green
```

The previously observed oversized-ciphertext test problem must be resolved, not skipped.

---

# M4B — Storage protocol + Android import MVP

## User value

User can import a file and retrieve it later from Storage.

## Scope

### Protocol

- explicit storage scope/node IDs;
- canonical `storage.import` mutation (not limited only to active chat workspace);
- import cancel/recovery semantics;
- bounded Storage list/detail query;
- stable storage/import errors.

### Android

- system picker;
- platform request/correlation handling;
- staged copy/access flow;
- Storage screen MVP;
- stored-vs-indexing state separation;
- basic metadata/preview for supported types.

## Exit criteria

```text
[ ] Import from Android picker
[ ] File is encrypted at rest
[ ] Appears in correct scope after restart
[ ] Picker cancellation is not failure
[ ] Indexing failure does not hide stored file
[ ] Process death during import recovers deterministically
[ ] No cross-scope leakage
[ ] No indefinite plaintext staging
```

## Deliverable

**Storage MVP APK**.

---

# M5 — Workspace vertical slice

## User value

Structured file-producing work becomes tangible from Conversation.

## Dependencies

- ADR-001 accepted/modified;
- M4 storage scopes;
- explicit workspace query/protocol scope.

## Scope

- workspace identity/lifecycle;
- primary-workspace relation policy;
- `WorkspaceCard` in conversation;
- Workspace screen;
- file/folder semantic presentation;
- Created/Edited/Imported/Read/Failed states;
- active mutation visibility;
- basic file preview;
- recovery after partial task failure;
- explicit workspace authorization.

## Exit scenario

```text
Chat request
→ structured work becomes relevant
→ create/select explicit workspace
→ create/edit durable files
→ WorkspaceCard appears
→ inspect workspace/files
→ leave/kill app
→ return to same durable workspace
```

## Security gate

Manipulated workspace/chat/scope IDs must fail closed.

## Deliverable

**Workspace Vertical Slice APK**.

---

# M6 — Artifact identity and export

## User value

Mukei delivers usable outputs rather than only messages/files-in-progress.

## Dependencies

- ADR-004 accepted/modified;
- Storage/Workspace durable version identities.

## Scope

- Artifact entity/projection;
- designate user-meaningful deliverable from generated file/version/bundle;
- `ArtifactCard`;
- export/save/share platform flow;
- ZIP/report/document preparation as supported;
- export history/status where useful;
- re-export from Storage/Workspace after restart.

## Exit criteria

```text
[ ] Artifact has stable ID and backing storage identity
[ ] ArtifactCard states what is ready
[ ] External export failure preserves internal artifact
[ ] Export does not silently delete workspace copy
[ ] Re-export works after app restart
```

## Deliverable

**Artifact/Export APK**.

---

# M7 — Models product surface

## User value

Fresh install can move from `artifacts_required` to usable inference without technical guesswork.

## Dependencies

- bounded model inventory/active projection;
- existing model download/select/delete commands integrated through repository.

## Scope

- Models list/detail;
- installed vs available;
- storage size/compatibility;
- real download progress;
- verify/install/activate;
- cancel;
- pause/resume only if backend supports it;
- local vs remote/provider distinction;
- active model projection.

## Exit criteria

```text
[ ] Missing-model state routes to Models
[ ] Install progress is real
[ ] Verification failure is explicit
[ ] Installed does not imply Active
[ ] Activation failure does not falsely show Active
[ ] Active model enables Conversation capability
```

## Deliverable

**Model Setup APK** capable of completing first usable inference setup where artifacts/source are available.

---

# M8 — Projects

## User value

Long-lived work can be organized across chats, workspaces, files, and artifacts.

## Dependencies

- ADR-003 accepted/modified;
- workspace/artifact stable identities;
- project query/protocol contract.

## Scope

- project list/detail;
- create/rename/delete;
- attach/detach chats;
- reference workspaces/artifacts/files according to accepted model;
- visible active project context;
- continue-work flow;
- explicit mutation targets.

## Exit criteria

```text
[ ] Project deletion semantics are explicit
[ ] Adding item does not silently duplicate bytes
[ ] Active context visible before mutation
[ ] Chat deletion does not silently destroy independent project work
[ ] Resume project restores correct context
```

## Deliverable

**Projects MVP APK**.

---

# M9 — Settings, personalization, privacy controls

## Scope

- Personalization;
- Memory;
- Appearance;
- Privacy;
- Storage controls;
- Providers/credentials via secure secret references;
- Advanced/diagnostics;
- About/open-source notices.

## Rules

- no nonfunctional toggles;
- generic settings payload must not become insecure secret storage;
- destructive reset/delete scope explicit;
- reduced-motion setting integrated with design system.

## Deliverable

Settings-complete internal candidate.

---

# M10 — Release certification/hardening

## Scope

- full accessibility matrix;
- performance/startup profiling;
- process death/restart matrix;
- offline/local-first behavior;
- storage corruption/recovery cases;
- signed install/update validation;
- ABI/API/device matrix;
- schema migration compatibility;
- destructive-action tests;
- privacy/provider disclosure review;
- diagnostic redaction.

## Release gate

Use `10_TEST_ACCEPTANCE_PLAN.md` release candidate checklist.

A release candidate MUST include officially signed/verifiable artifacts and real-device acceptance evidence.

---

# Parallel work tracks

Some work can happen in parallel without violating dependency order.

## Design/UI track

```text
Design system primitives
→ Home/Drawer previews
→ Conversation renderer/components
→ Activity/Workspace/Artifact components
```

UI previews use fake typed models, not invented backend behavior.

## Protocol/runtime track

```text
Typed coordinator
→ bounded query contract
→ conversation projections
→ storage/workspace protocol
→ artifact/project projections
```

## Rust storage track

```text
Review temp storage commits
→ resolve failing test
→ selective core port
→ schema/cardinality adjustments
→ full isolation/recovery matrix
```

## QA track

```text
Official signing/install harness
→ emulator smoke
→ ARM64 physical smoke
→ milestone-specific acceptance automation
```

---

# Branch/integration strategy

For each milestone:

```text
Kotlin
  ↓ feature branch
small vertical PRs
  ↓
CI + review
  ↓
merge to Kotlin
  ↓
officially signed internal APK
  ↓
device acceptance
```

Avoid long-lived mega-branches that combine UI, storage migrations, protocol redesign, and unrelated fixes.

Storage temp branch is mined by specific commits/files/ideas, not merged as ancestry.

---

# First executable issue sequence

After ADR review, implementation should begin with these concrete work packets:

## P1 — Typed runtime readiness

- introduce typed readiness/capabilities;
- protocol builder for initialize;
- remove direct bootstrap string rendering dependency;
- preserve existing backend boot.

## P2 — Typed event transport

- decode `EventBatchV2` in protocol layer;
- session/sequence tracker;
- Flow-based event router;
- raw listener compatibility adapter temporarily if needed.

## P3 — Product shell

- semantic tokens/theme;
- NavHost + drawer;
- Home/composer shell;
- startup/model-unavailable states.

## P4 — Query contract prototype

- bounded conversation detail query;
- Kotlin/Rust round-trip;
- schema/version/limit tests.

## P5 — Conversation repository + UI

- send/ack/events/query reconciliation;
- stop;
- durable reopen/restart;
- document-like renderer.

Only after P1–P5 are stable should broad Storage UI implementation begin.

---

# Milestone evidence standard

Every milestone closure record includes:

- merged commit SHA;
- CI run ID;
- signed APK SHA-256;
- signing certificate/channel;
- emulator/device/API/ABI tested;
- acceptance checklist result;
- known limitations;
- screenshots where useful;
- linked follow-up issues.

---

# Current immediate next action

The documentation dependency layer is now drafted. Before implementation begins:

```text
1. Review/accept or modify ADR-001..ADR-007
2. Freeze M1A/M1B contracts
3. Create small implementation issues P1–P5
4. Start P1 typed runtime readiness on a fresh branch from Kotlin
```

Do not begin Projects or direct-merge the temp storage branch while ownership/cardinality/protocol decisions remain unreviewed.
