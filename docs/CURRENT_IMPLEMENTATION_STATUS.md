# Kotlin Android implementation status

**Branch:** `Kotlin`  
**Role:** Android production branch  
**Desktop branch:** `main`

## Foundation completed

1. `Kotlin` was created from `main` as the Android product branch.
2. QML/CXX-Qt Android packaging was removed from this branch in favor of Kotlin/Jetpack Compose under `android/`.
3. Protocol V2 Kotlin boundary models live under `android/core/protocol/`.
4. The Kotlin native gateway and dedicated Rust `mukei-android-jni` runtime bridge are integrated.
5. Secure runtime bootstrap, SQLCipher-backed local state, Android Keystore wrapping, native event/snapshot transport, and hardened release packaging are part of the active Android implementation.

## Current bridge/runtime state

The Android native boundary is no longer only the original scaffold. `MukeiNativeGateway` exposes protocol capabilities, security/readiness state, typed command submission, Temporary Chat begin/end, event draining, platform request/response transport, domain snapshots, and runtime shutdown.

The Android host consumes the native application snapshot for inference readiness and fails closed on runtime event-sequence discontinuities. Temporary Chat and chat-submission transitions have explicit rollback/cleanup contract checks.

This status does **not** imply every product-roadmap feature is complete. Share-to-Mukei, full chat history/branching UX, accessibility completion, expanded SAF/file UX, export, and Settings remain roadmap work.

## Automated validation

`android-kotlin-ci` currently exercises the Android protocol/native/app test suites, lint, debug/offline assembly, Rust/security feature matrix, hardened native release builds, R8 release APKs, ABI/JNI packaging verification, and release artifact generation.

The dedicated `android-release-candidate` workflow builds hardened native capsules and ABI-specific release APKs, verifies packaged native libraries, signs with Android Build Tools, verifies signatures and zip alignment, records SHA-256 checksums, and publishes an observable commit status.

## Release signing policy

- Untagged `Kotlin` and manual release-candidate builds use an ephemeral CI-generated **test certificate** and are validation artifacts only.
- `android-v*` release tags are fail-closed and require the persistent Android release/update signing identity configured through the secrets documented in `docs/ANDROID_RELEASE_SIGNING.md`.
- A test-signed APK must not be published as the first Beta because future builds signed by a different certificate cannot provide normal update continuity for the same application ID.

## Release status — 2026-07-20

P1/P2 runtime hardening and release-candidate observability are merged into `Kotlin`. Automated Android/Rust/release-hardening gates have produced verified ARM64 test-signed release-candidate artifacts.

The first public Beta is **not yet certified**. Two hard release gates remain:

1. **Persistent release/update signing identity provisioning** — tracked in GitHub issue #113.
2. **Physical ARM64 device end-to-end certification on the exact release-signed artifact** — tracked in GitHub issue #114.

Do not cut a public Beta/GitHub APK release until both gates are closed and the published APK checksum matches the exact artifact that passed physical-device validation.