# Mukei QML Architecture — Phase 4

Phase 4 builds on the cumulative Phase 1–3 architecture and connects the remaining product-facing management flows without moving domain authority into QML.

## Implemented

### Model lifecycle UI

- Added typed bridge commands to validate/select an installed catalogue model.
- Selection is explicitly for the next engine session; the UI does not pretend that a live engine was hot-swapped.
- Added guarded model deletion:
  - catalogue ID validation
  - inference-busy rejection
  - active-download rejection
  - canonical app-private path containment
  - idempotent missing-file response
- Added destructive confirmation and capability-driven controls in Model Manager.

### Private document management

- Added privacy-safe document grant and revoke commands.
- UI receives a one-way `document_id`, never a raw SAF token.
- Durable revoke reuses SQL chunk deletion, audit linkage, vector cleanup, and retryable tombstone handling.
- File picker results are validated by the Rust bridge as `content://` or `file://` targets.
- UI wording remains honest: access registration is distinct from successful native permission persistence and document indexing.

### Diagnostics

- Added a privacy-safe diagnostics projection and export flow.
- Snapshot excludes prompts, document contents, keys, tokens, provider responses, and private paths.
- Export returns an opaque ID and filename only.
- Export uses an atomic temporary-write, `sync_all`, and rename sequence.

### Responsive chat

- Expanded layouts now show a persistent conversation pane beside chat.
- Compact layouts preserve the drawer flow.
- Feature stores and durable state remain layout-independent.

### Accessibility

- Added centralized streaming-announcement batching.
- Added an accessibility store and bounded announcement component.
- Replaced the remaining placeholder QML tests with behavioural tests for:
  - accessible names
  - keyboard focus/tab order
  - 200% font scaling
  - RTL mirroring
  - multiline composer behaviour
  - announcement batching
  - destructive two-step confirmation
  - reduced motion
  - timeline tool semantics
- Added a QuickTest executable and CTest target for the QML test suite.

### Build and architecture contracts

- Added `Qt6::QuickDialogs2` for the native file picker.
- Extended the desktop/demo bridge stub with the same Phase 4 invokables as the Rust bridge.
- Updated architecture checks for the new stores and component.
- No production screen/component calls `mukeiAgent`, `mukeiBridge`, or `mukeiRuntime` directly.

## Deliberate limitations

- Installed model selection validates and persists UI selection for the next engine session; true live engine hot-swap is not claimed.
- Android `takePersistableUriPermission` integration is not yet compiled/device-verified. The UI therefore does not claim that Android OS permission persistence is complete.
- Document ingestion/indexing remains a separate backend pipeline; access registration does not claim indexing success.
- Accessibility batching emits the architecture-level announcement signal. Native assistive-technology delivery must be completed/verified with the selected Qt/Android runtime.
- Rust, Qt, Android, Clippy, and device compilation remain external verification steps.

## Version

Package: `Mukei_v0.13_qml_phase4`
