# Mukei QML Architecture Phase 6

**Theme:** Contract and recovery hardening

Phase 6 freezes the QML/Rust integration boundary before additional feature work. It intentionally prioritizes mixed-build safety, resume correctness, and source-level integration defects over pretending that unsupported model or embedding capabilities are complete.

## Implemented

- Fail-closed QML/Rust contract negotiation before private storage startup.
- Versioned UI, command, event, and snapshot contract handshake.
- Required-feature negotiation with unknown-feature rejection.
- Blocking compatibility lifecycle state, route, and screen.
- Retryable negotiation that cannot bypass lifecycle or intent gating.
- Native privacy-safe operation snapshot for durable downloads, document cleanup, and document ingestion.
- App-resume reconciliation for conversations, chat, downloads, documents, storage, diagnostics, and operations.
- Separate active and blocked operation counts.
- Stable download operation identity across snapshots, fallbacks, and live events.
- QML tests for compatible/incompatible contracts and operation snapshot normalization.
- Cross-language CI contract guard.
- QML singleton metadata for isolated QuickTest imports.
- Fixed duplicate immutable `chunk` binding in the mock inference wrapper.
- Aligned stub ingestion state with the durable `waiting_for_embedder` contract.
- Removed duplicate C++ access-label noise in the stub bridge.
- Updated the frozen architecture specification with contract and resume rules.

## Intentional boundaries

This phase does not claim:

- a connected document parser/embedder worker;
- live llama.cpp model activation or hot-swap;
- successful Rust, Qt, Android, or device compilation in this environment.

Unsupported ingestion remains explicitly `blocked`/`waiting_for_embedder`, and the model-session projection remains truthful about the unwired engine.
