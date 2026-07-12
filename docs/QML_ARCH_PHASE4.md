# QML Architecture Phase 4

Phase 4 completes the first product-management layer on top of Mukei's persistent reactive projection architecture.

## Architecture additions

- Safe model selection/deletion intents and projections
- Private document picker/grant/revoke flow
- Privacy-safe diagnostics snapshot and atomic export
- Responsive expanded chat split view
- Central accessibility announcement batching
- Behavioural QML test suite replacing placeholder tests

## Ownership rule

Rust remains authoritative for model files, document grants, cleanup state, diagnostics data, and security validation. QML only dispatches typed intents and renders safe projections.

## Remaining native contracts

- Engine hot-swap/restart lifecycle
- Android persistable URI permission acquisition
- Document ingestion/indexing orchestration
- Native assistive-technology announcement sink
- Full Qt/Android compile and device validation
