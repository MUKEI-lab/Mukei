# Mukei QML Architecture — Phase 1 Foundation

This patch begins implementation of `docs/Qml_Arc.md` on top of the current v0.9 workspace snapshot.

## Implemented

- Rust-owned architecture boundary represented by a root `AppStateHub`
- Scoped lifecycle, capabilities, navigation, chat, operation, error, and UI-session stores
- Typed `IntentDispatcher` as the only QML command gateway
- Canonical `EventDispatcher` with schema validation, source-local sequencing, stale-event rejection, bounded event-id deduplication, and sequence-gap signaling
- Snapshot recovery contract through `SnapshotController`
- Explicit lifecycle-derived routing
- Global app shell with router, banner, snackbar, operation, sheet, and dialog hosts
- App-private config path from `QStandardPaths`
- Android builds default to the real Rust bridge
- Real-bridge startup waits for the native database-key provider; database keys are never routed through QML
- Demo conversation removed
- Chat screen consumes `ChatStore` instead of direct Rust signals
- One assistant row is updated during streaming instead of creating one bubble per chunk
- `ListView` + delegate reuse replaces `Flickable + Repeater` for the active timeline
- Controlled auto-scroll that respects users reading older messages
- Draft-session contract and debounce-ready persistence boundary
- Capability-gated send/stop/settings/clear actions
- Settings navigation and theme actions wired
- User message bubble width defect fixed
- Keyboard send path now respects streaming, capability, and empty-text guards
- Unified `MukeiIcon` component introduced
- Static QML architecture guard added to CI

## Transitional boundaries

- `ChatStore` and other QML stores currently adapt the existing JSON bridge contract. They are compatibility projections, not durable sources of truth.
- Native Rust QObject/QAbstractListModel projections will replace the compatibility stores in the chat implementation phase.
- `UiSessionStore` exposes a versioned persistence contract, but durable storage is pending the Rust `ui_session_repository`.
- Snapshot requests are detected and surfaced, but the Rust snapshot endpoint is not yet implemented.
- Native SQLCipher key preparation remains a native-platform responsibility.

## Next phase

- Rust `AppStateHub` projection objects
- Native paginated `QAbstractListModel` timeline
- Durable UI session and draft repository
- Snapshot endpoints and atomic projection replacement
- Conversation/branch opening commands
- Recovery UI and operation reconciliation
- Model/download/document stores
- Accessible control primitives and complete QML tests
