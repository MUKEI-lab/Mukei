# QML Architecture Phase 6 — Contract and Recovery Hardening

Phase 6 makes compatibility and recovery first-class architecture concerns.

## Startup contract

Before private storage opens, QML requests a bridge-owned contract snapshot. Startup proceeds only when:

- the local QML contract version is within the bridge-supported range;
- command, event, and snapshot schemas are understood; and
- all required architectural features are known and present.

Failure routes to `CompatibilityScreen` and blocks all non-retry intents.

## Recovery contract

Application resume rehydrates authoritative snapshots rather than trusting stale in-memory state. Rust provides a privacy-safe operation snapshot for durable downloads, document cleanup, and document ingestion. Blocked ingestion is distinguished from active work.

## Source consistency

The phase also fixes a duplicate mock-inference binding and aligns development stubs with durable ingestion-state names.

## Boundary

No document embedder or live llama.cpp activation is claimed. Those capabilities remain blocked until their native implementations compile and pass runtime/device validation.
