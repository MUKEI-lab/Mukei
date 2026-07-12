# Mukei QML Architecture — Phase 5

## Scope

Phase 5 hardens the platform boundary and makes the frontend truthful about engine, document, and accessibility capabilities.

## Implemented

### Android document permission boundary

- Added `MukeiDocumentAccess.java` using Android `ContentResolver`.
- Attempts read-only `takePersistableUriPermission` for `content://` selections.
- Distinguishes `persisted`, `transient`, `not_required`, and `failed` permission states.
- Verifies current readability before accepting a grant.
- Releases persisted permission during successful revoke.
- Never converts a content URI into a filesystem path.
- Added target-gated Rust JNI adapter using the application class loader.

### Atomic durable document registration

- Added migration `V012__document_access_and_ingestion_jobs.sql`.
- SAF grant, real OS permission outcome, and initial ingestion job commit in one SQLite transaction.
- QML receives only a one-way `document_id`.
- Revoke resolves through the durable document mapping rather than scanning bridge-side SQL.
- Revoke cancels an unfinished ingestion job and preserves existing vector-cleanup tombstone behavior.
- Invalid permission state cannot leave a partial grant.

### Truthful ingestion projection

- Added durable ingestion states and progress/error fields to document projections.
- Current build records `waiting_for_embedder` instead of pretending indexing has begun.
- Retry resets the durable job without claiming completion.
- UI exposes persisted versus transient access and retryable ingestion failures.
- The actual content-reader/embedder worker remains a separate backend integration and is not falsely reported as connected.

### Engine-session truth

- Added `engine_session_snapshot_json()`.
- Model UI distinguishes selected model from loaded model.
- Current backend explicitly reports `mock_unwired` and `activation_supported=false` rather than claiming live llama.cpp activation.
- Selected model remains durable for a future supported engine session.

### Streaming and accessibility performance

- Chat chunks are coalesced into 48 ms UI batches before updating the single assistant row.
- Screen-reader text remains separately batched and bounded.
- Added a native Qt accessibility adapter.
- Qt 6.8+ sends `QAccessibleAnnouncementEvent`; Qt 6.5–6.7 retains the architecture signal without claiming native delivery.

## Tests added

- Host file-URI permission behavior.
- Host content-URI rejection/readability behavior.
- Atomic grant + permission + ingestion projection round trip.
- Invalid permission state leaves no partial database or memory grant.
- Document revoke cancels the queued ingestion job.

## Deliberate boundaries

- Real Android permission retention requires Android compilation and device/provider testing.
- The on-device document reading, parsing, chunking, and Candle embedding worker is not wired in this phase.
- Live llama.cpp model activation is not wired in this phase.
- Native accessibility announcement delivery is available only when built with Qt 6.8+; older supported Qt builds use the existing batched signal boundary.
