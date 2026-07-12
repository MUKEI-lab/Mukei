# QML Architecture Phase 1

This patch introduces the foundation described in `docs/Qml_Arc.md`.

Implemented:

- Root `AppStateHub` composition
- Scoped lifecycle, capabilities, navigation, chat, operations, error, and UI-session stores
- Typed intent gateway with a strict known-command set
- Lifecycle-derived routing and global shell hosts
- Snapshot-gap detection contract
- Central error presentation and operation overlay
- Non-demo chat timeline with a single streaming assistant row
- `ListView` delegate reuse and controlled tail-follow behavior
- Draft-session contract and in-memory compatibility adapter
- App-private runtime config path supplied by C++
- Android builds default to the real Rust bridge

Transitional boundaries:

- QML stores currently adapt the existing JSON bridge events. Native Rust projection objects and
  `QAbstractListModel` implementations will replace these compatibility adapters in the chat phase.
- Durable UI-session persistence requires the planned Rust `ui_session_repository`; the QML store
  already exposes the persistence contract but does not claim durable storage.
- Real SQLCipher startup remains waiting for the native secure-key provider. The key is never routed
  through QML.
