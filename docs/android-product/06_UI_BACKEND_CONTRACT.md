# 06 — UI ↔ Backend Contract

Status: **Draft v0.1**

This document maps the Android product contract to the current Kotlin/JNI/Protocol V2/Rust runtime surface.

It distinguishes:

- **Implemented now** — present on the current `Kotlin` branch;
- **Prototype available** — exists on `temp/universal-storage-workspace-v0.1` but is not part of the canonical Kotlin line;
- **Required / proposed** — product contract needed to implement the UI/UX blueprint truthfully.

Proposed command/event names are drafts until implemented and reviewed.

---

# 1. Boundary rule

The UI MUST NOT call Rust-native implementation details directly.

Canonical flow:

```text
Compose Screen
  ↓ user action
ViewModel / Use Case
  ↓ typed app/domain request
Repository
  ↓ Protocol adapter
CommandEnvelopeV2 / snapshot request
  ↓
MukeiNativeGateway
  ↓ JNI bounded JSON
Rust Runtime
  ↓ acknowledgement + ordered events + snapshots
Repository projection
  ↓ immutable UiState
Compose
```

`MukeiNativeGateway` is a transport boundary, not a feature repository.

---

# 2. Current Protocol V2 foundation

Current protocol provides:

- versioned command envelopes;
- command/request/correlation/operation identities;
- optional idempotency keys;
- structured command scope;
- immediate accepted/rejected acknowledgements;
- ordered event envelopes and bounded batches;
- runtime snapshots;
- capability negotiation;
- replay protection and operation lifecycle capabilities.

Current command scope fields:

```text
conversation_id?
branch_id?
turn_id?
model_id?
document_id?
```

Current scope does **not** identify:

- workspace;
- storage scope/node;
- project;
- artifact;
- file version.

---

# 3. Current canonical command registry

Implemented registry on `Kotlin`:

```text
app.initialize
chat.send_message
chat.stop_generation
chat.clear_conversation
model.download
download.cancel
model.select
model.delete
document.grant
document.revoke
document.retry_ingestion
settings.update
recovery.resume
recovery.regenerate
```

Runtime capability advertisement is dynamic. For example, chat/model-selection/recovery commands are exposed only when the inference backend factory is present; document retry depends on RAG service availability; model download depends on network build capability.

The UI MUST gate actions using negotiated capabilities rather than assuming every registry command is executable.

---

# 4. Current snapshot limitation

Current public snapshot domains are:

```text
application
settings
protocol
operations
```

The runtime internally persists projections for:

- conversations;
- models;
- documents;
- operations.

However, the canonical public snapshot domain enum does not currently expose first-class conversation/model/document/storage/project/workspace snapshots to Android.

This is a major product integration gap.

## Contract principle

The UI SHOULD NOT reconstruct durable product state only by replaying transient events from process start.

For every durable feature domain, Android needs an authoritative query/snapshot path.

---

# 5. Command acknowledgement contract

A user action that submits a command follows:

```text
UI action
  ↓
CommandEnvelopeV2
  ↓
Immediate acknowledgement
  ├─ Accepted(operation_id) → project operation state
  └─ Rejected(reason) → recover draft / show capability or validation state
```

Acknowledgement means **accepted for processing**, not completed.

UI MUST NOT interpret `Accepted` as success.

Common rejection mappings:

| Protocol reason | UI interpretation |
|---|---|
| `unsupported_protocol` | app/runtime incompatible; blocking update/recovery state |
| `unknown_command` | implementation mismatch; diagnostic failure |
| `invalid_payload` | app bug or invalid user input; do not blindly retry |
| `capability_unavailable` | route/configure required capability |
| `busy_conflict` | operation conflict; wait/stop/retry when valid |
| `stale_scope` | selected context is no longer valid; refresh/reselect |
| `backend_unavailable` | affected runtime capability unavailable |
| `duplicate_replay_conflict` | idempotency conflict; do not duplicate action |
| `policy_denied` | local security/product policy denied action |

User-facing copy MUST be domain-specific, not a raw enum display.

---

# 6. Event contract

Current events include common operation lifecycle semantics such as:

```text
operation.accepted
operation.completed
```

Feature implementations emit additional event types.

## UI projection rules

1. Events are ordered within `stream_id` using `sequence`.
2. Kotlin repository MUST validate protocol/session identity before projection.
3. Duplicate events MUST be ignored idempotently.
4. Sequence gaps MUST trigger snapshot/reconciliation rather than silent guesswork.
5. Events update repository/domain projections; composables do not parse arbitrary event JSON directly.
6. Terminal operation event/snapshot state overrides stale local loading flags.

---

# 7. App readiness mapping

Related: **S00 / F01 / Interaction State §4**

## Existing

- secure runtime factory/JNI runtime creation;
- `app.initialize`;
- `application.ready` event;
- protocol capability contract;
- application/protocol/operations snapshots.

## Required Kotlin projection

```text
AppReadinessRepository
- shell readiness
- secure runtime readiness
- encrypted storage readiness
- inference readiness
- provider/network readiness
```

`Backend ready` MUST remain distinct from `model artifacts available`.

## Gap

Current bootstrap/status screen exposes security summary directly. Product shell needs a typed readiness projection and human recovery mapping.

---

# 8. Home / composer mapping

Related: **S01 / F02**

Home itself requires no backend command to render.

## UI-owned

- greeting;
- draft;
- selected capability hint;
- attachment selection before durable import;
- keyboard/focus.

## Backend dependencies

- negotiated inference capability;
- active model/provider readiness;
- optional active project/context.

## Required query gap

Android needs a reliable capability/readiness projection rather than inferring readiness from generic startup text.

---

# 9. Conversation mapping

Related: **S03 / F03–F06, F12–F13**

## Existing commands

### Send

```text
chat.send_message
scope:
  conversation_id?
  branch_id?
payload:
  text
```

Requires idempotency key.

### Stop

```text
chat.stop_generation
operation_id: required
scope:
  conversation_id: required
  branch_id: required
```

### Clear

```text
chat.clear_conversation
```

### Recovery

```text
recovery.resume
recovery.regenerate
```

Recovery commands require conversation + branch scope.

## Existing durable internals

Runtime stores conversation projections keyed by conversation/branch and persists messages.

## Missing public product contract

Android needs authoritative access to:

- list chats;
- open one conversation/branch;
- conversation message projection;
- title/pin/archive/project-link metadata;
- current operation/recovery status;
- branch/recovery choices.

## Proposed snapshot domains

```text
conversation
chat_index
```

Alternative: one `conversations` domain with paged/query selectors. Exact transport shape should be chosen based on snapshot size limits.

## Proposed commands

Only mutations require commands. Suggested registry additions:

```text
chat.create                 (only if explicit creation is needed before send)
chat.rename
chat.pin
chat.unpin
chat.archive
chat.unarchive
chat.delete
chat.add_to_project
chat.remove_from_project
```

Do not add commands for simple reads if authoritative typed snapshots/query APIs are more appropriate.

## MVP requirement

M2 Conversation MVP minimally needs:

- send;
- stop;
- authoritative conversation projection after send/restart;
- operation projection;
- model-unavailable mapping;
- recovery after process death.

Pin/archive/project linking may land later.

---

# 10. Hybrid response streaming

UI requires composed incremental response blocks.

## Required event semantics

The backend/repository contract should expose structured response evolution such as:

```text
response.started
response.chunk_appended
response.block_replaced   (optional for structured rendering)
response.completed
response.failed
```

Exact names are proposed.

Payload MUST carry stable message/turn identity so Kotlin can converge partial and final content without duplicate messages.

## Do not

- key partial content only by operation ID when a durable message ID exists;
- expose raw token callback behavior directly to Compose;
- require UI to concatenate arbitrary JSON fragments.

---

# 11. Activity mapping

Related: **S04 / F05–F07**

## Existing foundation

- operation records;
- operation status/progress/detail/result;
- ordered event streams;
- platform request broker capability.

## Required product projection

```text
ActivityProjection
- operationId
- summary
- grouped phases
- items/status
- real progress?
- provider/tool disclosure?
- available controls
- preservedWork?
```

## Proposed event families

```text
activity.phase_started
activity.phase_progress
activity.item_updated
activity.waiting_for_user
activity.phase_completed
```

Feature events may be richer internally, but Kotlin should normalize them into one product activity model.

## Rule

`detail` string in an operation record is insufficient as the long-term UI contract for structured Activity details.

---

# 12. Documents / file attachment mapping

Related: **S01, S03, S06 / F07**

## Existing canonical commands

```text
document.grant
document.revoke
document.retry_ingestion
```

Current grant payload accepts a staged private target, label, and MIME type.

Runtime persists document projection with states including staging/staged/indexed/ingestion unavailable/revoked/failed.

## Product mismatch

The blueprint requires durable universal/workspace storage semantics, not only a RAG/document-grant abstraction.

`document.grant` should remain a document-access/ingestion capability where appropriate, but it must not become the canonical file-storage API.

## Prototype

Temp storage branch adds:

```text
storage.import_file
```

for Android selected documents imported into an active chat workspace.

This is useful but too narrow for the full product because Storage also needs universal-scope import and explicit target selection.

## Proposed storage commands

```text
storage.import
storage.cancel_import
storage.create_directory
storage.rename_node
storage.move_node
storage.trash_node
storage.restore_node
storage.delete_permanently
storage.create_version / storage.update_file (policy-dependent)
```

Command design should use explicit target scope/node IDs and idempotency keys for mutations.

Read/list/search should use storage snapshot/query contracts.

---

# 13. Storage/workspace snapshot contract

Required domains/projections:

```text
storage.overview
storage.scope
storage.node
workspace
```

Implementation may expose these as selector-driven snapshot requests rather than separate fixed enum cases if payload size/pagination demands it.

Minimum workspace projection:

```text
workspaceId
scopeId
owner/context relationship
state
rootNodeId
title/displayName
files/folders summary
artifacts summary
active operations
```

Minimum storage node projection:

```text
nodeId
scopeId
parentNodeId?
kind
name
state
currentVersionId?
role?
size/type metadata
provenance
availableActions
```

---

# 14. Workspace mapping

Related: **S05 / F06, F08, F10**

## Current Kotlin

No canonical workspace command/snapshot identity in `CommandScope`.

## Temp prototype

Provides workspace IDs/scopes and one-workspace-per-chat domain rules, plus storage import into active chat workspace.

## Required protocol extension

`CommandScope` (or successor scope structure) needs explicit fields when workspace mutations are introduced:

```text
workspace_id?
storage_scope_id?
storage_node_id?
project_id?
artifact_id?
file_version_id?
```

Not every command should carry every field. Validation must reject contradictory/cross-scope combinations.

## Proposed commands

Pending ownership ADR:

```text
workspace.create
workspace.rename
workspace.delete
workspace.export
```

If workspace creation is implicit per chat, `workspace.create` may be internal and not public. The protocol should not freeze this until ADR resolves ownership/cardinality.

---

# 15. Artifact/export mapping

Related: **S10 / F09**

No first-class canonical Artifact protocol identity exists today.

## Required model

Artifact is a semantic durable output projection linked to a storage file/version/bundle, not raw bytes in event payloads.

## Proposed commands

```text
artifact.export
artifact.share_prepare   (only if backend preparation is required)
artifact.delete          (if artifact lifecycle differs from storage node deletion)
```

Android system share/save picker launch itself is a platform action and should remain in Kotlin/platform request flow where possible.

## Required snapshot

Artifact projection must expose:

- artifact ID;
- backing storage identity/version;
- type/name/size;
- readiness;
- provenance;
- export history/state if useful;
- available actions.

---

# 16. Models mapping

Related: **S08 / F11**

## Existing commands

```text
model.download
model.select
model.delete
download.cancel
```

## Existing internal projection

Runtime persists model records with statuses such as downloading/installed/verifying/activating/ready/failed.

## Missing public product contract

Android needs authoritative model inventory/catalog/active-model projection.

Proposed snapshot domains/queries:

```text
models.inventory
models.catalog
models.active
```

## Potential command gaps

Only add when backend supports them:

```text
download.pause
download.resume
model.import
model.configure
```

UI MUST NOT display pause/resume merely because the design contains those controls.

---

# 17. Projects mapping

Related: **S07 / F10**

No canonical project commands/snapshots exist on current Kotlin protocol.

Pending `07_STORAGE_WORKSPACE_MODEL.md` + ADR.

Likely required mutations:

```text
project.create
project.rename
project.delete
project.attach_chat
project.detach_chat
project.attach_workspace   (only if model permits)
```

Likely queries/snapshots:

```text
project.index
project.detail
```

Project MUST use explicit IDs in mutating scope. Hidden `last active project` state must not decide file mutation targets.

---

# 18. Settings mapping

Related: **S09**

## Existing

```text
settings.update
```

Public snapshot domain:

```text
settings
```

## Required rules

- settings keys must be allowlisted/typed by domain policy;
- secrets must not travel through generic settings payloads unless specifically designed for secure secret storage;
- provider/API key storage should use Android Keystore/secure backend port and opaque secret references.

Read/write separation is acceptable: snapshot for reads, command for mutations.

---

# 19. Platform request broker

Current runtime advertises platform request broker and Android document/keystore port capabilities.

Use platform requests for operations Android must perform, e.g.:

- system document picker/access;
- share/save destination;
- Android Keystore operation;
- permission/consent requiring platform UI.

## Contract rules

- every request has stable request ID;
- Kotlin processes request once;
- response correlates to same ID;
- recomposition/process recreation must not duplicate external intents blindly;
- request timeout/cancellation is represented explicitly.

---

# 20. Capability negotiation rules

UI feature availability derives from negotiated capabilities.

Example:

```text
CanSendChat = capability contains command:chat.send_message
CanStop = capability contains command:chat.stop_generation AND active operation exists
CanDownloadModel = capability contains command:model.download
```

For proposed feature sets, prefer coarse feature capabilities in addition to individual command capabilities when multiple primitives must exist together.

Examples:

```text
feature:conversation_v1
feature:universal_storage_v1
feature:workspace_v1
feature:artifacts_v1
feature:projects_v1
```

A feature capability MUST only be advertised when its minimum contract is truly usable.

---

# 21. Query/snapshot evolution

Current fixed snapshot-domain enum is too small for the full product.

Two viable designs:

## Option A — expand SnapshotDomainV2

Add domains:

```text
conversations
models
documents
storage
workspaces
artifacts
projects
```

Pros: simple.  
Cons: coarse payloads, pagination/selector pressure.

## Option B — versioned query/snapshot request API

Introduce typed bounded queries:

```text
snapshot/query request
- domain
- selector/id
- pagination cursor
- projection version
```

Pros: scalable and explicit.  
Cons: larger protocol addition.

Recommendation: use Option B or a hybrid before Storage/Projects scale grows. Do not ship unbounded all-files/all-chats snapshots.

---

# 22. Pagination and payload limits

Protocol limits currently bound:

- command envelope to 64 KiB;
- event batch to 512 KiB;
- event count per batch to 256.

Therefore:

- file lists/chat indexes/model catalogs need pagination or bounded selectors;
- binary file content MUST NOT cross as JSON payload;
- large documents/artifacts use storage/object/platform handles;
- snapshots must remain bounded.

---

# 23. Error contract

Backend errors should expose stable machine codes plus safe structured context.

UI maps:

```text
machine code
+ affected domain
+ retryability
+ preserved-work metadata
+ recommended actions
→ human ErrorRecoveryCard/state
```

Avoid flattening all native failures into one string such as `backend_runtime_failed`.

A stable diagnostic code should survive transport while sensitive internals remain in logs/redacted details.

---

# 24. Idempotency policy

Mutations that can duplicate user-visible effects MUST be idempotent/replay-protected.

Examples:

- send message;
- import file;
- model download;
- delete model;
- create project/workspace;
- export preparation if it creates durable artifact records;
- recovery resume/regenerate.

Kotlin should generate stable idempotency key per user intent, not per recomposition/retry attempt.

---

# 25. Minimum protocol plan by milestone

## M1 — Product shell

No major new domain commands required.

Required:

- typed readiness/capability repository;
- stable initialization/recovery mapping.

## M2 — Conversation MVP

Existing mutations mostly sufficient:

- `chat.send_message`;
- `chat.stop_generation`;
- recovery commands.

Required additions:

- authoritative conversation/chat-index query/snapshot;
- structured response projection/events;
- typed activity projection;
- model readiness/inventory query.

## M3 — Storage foundation

Required:

- universal storage domain port;
- bounded storage queries/snapshots;
- import mutation + cancellation/recovery;
- explicit storage scope/node identities.

## M4 — Workspace

Required:

- workspace projection;
- workspace ownership/cardinality ADR;
- explicit workspace scope for mutations;
- file-change/activity projections.

## M5 — Artifacts

Required:

- artifact projection and backing storage identity;
- export/platform request contract.

## M6 — Models

Required:

- model inventory/active projection;
- truthful download/activation events.

## M7 — Projects

Required:

- project model ADR;
- project commands/queries;
- explicit context propagation.

---

# 26. Traceability matrix

| UX action | Current backend | Gap |
|---|---|---|
| Launch app | `app.initialize`, capabilities, application events | typed product readiness projection |
| Send message | `chat.send_message` | public conversation projection/structured streaming |
| Stop generation | `chat.stop_generation` | UI repository/state integration |
| Recover interrupted chat | recovery commands | public conversation/operation reconciliation contract |
| Attach/import file | document grant exists; storage import prototype exists | canonical universal/workspace storage import |
| Inspect workspace | none canonical | workspace query/snapshot + identities |
| Browse Storage | none canonical | paged storage query/snapshot |
| Export artifact | no first-class artifact contract | artifact model + export contract |
| Install model | `model.download` | public model inventory/catalog projection |
| Activate model | `model.select` | public active model projection |
| Manage Projects | none | project domain/protocol |
| Update setting | `settings.update` | typed key policy/secret separation |

---

# 27. Contract acceptance criteria

Before a product feature is implemented in Compose:

- mutations have explicit command or platform-action ownership;
- reads have authoritative bounded query/snapshot source;
- IDs/scopes required for safety are explicit;
- capability gating is defined;
- operation lifecycle/cancellation is defined;
- process-death reconciliation is defined;
- stable error mapping exists;
- idempotency policy is defined;
- payload limits cannot be exceeded by normal feature usage;
- UI does not parse arbitrary native implementation strings as its domain model.
