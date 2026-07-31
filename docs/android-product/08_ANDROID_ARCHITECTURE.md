# 08 — Android Architecture

Status: **Draft v0.1**

This document defines the target Android/Kotlin architecture for implementing the Mukei product specification on the canonical `Kotlin` branch.

It is evolutionary: preserve the working secure runtime/JNI foundation, then move product behavior out of the current bootstrap singleton/screen into typed repositories, state holders, and feature surfaces.

---

# 1. Current baseline

Current Gradle modules:

```text
:app
:core:protocol
:core:native
:core:designsystem
```

Current application shape is approximately:

```text
MainActivity
  ↓
Compose bootstrap/status UI
  ↓
BackendRuntimeHost process singleton
  ├─ SecureRuntimeFactory
  ├─ RustNativeGateway
  ├─ event-drain worker
  └─ AndroidPlatformRequestProcessor
```

This has proven the secure native bootstrap and transport path, but it is not the final product architecture.

Key current limitations:

- `MainActivity` renders backend status directly instead of a navigation/product shell;
- `BackendRuntimeHost` exposes Compose `mutableStateOf` directly from a process singleton;
- events are distributed as raw JSON strings;
- feature repositories do not yet own typed projections;
- runtime readiness, product capability, and UI navigation are coupled too closely;
- no feature module boundaries exist yet.

---

# 2. Target dependency direction

```text
Compose UI
  ↓
Feature ViewModel / state holder
  ↓
Use case (only where orchestration adds value)
  ↓
Repository interface
  ↓
Protocol/domain adapter
  ↓
Native runtime coordinator / gateway
  ↓
JNI
  ↓
Rust runtime
```

Dependencies MUST point inward toward stable contracts.

Composable functions MUST NOT:

- call JNI/native gateway directly;
- parse protocol JSON directly;
- manage idempotency/correlation IDs;
- own durable domain truth;
- launch duplicate platform requests because of recomposition.

---

# 3. Composition root

`:app` is the Android application composition root.

Responsibilities:

- create process-scoped runtime coordinator;
- create repository implementations;
- wire navigation;
- host Mukei theme;
- own Android Activity/Application lifecycle integration;
- wire platform adapters (document picker, share/export, permissions);
- select feature implementations.

`:app` SHOULD NOT accumulate feature business logic.

---

# 4. Runtime coordinator

Replace the product-facing role of `BackendRuntimeHost` with a typed process-scoped coordinator.

Candidate concept:

```text
MukeiRuntimeCoordinator
- start()
- shutdown()
- readiness: StateFlow<AppReadiness>
- capabilities: StateFlow<RuntimeCapabilities>
- command transport
- snapshot/query transport
- event stream
- platform request stream
```

The existing `BackendRuntimeHost` may be incrementally refactored into this role rather than rewritten at once.

## Coordinator responsibilities

- own exactly one active native gateway per process;
- serialize bootstrap/shutdown lifecycle;
- expose typed readiness, not Compose mutable state;
- drain/validate protocol events off main thread;
- detect runtime session changes;
- trigger snapshot reconciliation on gaps/restart;
- broker platform requests;
- redact/map stable bootstrap failures.

## Must not

- become a god-object containing chat/storage/model business logic;
- expose raw `JSONObject` to feature code;
- own feature-specific Compose state.

---

# 5. Protocol module

`:core:protocol` owns Kotlin representations and codecs for the cross-language contract.

Responsibilities:

- Protocol V2 envelope models;
- version/capability models;
- typed command builders/codecs;
- typed acknowledgement decoding;
- typed event envelope decoding;
- typed snapshot/query envelope decoding;
- protocol limits and validation;
- stable machine error/rejection models.

## Rule

Feature modules SHOULD consume typed domain/repository models, not protocol models directly except at explicit adapter boundaries.

## JSON

Protocol JSON serialization belongs in protocol adapters/codecs.

Do not construct command JSON manually in feature ViewModels.

The current manual `JSONObject` initialization envelope should migrate to the shared protocol codec/builder.

---

# 6. Native module

`:core:native` owns Android↔JNI transport and secure native-runtime creation.

Responsibilities:

- `MukeiNativeGateway` / `RustNativeGateway` transport;
- secure key bootstrap/factory;
- JNI validation and bounded byte transport;
- platform request processor primitives;
- native library loading;
- native runtime security status transport;
- no product-screen logic.

## Rule

`:core:native` MUST NOT depend on feature modules.

The JNI boundary remains raw bytes/JSON where required, but raw transport must be decoded before reaching feature repositories.

---

# 7. Design system module

`:core:designsystem` owns the contract in `04_DESIGN_SYSTEM.md`.

Responsibilities:

- `MukeiTheme`;
- semantic colors;
- typography;
- shapes;
- spacing tokens;
- motion tokens;
- icon mapping/adapter;
- generic buttons/chips/card surfaces;
- common dialog/sheet/status primitives.

It should not own domain-aware components that need chat/workspace/model repositories.

---

# 8. Recommended shared model module

Add when typed repositories begin to grow:

```text
:core:model
```

Responsibilities:

- product-domain immutable Kotlin models shared across features;
- IDs/value objects;
- readiness/capability models;
- conversation summaries;
- storage/workspace/artifact summaries;
- model inventory summaries;
- typed UI-independent errors.

Avoid placing Android framework classes in `:core:model`.

If premature for M1, these models may begin in feature/repository packages and extract once sharing becomes real.

---

# 9. Repository layer

Repositories convert protocol/runtime/storage projections into stable product contracts.

Candidate interfaces:

```text
AppReadinessRepository
ConversationRepository
ActivityRepository
StorageRepository
WorkspaceRepository
ArtifactRepository
ProjectRepository
ModelRepository
SettingsRepository
```

Not all need separate classes immediately; split by cohesive ownership, not by checklist.

## Repository responsibilities

- submit commands with IDs/idempotency;
- decode acknowledgements;
- project ordered events;
- request authoritative snapshots/queries;
- reconcile after runtime restart/event gap;
- expose `Flow`/`StateFlow` of immutable domain projections;
- map backend failures to typed domain errors;
- keep Compose unaware of transport details.

## Repository non-responsibilities

- Composable layout;
- Activity navigation;
- arbitrary formatting/copy;
- raw secret presentation.

---

# 10. Event projection architecture

Current runtime drains JSON event batches on a dedicated executor and dispatches raw strings to listeners.

Target:

```text
Native event drain
  ↓
ProtocolEventDecoder
  ↓ validate version/session/sequence
EventRouter
  ↓
Feature/domain projectors
  ↓
Repository StateFlow
```

## Requirements

- sequence gap detection;
- duplicate event suppression;
- runtime session reset handling;
- bounded queues/backpressure;
- no heavy JSON parsing on main thread;
- unknown forward-compatible events are diagnosable and safely ignored when policy permits;
- terminal operation events update authoritative repository projection.

---

# 11. Snapshot/query reconciliation

Events provide incremental updates; snapshots/queries provide authoritative recovery.

Repository algorithm:

```text
Start / runtime session changed / sequence gap
  ↓
mark projection Recovering if user-visible
  ↓
request bounded authoritative snapshot/query
  ↓
replace/reconcile repository projection
  ↓
resume incremental event application
```

Do not replay the entire event history to rebuild durable product state after every process start.

---

# 12. Feature modules

Recommended target structure as implementation grows:

```text
:feature:home
:feature:conversation
:feature:storage
:feature:workspace
:feature:models
:feature:projects
:feature:settings
```

Artifact/activity UI may begin inside conversation/workspace features and extract only if shared ownership justifies dedicated modules.

## Module timing

Do not create all modules empty on day one.

Create a feature module when:

- it has a real screen/navigation destination;
- it owns meaningful state/repository dependencies;
- separation improves build/test ownership.

M1 may implement Home/navigation in `:app` temporarily if boundaries remain clean; M2 should establish the feature pattern before complexity grows.

---

# 13. Feature internal structure

Suggested pattern:

```text
feature/conversation/
  ui/
    ConversationRoute
    ConversationScreen
    components/
  ConversationViewModel
  ConversationUiState
  ConversationUiAction
  navigation/
```

Repositories/interfaces may live in a domain/data layer outside the feature depending on reuse.

## Route vs Screen

- `Route` wires ViewModel, lifecycle collection, navigation callbacks.
- `Screen` is stateless/pure enough for previews/tests.

This pattern is recommended, not an absolute naming mandate.

---

# 14. UI state ownership

Follow `05_INTERACTION_STATE_MODEL.md`.

Each feature should expose one coherent immutable state tree.

Example:

```text
ConversationUiState
- readiness/capability summary
- conversation projection
- composer state
- active operation state
- transient UI state
```

Avoid independent mutable booleans that can contradict each other.

## Compose collection

Use lifecycle-aware Flow collection.

Do not expose `mutableStateOf` from process singletons as the long-term repository API.

---

# 15. UI actions and effects

Use explicit actions:

```text
SendClicked
StopClicked
AttachmentClicked
WorkspaceClicked
RetryClicked
```

ViewModel decides domain request.

One-off effects:

- open Android picker;
- open share sheet;
- request focus;
- show transient snackbar when appropriate.

Effects must not replay accidentally after recomposition/configuration change.

External intents should use stable request identity/reconciliation where operation correctness depends on them.

---

# 16. Navigation architecture

Top-level product destinations:

```text
Home
Storage
Projects
Models
Settings
Conversation(chatId)
Workspace(workspaceId)
Project(projectId)
```

Drawer provides top-level navigation, but detail screens use nested navigation routes.

## Recommended model

Single Activity + Compose Navigation.

Use typed route arguments/value IDs rather than passing entire mutable objects.

## Back rules

- drawer/sheet/dialog closes first;
- detail → parent/context;
- top-level destination follows normal root/back-stack policy;
- running operations do not cancel just because navigation changes.

Exact top-level stack preservation policy should be locked by ADR.

---

# 17. IDs and scope

Use stable typed IDs throughout domain/Kotlin models:

```text
ConversationId
BranchId
OperationId
WorkspaceId
StorageScopeId
StorageNodeId
FileVersionId
ArtifactId
ProjectId
ModelId
```

Raw String may be transport representation, but feature logic should prefer value objects where practical.

Mutations must carry explicit scope IDs required for safety.

Never use filesystem path as product identity.

---

# 18. Coroutines and threading

Target concurrency rules:

- UI/ViewModel orchestration: structured coroutines;
- JNI/native calls: repository/coordinator dispatcher, not main thread if potentially blocking;
- JSON decode/projection: background dispatcher;
- database/native internal work remains Rust-owned where designed;
- Android platform UI launch occurs on appropriate lifecycle/main context;
- cancellation propagates by operation command/token semantics, not merely cancelling a Kotlin collector.

## Executors migration

Current dedicated Java executors may remain inside coordinator during transition.

Long term, prefer structured lifecycle ownership and clearly bounded threads rather than feature-created executors.

Do not migrate blindly if native long-poll/blocking behavior requires dedicated threads; document the reason.

---

# 19. Process lifecycle

Runtime is process-scoped.

Application startup owns initialization; Activity recreation must not recreate native runtime.

## Required invariants

- one active runtime per process;
- Activity rotation/recreation does not duplicate workers;
- app process death is reconciled from durable/runtime state on restart;
- shutdown is deterministic where Android lifecycle allows, but correctness must not depend on `Application.onTerminate()` (not reliable on production Android);
- abrupt process death leaves storage/recovery journals consistent.

---

# 20. Dependency injection

Use explicit constructor injection.

A full DI framework is optional.

For M1/M2, a simple application container can provide:

```text
RuntimeCoordinator
Repositories
Protocol codecs
Platform adapters
```

Adopt Hilt/Koin/etc. only if benefits justify dependency/build complexity.

Service locator access from arbitrary composables is prohibited.

---

# 21. Storage architecture

Follow `07_STORAGE_WORKSPACE_MODEL.md`.

Android responsibilities:

- system picker interaction;
- URI permission/access;
- bounded staging copy when requested by native protocol;
- user-visible export/share destination;
- preview/content handles where appropriate.

Rust responsibilities should remain authoritative for:

- file admission policy;
- encrypted object storage;
- storage metadata/versioning;
- scope isolation;
- import commit/recovery journal;
- trash/recovery semantics;
- indexing domain.

Do not reimplement storage policy independently in Kotlin.

---

# 22. Platform request architecture

Native runtime may request Android platform actions through broker.

Target path:

```text
Rust PlatformRequest
  ↓ JNI
RuntimeCoordinator
  ↓ typed PlatformRequest
PlatformRequestHandler
  ↓ Android API / user UI
PlatformResponse
  ↓ coordinator/JNI
Rust
```

Handlers must be testable and lifecycle-aware.

No Compose recomposition should invoke a request twice.

---

# 23. Error architecture

Layers:

```text
Native machine error
  ↓ protocol typed code/context
Repository DomainFailure
  ↓ feature mapping
UiFailure / recovery actions
  ↓
human copy + Details
```

Never make UI parse arbitrary exception message text to decide behavior.

Stable diagnostic codes can be retained separately from user-facing copy.

Sensitive paths/keys/provider secrets must be redacted.

---

# 24. Logging and diagnostics

- structured logging with stable codes;
- no plaintext SQLCipher/object keys;
- no API secrets;
- avoid full user document/chat content in routine logs;
- diagnostics export must be reviewed for privacy/redaction;
- release builds should retain enough stable error codes for support without exposing internals.

---

# 25. Testing architecture

## Unit

- reducers/state transitions;
- command builders/idempotency;
- event projectors;
- repository error mapping;
- navigation decision logic;
- design-system semantic rules where testable.

## JVM/Android integration

- ViewModel + fake repositories;
- protocol codecs/golden fixtures;
- platform request handlers;
- storage import adapter boundaries.

## Instrumented/Compose

- screen semantics;
- navigation;
- large text;
- reduced motion behavior;
- operation controls;
- process/recreation scenarios where feasible.

## Native/CI

- Rust core tests;
- JNI boundary tests;
- APK native dependency verification;
- real signed install/launch acceptance.

Detailed gates are in `10_TEST_ACCEPTANCE_PLAN.md`.

---

# 26. Migration plan from current app

## Step A — Runtime API cleanup

- introduce typed readiness `StateFlow` adapter around current host;
- centralize protocol command encoding/decoding;
- expose typed event flow instead of raw listener strings.

## Step B — Product shell

- replace diagnostic `MainActivity` content with `MukeiApp` navigation host;
- Home + drawer;
- startup failure/recovery route.

## Step C — Conversation repository

- implement command submission/ack handling;
- operation/event projection;
- authoritative conversation query/snapshot contract;
- ViewModel.

## Step D — Storage/workspace repositories

- selectively port hardened storage core;
- add typed protocol/query APIs;
- implement Storage/Workspace screens.

## Step E — Models/projects/artifacts

- expand repository/feature modules vertically.

---

# 27. Architecture invariants

1. Compose never calls JNI directly.
2. Raw JSON does not cross into feature UI.
3. Runtime lifecycle is process-scoped and single-owner.
4. Repositories own domain projection/reconciliation.
5. Durable truth is not stored only in ViewModels/SavedState.
6. Events are incremental; snapshots/queries recover authority.
7. Feature actions are capability-gated.
8. Mutations use explicit IDs/scopes/idempotency.
9. Kotlin does not duplicate Rust storage/security policy.
10. Navigation does not imply operation cancellation.
11. Platform requests execute once with stable correlation.
12. Every shipped vertical slice ends in real-device acceptance, not compilation alone.
