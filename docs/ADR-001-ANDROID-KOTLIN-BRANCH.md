# ADR-001: Separate Android and desktop production branches

## Status

Accepted.

## Decision

- `Kotlin` is the canonical Android production branch.
- `main` remains the canonical Qt/QML desktop branch.
- Android uses Kotlin and Jetpack Compose.
- Desktop uses Qt and QML.
- Both lines retain `mukei-core` as the product engine.
- Android uses a dedicated JNI bridge and must not depend on CXX-Qt.

## Consequences

Platform UI code is intentionally not merged between production branches. Shared behavior crosses the Rust protocol and core boundaries instead of sharing presentation frameworks.
