# Mukei QML Architecture — Phase 2

Phase 2 implements durable chat projection and recovery foundations on top of the Phase 1 shell.

## Implemented

- Native `QAbstractListModel` timeline adapter with stable roles, row updates, pagination prepend, and duplicate suppression.
- Rust-backed conversation snapshots scoped by durable conversation and branch UUIDs.
- Durable conversation-list projection endpoint.
- Durable, versioned UI sessions and per-conversation/branch drafts through migration V011.
- Debounced draft persistence with forced app-pause flush.
- Last safe route, conversation, branch, timeline anchor, and selected-model persistence.
- Conversation drawer hydration and opening.
- Stable message-anchor pagination.
- Typed bridge events now include durable `branch_id` alongside `conversation_id`.
- First-turn UI scope reconciliation from bridge events.
- Interrupted-turn discovery, dedicated recovery route, Continue/Start Again/Not Now actions.
- Recovery-safe restoration before normal chat routing.
- Native timeline unit-test target and Rust repository tests for session and scope isolation.
- Motion tokens aligned to the finalized QML architecture.

## Architecture boundary

Domain truth and durable persistence remain in Rust/SQLCipher. The C++ `MukeiTimelineModel` is a Qt projection adapter only; it does not own domain persistence. QML renders the model and dispatches intents.

## Pending later phases

- Move the projection adapter into a CXX-Qt Rust model if desired without changing the QML role contract.
- Models/downloads/documents feature stores and snapshots.
- Native capability and operation projections beyond the existing event contract.
- Full accessibility primitive replacement and responsive expanded layout.
- Qt/QML, Rust, Clippy, Android, emulator, and device compilation/testing.
