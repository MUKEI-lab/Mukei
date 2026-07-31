# 05 — Interaction State Model

Status: **Draft v0.1**

This document defines the explicit user-visible and lifecycle state machines required by the Android product.

Its purpose is to prevent UI behavior from degenerating into unrelated booleans, duplicated state ownership, or optimistic screens that disagree with the Rust runtime.

## Core rule

Every meaningful interaction must have:

1. a single authoritative state model;
2. explicit transitions;
3. defined failure/cancellation/recovery behavior;
4. a truthful projection after process death;
5. a mapping to backend commands/events/snapshots where applicable.

The exact Protocol V2 mapping is specified in `06_UI_BACKEND_CONTRACT.md`.

---

# 1. State taxonomy

Mukei UI state is divided into four categories.

## A. Durable domain state

State that survives process death because it is persisted/authoritative outside the current Compose tree.

Examples:

- chat/message history;
- project membership;
- workspace files;
- artifacts;
- installed models;
- storage metadata;
- completed operation results.

## B. Runtime/session state

Authoritative while the native/backend runtime is alive and reconstructable from snapshot/event state.

Examples:

- active generation;
- active download;
- current platform request;
- runtime readiness;
- model activation in progress;
- active task progress.

## C. UI interaction state

Ephemeral presentation state owned by the current screen/navigation layer.

Examples:

- drawer open;
- activity details expanded;
- selected tab/filter;
- search query;
- composer focus;
- sheet/dialog visibility.

## D. Recoverable UI draft state

User-authored temporary state that may deserve SavedState/persistence even before backend submission.

Examples:

- composer draft;
- selected attachments not yet sent;
- pending project picker selection;
- unsent search/filter text where preserving it improves UX.

These categories MUST NOT be conflated.

---

# 2. Authority and reconciliation

The UI must not treat its own previous rendering as authoritative after process death.

General recovery order:

```text
Process/screen recreated
  ↓
Restore safe UI-only draft/navigation state
  ↓
Reconnect/recreate runtime as required
  ↓
Load durable repository/domain projection
  ↓
Load authoritative runtime snapshot/capabilities
  ↓
Reconcile active/transient operation state
  ↓
Render truthful UI
```

If a previous UI claimed `Running` but no authoritative operation exists after recovery, the UI MUST transition to a truthful recovered terminal/unknown state rather than fabricate progress.

A dedicated ADR must resolve the canonical reconciliation strategy between Rust snapshot, Kotlin persistence, and durable database projections.

---

# 3. Anti-pattern: boolean soup

Avoid models such as:

```text
isLoading
isGenerating
isStopping
hasError
isWorkspaceLoading
isExporting
isRetrying
```

when multiple values can become logically contradictory.

Prefer explicit algebraic/sealed states.

Conceptual example:

```text
ConversationOperationState
- Idle
- Submitting
- Running
- Cancelling
- Completed
- Cancelled
- Failed
- Recovering
```

State-specific data belongs inside the corresponding state.

---

# 4. Global app readiness state

Related screen: **S00**  
Related flows: **F01, F13**

Readiness is multidimensional, not a single `backendReady` flag.

## Model

```text
AppReadiness
- Shell
- SecureRuntime
- EncryptedStorage
- Inference
- ProviderNetwork
```

Each capability may be:

```text
Unknown
Starting
Ready
Unavailable(reason)
Failed(recoverable?, diagnosticCode?)
```

Inference may use richer substate:

```text
InferenceReadiness
- Unknown
- NoArtifacts
- InstalledNoActiveModel
- Activating
- Ready(modelId)
- Incompatible(reason)
- Failed(reason)
```

## Invariants

1. `SecureRuntime=Ready` + `Inference=NoArtifacts` is a valid usable app state.
2. Missing provider/network MUST NOT block purely local surfaces.
3. Storage failure may block storage-dependent actions without necessarily blocking Settings/diagnostics.
4. UI copy must describe the affected capability, not collapse everything into `Backend unavailable`.

---

# 5. Navigation shell state

Related screens: **S01, S02**

## Model

```text
ShellState
- currentTopLevelDestination
- drawerState: Closed | Opening | Open | Closing
- activeTransientSurface?
```

Top-level destinations:

```text
Home
Storage
Projects
Models
Chats/Conversation context
Settings
```

## Invariants

- Drawer open is UI-only state.
- Selecting the currently active destination must not duplicate back-stack entries.
- Back closes modal/transient surfaces before leaving the current top-level destination.
- Running backend operations are not automatically cancelled by navigation unless a specific operation contract requires it.

---

# 6. Home composer state

Related screen: **S01**  
Related flows: **F02, F04, F07**

## Model

```text
ComposerState
- draftText
- attachments[]
- selectedCapabilityHint?
- validation
- submissionState
```

Submission state:

```text
Ready
Blocked(reason)
Submitting(localRequestId)
```

## Transition model

```text
Empty
  ├─ type → Drafting
  ├─ attach → DraftingWithAttachments
  └─ select hint → Empty/HintSelected

Drafting
  ├─ send → Submitting
  ├─ attach → DraftingWithAttachments
  └─ clear → Empty

Submitting
  ├─ accepted → Conversation operation Running
  ├─ rejected → Draft restored + actionable error
  └─ transport/runtime failure → Draft preserved where safe
```

## Invariants

- Send cannot create duplicate commands from repeated taps while submission is pending.
- Draft SHOULD remain recoverable when submission fails before authoritative acceptance.
- Once accepted, the canonical user message/operation comes from domain/backend projection, not a permanently separate optimistic duplicate.
- Capability hint is context, not a mandatory mode.

---

# 7. Conversation operation state

Related screen: **S03**  
Related flows: **F03, F04, F05, F06, F12, F13**

## State machine

```text
Idle
  ↓ Send
Submitting
  ├─ rejected → Failed/Idle with draft recovery
  ↓ accepted(operationId)
Running
  ├─ response/activity events → Running(updated projection)
  ├─ Stop → Cancelling
  ├─ terminal success → Completed
  └─ terminal failure → Failed

Cancelling
  ├─ cancellation confirmed → Cancelled
  ├─ operation finishes first → Completed/Failed
  └─ reconciliation required → Recovering

Recovering
  ├─ authoritative active op found → Running/Cancelling
  ├─ terminal record found → Completed/Cancelled/Failed
  └─ no safe determination → Interrupted/Failed with recovery guidance
```

`Interrupted` MAY be represented as a specialized failure/recovery state rather than a public enum if backend semantics support a better terminal state.

## Running data

A Running state should contain identifiers/projection references such as:

```text
operationId
phase
responseProjection
activitySummary
controlsAvailable
startedAt
```

Do not store an ever-growing duplicate event log in Compose state if the repository/runtime owns the authoritative projection.

## Invariants

- Stop visible only when supported.
- Stop becomes disabled/replaced while `Cancelling`.
- A completion event wins over stale local loading flags.
- Navigation away does not imply cancellation.
- Re-entering conversation derives operation state from authoritative projection.

---

# 8. Response streaming/render state

Streaming is a projection concern separate from the operation lifecycle.

## Model

```text
ResponseRenderState
- Empty
- Partial(chunks/structuredBlocks, isFollowing)
- Final(structuredContent)
- FailedPartial(structuredContent?, failure)
```

## Invariants

- Final content must replace/converge with partial content without duplication.
- Scroll-follow state is UI-only and must not affect operation execution.
- User reading older content sets `isFollowing=false`; incoming chunks must not force-scroll.
- `new response below` indicator appears based on scroll/projection state, not backend operation semantics.

---

# 9. Activity state

Related screens: **S03, S04**

## Domain activity projection

```text
ActivityState
- NotStarted
- Active(groups[], summary, controls)
- WaitingForUser(request)
- Completed(summary)
- Failed(summary, preservedWork, retryability)
- Cancelled(summary)
```

Activity group:

```text
ActivityGroup
- category
- summary
- items[]
- progress? (real only)
```

Item state:

```text
Pending
Running
Succeeded
Failed
Skipped
Cancelled
NeedsApproval
```

## UI expansion state

`Collapsed | Expanded` is UI-only and MUST NOT be stored as operation domain state.

## Invariants

- Parallel operations are grouped rather than dumped into the main conversation.
- Percent progress is used only when backed by meaningful measurable progress.
- `WaitingForUser` must surface approval/control prominently enough to unblock operation.

---

# 10. Workspace lifecycle state

Related screen: **S05**  
Related flows: **F06, F08, F10, F12**

The exact ownership relationship is deferred to ADR/`07_STORAGE_WORKSPACE_MODEL.md`, but UI lifecycle requires explicit states.

## Model

```text
WorkspaceLifecycle
- Absent
- Creating
- Ready
- Mutating(activeOperationIds)
- PartialFailure(preservedState, failedOperation)
- Exporting(exportOperationId)
- RecoveryRequired(reason)
- Deleting
- Deleted
```

Workspace file state is separate from workspace lifecycle.

## File state

User-visible semantic state may derive from richer backend metadata:

```text
Created
Edited
Read
Imported
Exported
NeedsReview
Failed
Locked
LocalOnly
```

A file can be durable and `Created` even when the broader build operation is `Failed`.

## Invariants

- Workspace failure MUST NOT hide successfully committed files.
- UI must distinguish `workspace absent` from `workspace loading`.
- Export failure does not mutate workspace into failed/deleted state.
- Deletion scope must be known before confirmation.

---

# 11. File import / ingestion state

Related screens: **S01, S03, S06**  
Related flow: **F07**

Import is explicitly multi-stage because storage success and indexing/readiness can diverge.

## State machine

```text
Selected(sourceUri)
  ↓ stage/copy
Staging
  ↓ validate
Validating
  ├─ invalid/unsupported/oversized → FailedBeforeCommit
  ↓ durable commit
Stored(fileId)
  ├─ no ingestion required → Ready
  ↓ ingestion/indexing
Indexing
  ├─ success → Ready
  └─ failure → StoredButIndexingFailed
```

Cancellation may occur where supported:

```text
Staging/Validating → Cancelling → Cancelled
```

Once durable storage commit occurs, cancellation of later indexing MUST NOT pretend the file was never stored.

## Invariants

- `StoredButIndexingFailed` remains discoverable in Storage.
- UI must not show ghost attachment as ready if import failed before commit.
- Permission/source revocation errors preserve clear original/copy semantics.
- Oversized/unsupported errors are human-readable.

---

# 12. Artifact state

Related screen: **S10**  
Related flows: **F06, F09**

## Model

```text
ArtifactLifecycle
- Generating
- Ready(artifactId)
- Exporting(exportId)
- ReadyWithLastExport(exportMetadata)
- ExportFailed(artifactStillReady, error)
- Missing/Deleted
```

## Invariants

- `ExportFailed` does not mean artifact generation failed.
- Export/share does not silently delete internal copy.
- Artifact identity must remain stable enough for later retrieval/re-export.

---

# 13. Export state

## Model

```text
ExportState
- Idle
- Preparing
- AwaitingDestination/ExternalAction
- Writing
- Completed(destinationMetadata)
- Failed(error, internalArtifactPreserved)
- Cancelled
```

## Invariants

- External picker cancellation is not necessarily an error.
- Completion only shown after destination/action confirms success where confirmation is available.
- Internal artifact/workspace preservation is explicit after failure.

---

# 14. Project context state

Related screen: **S07**  
Related flow: **F10**

## Model

```text
ProjectContext
- None
- Selected(projectId)
- Resolving(projectId)
- Active(projectId, allowedWorkspaceTargets)
- Invalid/Unavailable(reason)
```

## Invariants

- Active project context is visibly indicated where it affects operations.
- Mutating commands must carry explicit target/context identifiers rather than rely only on hidden last-opened project state.
- Project selection failure must not silently fall back to another project/workspace.

---

# 15. Model lifecycle state

Related screen: **S08**  
Related flows: **F03, F11**

## Catalog/install state

```text
ModelInstallState
- Available
- Queued
- Downloading(bytesDone, bytesTotal?)
- Paused
- Verifying
- Installed
- Incompatible(reason)
- Failed(error, retryability)
- Deleting
```

## Activation state

```text
ModelActivationState
- NoneActive
- Activating(modelId)
- Active(modelId)
- ActivationFailed(modelId, error)
```

Install and activation are separate.

## Invariants

- Installed does not imply Active.
- Backend Ready does not imply Active model.
- Progress percentage only when bytes total is known/reliable.
- Pause/Resume controls only when backend supports the transition.
- Delete active model must define deactivation behavior explicitly.

---

# 16. Error/recovery state

Related flow: **F12**

Errors are typed by user consequence, not only technical origin.

## User-facing recovery model

```text
RecoveryState
- BlockingFailure
- PartialFailure(preservedWork)
- CapabilityUnavailable(requiredCapability)
- PermissionRequired(scope)
- RetryableFailure(retryAction)
- NonRetryableFailure(nextBestActions)
- DiagnosticOnly(code/details)
```

A single failure may combine consequence + diagnostic metadata.

## Error card contract

Must answer:

1. What failed?
2. What is preserved?
3. What can the user do next?
4. Is cleanup/action required?

## Invariants

- Do not expose raw throwable text as primary copy.
- Do not show a Retry button unless replay semantics are safe.
- Do not claim rollback if partial files remain.
- Diagnostics must redact secrets and sensitive provider data.

---

# 17. Approval/platform-request state

Long-running native operations may require Android/platform/user interaction.

## Model

```text
PlatformRequestState
- None
- Pending(requestId, kind, explanation)
- InProgress(requestId)
- Responded(requestId)
- Declined(requestId)
- Expired/Failed(requestId)
```

Examples:

- document picker/access grant;
- provider permission/consent;
- export destination;
- destructive approval.

## Invariants

- Requests are correlated by stable request IDs.
- Recomposition cannot launch the same external intent repeatedly.
- Process recreation must reconcile pending requests rather than duplicate them blindly.

---

# 18. Process death and restart

Related flow: **F13**

## Scenario A — no active operation

```text
Recreate
→ restore navigation/draft safely
→ load durable conversation/workspace projection
→ render Ready
```

## Scenario B — process-scoped native operation cannot survive death

```text
Recreate
→ durable work loaded
→ operation no longer active
→ derive interrupted/recovery state
→ explain preserved work + valid next actions
```

## Scenario C — operation is durably resumable/recoverable

```text
Recreate
→ runtime starts
→ recovery journal/snapshot resolves operation
→ UI shows Recovering
→ operation resumes or terminal state is published
```

The UI MUST NOT assume which scenario applies without authoritative evidence.

---

# 19. SavedState vs repository guidance

Use `SavedStateHandle` / navigation saved state for lightweight UI state such as:

- draft text identifiers/content subject to privacy policy;
- selected filter/tab;
- search query;
- transient destination arguments.

Do not use it as authoritative storage for:

- completed messages;
- workspace files;
- model installation;
- active native operation truth;
- artifact existence.

Those belong to repository/domain/runtime sources.

---

# 20. State ownership matrix

| State | Primary owner | Survives process death? | UI restores from |
|---|---|---:|---|
| Drawer open | Compose/shell | No | default closed |
| Composer draft | ViewModel/SavedState or draft repo | Prefer yes | saved draft |
| Chat messages | Domain repository/backend | Yes | durable projection |
| Active generation | Native/runtime + operation repo | Depends | runtime snapshot/recovery |
| Activity expanded | UI | No/optional | default collapsed |
| Workspace files | Storage/domain | Yes | durable storage projection |
| Artifact | Storage/domain | Yes | durable artifact projection |
| Model installed | Model repository/runtime | Yes | model inventory |
| Model active | Runtime/domain | Reconcile | runtime snapshot/config |
| Export picker open | UI/platform | No | reconcile/cancel safely |
| Selected project context | Navigation/domain context | Prefer yes | explicit route/context projection |

---

# 21. Compose modeling guidance

Feature ViewModels SHOULD expose one coherent immutable `UiState` per screen/feature rather than many independently mutable `StateFlow<Boolean>` values.

Conceptual pattern:

```kotlin
sealed interface OperationState {
    data object Idle : OperationState
    data class Running(val operationId: String, val phase: Phase) : OperationState
    data class Cancelling(val operationId: String) : OperationState
    data class Failed(val failure: UiFailure) : OperationState
}

data class ConversationUiState(
    val content: ConversationProjection,
    val composer: ComposerUiState,
    val operation: OperationState,
    val transient: ConversationTransientUiState,
)
```

This is illustrative, not a mandatory exact API.

## Event handling rule

One-off UI effects (open picker, show share sheet, request focus) must not be represented as sticky state that re-fires on every recomposition.

Use an explicit effect/event mechanism or consumed request identity consistent with Android architecture decisions.

---

# 22. Transition validation

Invalid transitions should be impossible or rejected explicitly.

Examples:

- `Idle → Cancelling` is invalid.
- `Available model → Active` without install/remote availability resolution is invalid.
- `FailedBeforeCommit import → Ready` without a new retry/commit is invalid.
- `Deleted workspace → Mutating` is invalid.

Backend/repository adapters SHOULD normalize impossible/stale event sequences into diagnostics rather than expose contradictory state to Compose.

---

# 23. Testing requirements derived from state machines

Every state machine must produce tests for:

1. happy-path transitions;
2. duplicate action suppression;
3. cancellation race with completion;
4. failure after partial durable work;
5. process recreation/reconciliation;
6. stale/out-of-order event handling where relevant;
7. navigation away/back during running operation;
8. accessibility visibility of actionable states.

These cases feed `10_TEST_ACCEPTANCE_PLAN.md`.

---

# 24. First implementation slice state scope

For M1–M2, the minimum state models to implement completely are:

```text
AppReadiness
Shell/DrawerState
ComposerState
ConversationOperationState
ResponseRenderState
ActivityState (basic)
Model/InferenceReadiness routing
Startup/ErrorRecovery
```

Workspace/storage/model download state models may initially be repository stubs only if their screens are not yet shipped, but their future semantics must not be contradicted by temporary UI shortcuts.

## Completion criterion

A feature is state-model complete when:

- every visible state has an explicit source;
- every user action has a valid transition;
- duplicate/invalid actions are suppressed;
- failure says what is preserved;
- process recreation has defined reconciliation behavior;
- UI cannot represent mutually contradictory combinations;
- acceptance tests can be directly derived from the transition table/state diagram.
