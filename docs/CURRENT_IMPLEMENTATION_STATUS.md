# Kotlin Android implementation status

**Branch:** `Kotlin`  
**Role:** Android production branch  
**Desktop branch:** `main`

## Bootstrap completed

1. `Kotlin` was created from `main`.
2. QML, CXX-Qt bridge crates, Qt Android packaging scripts and their CI workflows were removed from this branch.
3. A Kotlin/Jetpack Compose Android scaffold was added under `android/`.
4. Protocol V2 Kotlin boundary models were added under `android/core/protocol/`.
5. A narrow Kotlin native gateway and dedicated Rust `mukei-android-jni` crate were added.

## Current bridge state

The JNI runtime owns opaque handles, validates input bounds, contains Rust panics and exposes command, event-drain and snapshot entry points. Domain dispatch into `mukei-core` is intentionally not implemented in this scaffold commit; commands currently return a stable `backend_unavailable` acknowledgement.

## Release status

This is an architecture scaffold, not a release candidate. Remote CI, generated dependency locks, Android ABI builds, native library packaging, Protocol V2 contract tests, Keystore/SQLCipher integration and physical-device validation remain open gates.
