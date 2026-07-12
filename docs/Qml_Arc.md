# Mukei QML Architecture Specification

**Document:** `Qml_Arc.md`  
**Version:** 1.0  
**Status:** FINAL — Architecture Baseline  
**Scope:** QML frontend architecture, Rust/QML integration, persistent UI state, mobile UX, failure recovery, performance, accessibility, and implementation governance.

---

## 1. Executive Decision

Mukei will use a **Rust-Owned Persistent Reactive Projection Architecture**.

The architecture combines:

- Rust-owned durable domain state
- Feature-scoped reactive ViewModels
- Typed unidirectional commands and events
- Snapshot + delta synchronization
- Explicit lifecycle and navigation state machines
- Persistent UI sessions
- Native Qt list models for large data
- Capability-driven controls
- Central operation and error registries
- A token-driven calm visual and motion system

The core rule is:

> **Rust owns truth, persistence, validation, recovery, capabilities, and long-running operations. QML renders projections, dispatches user intents, and owns only ephemeral visual interaction state.**

This architecture is the baseline for all future QML work. Any structural change requires an Architecture Decision Record and a migration plan.

---

## 2. Primary Goals

The architecture must:

1. Use the existing Rust backend as the authoritative source of truth.
2. Recover cleanly after process death, low-memory eviction, interrupted inference, partial downloads, and incomplete cleanup.
3. Keep QML free from business logic and database logic.
4. Remain responsive with long conversations and many operations.
5. Avoid duplicate, stale, or out-of-order UI state.
6. Support mobile, tablet, and desktop layouts without changing domain logic.
7. Make destructive and security-sensitive actions capability-gated.
8. Provide calm, soft, accessible, and predictable interactions.
9. Preserve privacy: no secrets, prompts, document contents, SAF tokens, or private paths in UI events.
10. Allow features to evolve independently without creating a global-state monolith.

---

## 3. Non-Goals

The QML layer will not:

- Own durable domain truth.
- Open or query SQLite directly.
- Store API keys or database keys.
- Reimplement Rust validation rules.
- Infer backend permissions using local booleans.
- Parse raw Markdown or arbitrary HTML.
- Replay internal tool protocols as visible chat text.
- Maintain a second independent conversation database.
- Use WebView for ordinary message rendering.
- Depend on whole-screen polling.
- Create one QML object per streamed token.
- Use navigation as an uncontrolled push/pop stack.

---

## 4. Architecture at a Glance

```text
┌──────────────────────────────────────────────────────────────┐
│                         QML Views                            │
│  Pages · Components · Animations · Input · Ephemeral State  │
└───────────────────────┬──────────────────────────────────────┘
                        │ User Intent
                        ▼
┌──────────────────────────────────────────────────────────────┐
│                  Feature ViewModels / Stores                 │
│ Chat · Models · Downloads · RAG · Recovery · Settings       │
│ Native list models · Reactive properties · Commands         │
└───────────────────────┬──────────────────────────────────────┘
                        │ Typed Command
                        ▼
┌──────────────────────────────────────────────────────────────┐
│                    Rust Command Gateway                      │
│ Validation · Capabilities · Idempotency · Operation IDs     │
└───────────────────────┬──────────────────────────────────────┘
                        │ Durable Operation
                        ▼
┌──────────────────────────────────────────────────────────────┐
│                    Rust Domain Services                      │
│ Storage · Agent · Models · Downloads · RAG · Audit · Crypto │
└───────────────────────┬──────────────────────────────────────┘
                        │ Domain Event / Snapshot
                        ▼
┌──────────────────────────────────────────────────────────────┐
│                    Projection Engine                         │
│ App state · Timeline · Operations · Errors · Capabilities   │
└───────────────────────┬──────────────────────────────────────┘
                        │ Reactive Qt properties/models
                        ▼
┌──────────────────────────────────────────────────────────────┐
│                         QML Re-render                         │
└──────────────────────────────────────────────────────────────┘
```

---

## 5. Ownership Boundaries

### 5.1 Rust owns

Rust is authoritative for:

- Conversations and branches
- Messages and tool history
- Recovery attempts
- Installed and active models
- Download jobs and reservations
- RAG documents, chunks, cleanup state, and SAF grants
- Preferences and secure secret references
- Audit and security state
- Runtime lifecycle state
- Capabilities
- Long-running operations
- Error classification
- Data migrations
- UI session persistence

### 5.2 QML owns

QML owns only ephemeral view concerns:

- Focus
- Hover and press state
- Animation progress
- Sheet drag offset
- Selection handles
- Temporary tooltip state
- Local transition state
- Uncommitted gesture state

### 5.3 QML must never own authoritative copies of

- Message history
- Model catalogue
- Download status
- Security state
- Recovery state
- Permissions
- Storage quota
- Network policy
- Document cleanup status

---

## 6. State Model

Mukei uses four state classes.

### 6.1 Durable Domain State

Stored in SQLCipher or the backend’s secure persistence layer.

Examples:

- messages
- conversations
- branches
- tool calls and results
- interrupted turns
- model metadata
- download jobs
- RAG documents
- settings
- audit records

### 6.2 Durable UI Session State

Persisted by Rust, versioned, and safe to discard if incompatible.

Examples:

- Last route
- Active conversation
- Active branch
- Timeline anchor message ID
- Per-conversation draft
- Cursor position
- Expanded tool cards
- Selected settings section
- Selected model
- Dismissed notices
- Onboarding progress
- Last safe route
- Drawer state on large screens

Suggested logical records:

```text
ui_session
- profile_id
- schema_version
- active_route
- active_conversation_id
- active_branch_id
- timeline_anchor_message_id
- selected_model_id
- updated_at

ui_feature_state
- profile_id
- feature_key
- state_version
- payload
- updated_at

ui_draft
- conversation_id
- branch_id
- text
- cursor_position
- attachment_refs
- updated_at
```

Rules:

- Draft persistence is debounced.
- Navigation and app pause force an immediate flush.
- Successful send clears the matching draft.
- Failed send preserves it.
- Session payloads are versioned.
- Corrupt or incompatible UI session state falls back safely without affecting domain data.

### 6.3 Runtime Projection State

Derived in Rust from domain state and active operations.

Examples:

- `canSendMessage`
- `canStopGeneration`
- `isDatabaseReady`
- `isModelLoaded`
- `isStorageCritical`
- `isQuarantined`
- `recoveryAvailable`
- `activeOperationCount`

### 6.4 Ephemeral Visual State

QML-only and never persisted.

---

## 7. Root State Hub

The Rust bridge exposes one root `AppStateHub` object with feature-scoped children.

```text
AppStateHub
├── lifecycle
├── navigation
├── capabilities
├── conversations
├── chat
├── models
├── downloads
├── documents
├── recovery
├── settings
├── security
├── operations
└── errors
```

Each child is a focused QObject or QAbstractListModel.

The root object is not a giant mutable map. It is a stable composition point.

Example QML usage:

```qml
ChatPage {
    timelineModel: AppStateHub.chat.timeline
    conversationId: AppStateHub.chat.activeConversationId
    branchId: AppStateHub.chat.activeBranchId
    streaming: AppStateHub.chat.streaming
    canSend: AppStateHub.capabilities.canSendMessage

    onSendRequested: AppStateHub.chat.sendMessage(text)
    onStopRequested: AppStateHub.chat.stopGeneration()
}
```

---

## 8. Unidirectional Interaction Flow

All user actions follow:

```text
View → Intent → Command Gateway → Domain Operation → Event → Projection → View
```

Views must not mutate feature state directly.

### 8.1 Command envelope

```json
{
  "schemaVersion": 1,
  "commandId": "cmd-uuid",
  "operationId": "op-uuid",
  "type": "chat.sendMessage",
  "conversationId": "conv-uuid",
  "branchId": "branch-uuid",
  "payload": {
    "text": "Explain entropy"
  }
}
```

Required command properties:

- Unique command ID
- Optional operation ID
- Explicit feature type
- Schema version
- Conversation/branch scope where applicable
- Validated payload
- Idempotency for destructive or retryable actions

### 8.2 Event envelope

```json
{
  "schemaVersion": 1,
  "eventId": "evt-uuid",
  "sequence": 1042,
  "operationId": "op-uuid",
  "type": "chat.messageUpdated",
  "timestamp": 1783700000,
  "payload": {
    "messageId": "msg-uuid",
    "append": "next chunk"
  }
}
```

Events support:

- Deduplication by event ID
- Ordered application by sequence
- Operation correlation
- Schema evolution
- Snapshot resynchronization

---

## 9. Snapshot + Delta Synchronization

Event-only synchronization is insufficient for mobile suspend/resume.

The protocol is:

```text
First load or recovery:
  Full snapshot

Normal runtime:
  Ordered deltas

Sequence gap:
  Reject delta
  Request fresh snapshot
  Atomically replace projection
```

Example:

```text
Last applied sequence: 140
Incoming sequence: 143
→ 141 and 142 are missing
→ do not apply 143
→ request a fresh feature snapshot
```

Every feature store must expose:

- `snapshotVersion`
- `lastSequence`
- `requestSnapshot()`
- atomic snapshot replacement
- stale delta rejection

---

## 10. Lifecycle State Machine

Lifecycle must be represented by a single explicit state.

```text
Cold
  ↓
Bootstrapping
  ↓
WaitingForKey
  ↓
OpeningDatabase
  ↓
Migrating
  ↓
VerifyingAudit
  ↓
HydratingProjections
  ↓
ReconcilingOperations
  ↓
Ready
```

Failure/degraded states:

```text
KeyRejected
MigrationFailed
AuditQuarantined
StorageUnavailable
ModelUnavailable
RecoveryRequired
SafeMode
Fatal
```

Rules:

- No feature action is accepted before `Ready`, unless explicitly allowed.
- Database pool must not become UI-accessible before audit verification.
- Quarantined state is read-only.
- Route selection is derived from lifecycle state.
- QML never combines several booleans to guess lifecycle.

---

## 11. Navigation Architecture

Navigation is state-driven.

```text
AppShell
├── RouterHost
├── NavigationDrawer
├── TopBarHost
├── BannerHost
├── SnackbarHost
├── SheetHost
├── DialogHost
└── OperationOverlayHost
```

Canonical routes:

```text
boot
unlock
migration
verification
recovery
home
chat/:conversation/:branch
models
downloads
documents
settings
security
diagnostics
```

Rules:

- Screens do not push or pop arbitrary routes.
- Screens dispatch navigation intents.
- NavigationStore validates access.
- Unsafe routes are unavailable during quarantine or incomplete initialization.
- Back navigation is deterministic.
- Unsaved draft handling is centralized.
- Deep links resolve through the same router.

---

## 12. Feature Store Contract

Every feature store defines:

```text
State
Commands
Events
Snapshot
Capabilities
Error policy
Persistence policy
```

Example ChatStore:

```text
State
- activeConversationId
- activeBranchId
- streaming
- activeMessageId
- timeline
- draft
- hasOlderMessages

Commands
- openConversation
- switchBranch
- sendMessage
- stopGeneration
- retryMessage
- resumeInterruptedTurn
- regenerateInterruptedTurn
- loadOlderMessages

Events
- conversationOpened
- messageInserted
- messageUpdated
- toolCallCommitted
- toolResultCommitted
- generationCompleted
- generationInterrupted
- generationFailed
```

---

## 13. Chat Timeline

The chat timeline uses:

- `QAbstractListModel`
- `ListView`
- `DelegateChooser`
- Stable row IDs
- Pagination
- Delegate reuse
- Controlled cache buffer
- Single-row streaming updates

Recommended row types:

```text
UserMessage
AssistantMessage
ThinkingSummary
ToolCall
ToolResult
SystemNotice
ErrorNotice
RecoveryNotice
BranchMarker
DateSeparator
```

### 13.1 Streaming

Wrong:

```text
Every chunk → create a new QML item
```

Correct:

```text
Stream starts
→ one assistant row inserted

Chunks arrive
→ Rust coalesces for approximately 32–60 ms
→ existing row updated by message ID

Stream completes
→ row status becomes completed
```

Rules:

- No per-token QML object creation.
- No full-list replacement for one changed row.
- Internal tool protocol text never enters visible assistant text.
- Timeline row status is explicit: pending, streaming, completed, failed, cancelled, interrupted.
- Auto-scroll occurs only when the user is already near the bottom.
- If the user scrolls upward, show a gentle “new response” affordance instead of forcing position.

### 13.2 Pagination

- Load recent 50–100 timeline rows initially.
- Fetch older rows using a stable anchor ID.
- Preserve scroll position when prepending.
- Never load a complete huge conversation into QML.

### 13.3 Rich content

- Rust converts Markdown into a validated structural representation.
- QML renders approved blocks.
- Raw HTML is not rendered.
- Code blocks are lazy-rendered.
- Syntax highlighting is deferred until visible.
- Large tool outputs are collapsed and loaded on demand.

---

## 14. Persistent Drafts

Drafts are scoped by conversation and branch.

Rules:

- Debounce writes by 300–500 ms.
- Flush immediately on route change, app pause, and branch switch.
- Preserve draft after failed or cancelled send.
- Clear only after the user message is durably committed.
- Attachment references are persisted, not raw attachment bytes.
- Draft restoration never automatically sends content.

---

## 15. Operation Registry

All long-running work is represented by the central OperationStore.

Operation types:

- Inference
- Model download
- RAG indexing
- Document cleanup
- Migration
- Export
- Model deletion
- Database backup
- Recovery attempt

Operation fields:

```text
id
type
state
phase
progress
cancelable
retryable
startedAt
lastUpdatedAt
safeError
relatedEntityId
```

Rules:

- Operations survive navigation.
- Durable operations survive process death.
- App resume reconciles backend jobs with UI projections.
- Cancellation is capability-gated.
- Operation completion is idempotent.
- A global operation indicator never blocks unrelated UI unless required.

---

## 16. Capability-Driven UI

QML never infers whether an operation is safe.

Examples:

```text
canSendMessage
canStopGeneration
canSwitchConversation
canDeleteConversation
canInstallModel
canCancelDownload
canAttachDocument
canUseRemoteProvider
canOpenSettings
canExportData
canRetryRecovery
```

Example:

```qml
SoftIconButton {
    enabled: AppStateHub.capabilities.canStopGeneration
    visible: AppStateHub.chat.streaming
}
```

Capabilities are derived from:

- Lifecycle
- Security state
- Model readiness
- Storage quota
- Network policy
- Active operations
- Recovery state
- Database availability
- User preferences

---

## 17. Error Architecture

Errors are typed and projected through a central ErrorStore.

Error fields:

```text
code
severity
recoverable
safeMessage
suggestedAction
operationId
feature
timestamp
```

Presentation policy:

```text
Field validation       → inline
Temporary failure      → snackbar
Degraded condition     → banner
Confirmation required  → modal
Blocking failure       → blocking page
Security failure       → quarantine page
```

Rules:

- Raw internal errors never cross into QML.
- Paths, tokens, keys, prompts, and document contents are redacted.
- Duplicate errors are coalesced.
- Repeated transient errors are rate-limited.
- Errors are associated with operation IDs when possible.

---

## 18. Security and Privacy Rules

QML must never receive:

- Database keys
- API keys
- Raw SAF tokens
- Raw Android content URIs unless strictly necessary
- Absolute app-private paths
- Full SQL error text
- Raw provider response bodies
- Hidden prompts
- Private document contents outside approved projections

All bridge-facing errors and events pass through a redaction layer.

Security state is explicit:

```text
Normal
Degraded
ReadOnly
Quarantined
Fatal
```

---

## 19. Visual Design System

The visual system must feel:

- Calm
- Soft
- Clean
- Capable
- Private
- Crafted
- Non-distracting

### 19.1 Token layers

```text
Foundation
- colors
- typography
- spacing
- radii
- motion
- elevation
- icon metrics

Primitives
- text
- icon
- surface
- divider
- focus ring

Controls
- buttons
- text fields
- toggles
- chips
- menu items

Patterns
- message bubbles
- tool cards
- operation cards
- banners
- empty states

Screens
- compose patterns only
```

Screens must not define one-off visual systems.

### 19.2 Motion tokens

```text
Immediate feedback: 90–120 ms
Micro transition:   140–180 ms
Content change:     180–240 ms
Sheet/dialog:       240–300 ms
```

Preferred easing:

- OutCubic
- InOutCubic
- OutQuart

Avoid:

- Strong bounce
- Large overshoot
- Flashing
- Aggressive shimmer
- Continuous glow
- Large parallax
- Multiple competing animations

Reduced-motion mode:

- Scale animations disabled
- Slides replaced with fades
- Decorative loops disabled
- Maximum duration approximately 80 ms

### 19.3 Press feedback

```text
Scale: 1.0 → 0.975 → 1.0
Duration: 80–110 ms
```

Use only for small controls. Large surfaces use tonal feedback instead.

---

## 20. Icon System

All icons use a single component:

```qml
MukeiIcon {
    name: "send"
    size: IconSize.standard
    tone: IconTone.primary
}
```

Rules:

- 20 px compact grid
- 24 px standard grid
- Consistent rounded stroke
- Rounded caps and joins
- No inconsistent mix of filled and outlined icons
- RTL-aware directional icons
- Disabled state uses opacity
- Semantic color reserved for status
- Icons include accessible names where they carry meaning

---

## 21. Accessibility

Mandatory requirements:

- Every interactive control has an accessible role and name.
- Custom controls inherit from appropriate Qt Quick Controls where possible.
- Keyboard activation supports Enter and Space.
- Focus order is deterministic.
- Visible focus ring is always available.
- Minimum touch target is 44 × 44 logical pixels.
- Text layouts support at least 200% scaling.
- No fixed-height container may clip text.
- RTL mirroring is tested.
- Color is never the only state indicator.
- Reduced motion is respected globally.
- Screen readers receive batched streaming announcements, not token-by-token noise.

---

## 22. Responsive Layout

Breakpoints are semantic, not device-name based.

Suggested modes:

```text
Compact
- Single pane
- Overlay drawer
- Bottom sheets

Medium
- Wider single pane
- Optional side panels

Expanded
- Conversation list + chat split view
- Persistent drawer
- Detail panel where useful
```

Domain and feature stores remain unchanged across layouts.

---

## 23. Performance Rules

Mandatory:

- Native list models for large collections
- Stable row IDs
- Paginated data
- Delegate reuse
- Bounded cache buffers
- Batched stream updates
- No full JSON parsing inside delegates
- No full-list replacement for single-item changes
- Heavy Markdown and syntax work outside the UI thread
- Lazy tool-result rendering
- Lazy code highlighting
- No large blur effects over scrolling surfaces
- No infinite animation on low-power/reduced-motion mode
- Image and SVG dimensions constrained
- App resume uses snapshots rather than replaying unbounded history

### 23.1 Performance budgets

Target budgets:

```text
Initial shell visible:          < 500 ms after QML engine start
Common route transition:        60 FPS target
Streaming UI update batch:      32–60 ms
Input-to-visual response:       < 100 ms
Timeline initial page:          50–100 rows
QML objects per chat viewport:  bounded by visible delegates + cache
```

These are design targets, not guarantees; profiling decides final thresholds.

---

## 24. Worst-Case Behaviour

| Scenario | Required response |
|---|---|
| App killed during inference | Restore interrupted row and recovery action |
| Duplicate backend event | Ignore by event ID |
| Missing event sequence | Request fresh snapshot |
| Database quarantined | Show read-only security route |
| Model removed externally | Reconcile and show model-unavailable state |
| Low storage | Disable unsafe actions and surface cleanup |
| Download interrupted | Restore durable operation and reservation |
| Vector cleanup incomplete | Persistent retryable operation |
| Backend unavailable | Preserve state and disable capabilities |
| Huge conversation | Paginated virtual timeline |
| 200% font scale | Flexible layouts with no clipping |
| RTL language | Mirrored layout and directional icons |
| Slow device | Batched updates and reduced motion |
| Corrupt UI session | Reset UI session only; preserve domain data |
| Invalid stale command | Reject by command/version/idempotency rules |
| App resumes after long pause | Hydrate snapshots and reconcile operations |

---

## 25. Folder Structure

```text
qml/
├── App.qml
├── architecture/
│   ├── AppCoordinator.qml
│   ├── IntentDispatcher.qml
│   ├── SnapshotController.qml
│   ├── RouteController.qml
│   └── PresentationPolicy.qml
├── shell/
│   ├── AppShell.qml
│   ├── RouterHost.qml
│   ├── DrawerHost.qml
│   ├── SheetHost.qml
│   ├── DialogHost.qml
│   ├── BannerHost.qml
│   ├── SnackbarHost.qml
│   └── OperationOverlayHost.qml
├── design/
│   ├── Theme.qml
│   ├── ColorTokens.qml
│   ├── TypeTokens.qml
│   ├── SpacingTokens.qml
│   ├── RadiusTokens.qml
│   ├── MotionTokens.qml
│   ├── ElevationTokens.qml
│   └── IconTokens.qml
├── primitives/
├── controls/
├── patterns/
├── features/
│   ├── lifecycle/
│   ├── conversations/
│   ├── chat/
│   ├── models/
│   ├── downloads/
│   ├── documents/
│   ├── recovery/
│   ├── settings/
│   ├── security/
│   └── diagnostics/
└── tests/
```

Recommended Rust bridge structure:

```text
mukei-bridge/src/ui/
├── app_state_hub.rs
├── command_gateway.rs
├── event_envelope.rs
├── snapshot_protocol.rs
├── lifecycle_projection.rs
├── navigation_projection.rs
├── capability_projection.rs
├── chat_projection.rs
├── conversation_projection.rs
├── model_projection.rs
├── operation_projection.rs
├── recovery_projection.rs
├── settings_projection.rs
├── security_projection.rs
├── error_projection.rs
└── ui_session_repository.rs
```

---

## 26. Forbidden Patterns

The following are architecture violations:

- QML screen calling SQL or filesystem code
- Business logic inside delegates
- Direct mutation of feature projection models from QML
- Multiple competing sources of truth
- Random untyped signal names for domain events
- Raw JSON parsing in every screen
- Global mutable QML singleton containing all app state
- Per-token signal and object creation
- Full history loaded at startup
- Screens directly controlling route stacks
- UI permissions derived from local booleans
- Raw technical errors displayed to the user
- Persistent secrets in QML properties
- Hardcoded private filesystem paths
- Visual tokens defined separately in each screen
- Placeholder tests that always pass

---

## 27. Testing Strategy

### 27.1 Store and projection tests

Test:

- Snapshot application
- Delta application
- Sequence gaps
- Duplicate events
- Capability changes
- Route derivation
- Recovery hydration
- Operation reconciliation
- Draft persistence

### 27.2 QML component tests

Test:

- Keyboard activation
- Accessible roles and names
- Focus order
- RTL
- 200% font scaling
- Reduced motion
- Touch target sizes
- Dynamic theme changes
- Error presentation
- Timeline delegate types

### 27.3 Performance tests

Test:

- 10,000-message synthetic conversation
- Rapid streaming deltas
- Large tool result expansion
- Multiple simultaneous operations
- Resume after long suspension
- Low-memory delegate destruction and recreation
- Model download progress updates

### 27.4 Contract tests

Rust and QML must share contract fixtures for:

- Command schemas
- Event schemas
- Snapshot schemas
- Error codes
- Capability names
- Route names
- Timeline row roles

---

## 28. Rollout Plan

### Phase 1 — Foundation

- Add AppStateHub
- Add typed command gateway
- Add event envelope and snapshot protocol
- Add lifecycle state machine
- Add state-driven router
- Add capability store
- Add persistent UI session repository
- Add design tokens and global presentation hosts

### Phase 2 — Chat

- Replace demo timeline with native model
- Add paginated history
- Add streaming row updates
- Add draft persistence
- Add branch support
- Add tool timeline
- Add recovery UI
- Add auto-scroll policy

### Phase 3 — Models, Downloads, and Documents

- Real model catalogue projection
- Download operation cards
- Storage pressure UI
- RAG document list
- Cleanup and tombstone states
- Safe destructive actions

### Phase 4 — Settings, Security, and Diagnostics

- Persistent settings projections
- Security and quarantine screens
- Diagnostics projection
- Privacy-safe export
- Full accessibility pass
- Tablet and desktop responsive layouts

### Phase 5 — Hardening

- Replace placeholder tests
- Add performance benchmarks
- Add lifecycle stress tests
- Add sequence-gap and snapshot-recovery tests
- Profile memory and frame timing
- Validate emulator and physical devices

---

## 29. Acceptance Criteria

The architecture is considered implemented when:

1. QML does not own durable business state.
2. Every feature uses a scoped store/ViewModel.
3. All domain actions use typed commands.
4. All projections support snapshot recovery.
5. Navigation is derived from state.
6. Capabilities gate all sensitive actions.
7. Chat uses a native paginated timeline model.
8. Streaming updates one existing row.
9. Drafts survive process death.
10. Recovery state is visible and actionable.
11. Long-running operations survive navigation and restart.
12. Raw technical errors and secrets never reach QML.
13. Icons are served through a unified icon component.
14. Reduced motion, RTL, keyboard use, and 200% text scaling pass tests.
15. Large-conversation performance remains bounded.
16. Placeholder QML tests are removed.
17. The same feature stores support compact and expanded layouts.
18. Architecture rules are enforced in code review.

---

## 30. Architecture Governance

This document is the frozen baseline.

Structural changes require an Architecture Decision Record containing:

- Problem
- Proposed change
- Alternatives considered
- Compatibility impact
- Persistence migration impact
- QML/Rust contract impact
- Performance impact
- Security impact
- Rollout plan
- Rollback plan

Rules:

- No new global singleton without architectural review.
- No new persistent UI state without schema/version ownership.
- No new event type without schema versioning.
- No feature bypasses the command gateway.
- No feature creates an independent navigation system.
- No existing durable contract is changed without migration support.

---

## 31. Final Architecture Statement

Mukei’s QML frontend will be built as:

> **A Rust-owned persistent reactive projection system with feature-scoped QML ViewModels, typed unidirectional commands and events, snapshot-plus-delta synchronization, explicit lifecycle and navigation state machines, durable UI sessions, native virtualized timeline models, capability-driven interaction, centralized operations and errors, and a calm token-based visual system.**

Responsibility summary:

```text
Rust
- truth
- persistence
- validation
- security
- capabilities
- recovery
- operations
- projections

QML ViewModels
- typed commands
- reactive properties
- native models
- snapshot coordination

QML Views
- rendering
- input
- animation
- focus
- ephemeral interaction
```

This architecture is designed to remain stable while Mukei grows from a mobile local-AI application into a reliable multi-platform product.


---

## 32. Phase 6 Addendum — Contract and Recovery Boundary

The following rules are now part of the frozen architecture baseline.

### 32.1 Contract negotiation precedes private storage

The QML bundle and Rust bridge must negotiate a versioned UI contract before encrypted storage, persisted sessions, conversations, or private documents are opened. The handshake includes:

- UI contract version and accepted QML version range
- command schema version
- event schema version
- snapshot schema version
- required architectural feature flags

An incompatible or malformed contract fails closed and routes to a blocking compatibility screen. Retry is allowed, but no feature command may bypass the contract gate.

### 32.2 Mixed bundles are not partially supported

Frontend and bridge artifacts are released as a compatible set. Unknown required features, unsupported schema versions, or a QML version outside the bridge range must not be treated as degraded mode. They are blocking compatibility failures.

### 32.3 Resume uses authoritative operation snapshots

On application resume, QML rehydrates feature snapshots and requests a privacy-safe operation snapshot from Rust. The operation projection covers durable downloads, document cleanup, and document ingestion. QML may reconstruct state from feature stores only as a compatibility fallback; Rust remains authoritative.

### 32.4 Active and blocked operations are distinct

An operation waiting for an unavailable subsystem, such as a document embedder, is represented as `blocked`, not as actively progressing. Blocked operations remain visible and retryable where supported, but they do not trigger a misleading global activity indicator.

### 32.5 Contract evolution governance

Any change to command, event, snapshot, role, route, or required-feature semantics must:

1. update the relevant schema or contract version;
2. add shared fixture/guard coverage;
3. document compatibility and migration behavior; and
4. preserve fail-closed startup for unsupported combinations.
