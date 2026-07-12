# Mukei QML Architecture — Phase 3

Phase 3 extends the persistent reactive projection architecture from chat into models, downloads, private documents, storage, settings, accessibility primitives, and responsive navigation.

## Implemented

- Rust-backed model catalogue projection with verified install state and bytes-on-disk metadata.
- Durable download job projection that exposes opaque destination tokens rather than private filesystem paths.
- Download event correlation by model ID for parallel-job-safe UI updates.
- Storage quota snapshot with normal, warning, and critical pressure states.
- Privacy-safe private-document projection with hashed document identifiers, chunk counts, revoke state, and retryable cleanup status.
- Typed settings snapshot and validated persistence for theme, motion, contrast, text scale, inference defaults, and remote-feature policy.
- Model, download, storage, document, settings, and responsive feature stores.
- Durable operation reconciliation for restored downloads and pending document cleanup.
- Real model manager, model picker, download history, private-document, diagnostics, and settings screens.
- Responsive compact, medium, and expanded shell navigation.
- Qt Control-based accessible button primitives with keyboard activation, focus indication, and 44-pixel minimum touch targets.
- Storage-pressure banner and cards.
- Unified `MukeiIcon` usage for soft, palette-aware icon rendering.
- Qt minimum aligned to 6.5 because the application uses Qt Quick Effects / MultiEffect.
- Additional Rust tests for settings validation, settings snapshot projection, and download-path privacy.

## Architecture boundary

Rust and SQLCipher remain authoritative for all durable state. QML stores are reactive projections and dispatch typed intents. Download and document projections deliberately omit raw private paths, SAF tokens, and document content.

## Deliberately deferred

- Real backend model activation/switching API. The UI does not pretend that selecting a catalogue row loads a model.
- Safe active-model deletion and orphan-model cleanup.
- Android document picker, document attach, and revoke commands.
- Privacy-safe diagnostics export.
- Expanded split-view chat/conversation layout.
- Complete QML lint, Qt compilation, Rust compilation, Android build, emulator, and physical-device validation.
