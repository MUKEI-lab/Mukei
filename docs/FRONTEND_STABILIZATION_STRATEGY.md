# Frontend Stabilization Strategy

## Objective

Stop the one-fix/one-APK loop. Most defects must be detected by deterministic QML, bridge, and Android automation. A physical phone is reserved for milestone builds.

## Delivery cadence

1. Source batch
2. QML/offscreen interaction tests
3. Rust/CXX-Qt contract tests
4. Android emulator smoke
5. One signed milestone APK
6. Fixed physical-device checklist

## Batch A — interaction contracts

- Assign stable `objectName` identifiers to every interactive control.
- Maintain `reports/frontend/INTERACTION_MATRIX.md`.
- Fail CI when a visible action has no handler.
- Test local navigation without a native runtime.
- Test state-dependent controls with deterministic stores.

## Batch B — startup state machine

The required observable sequence is:

`bootstrapping → booting → loading_config → secure-key stages → opening_database → ready/degraded`

Each boundary must produce a structured stage or classified error. The global watchdog is only a final safety net.

## Batch C — feature workflows

Cover model catalogue, download/cancel/select/delete, chat send/stop, documents, settings, diagnostics, and recovery using a deterministic test bridge.

## Batch D — Android milestone

A phone APK is produced only when:

- QML tests pass
- bridge tests pass
- Rust workspace passes
- emulator cold launch and core navigation pass
- APK validation and signing pass

## Current milestone gate

The next physical-device APK requires Batch A and the startup state-machine contract to pass. It is not triggered by an individual button change.
