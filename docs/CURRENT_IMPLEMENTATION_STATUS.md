# Kotlin Android implementation status

**Branch:** `Kotlin`  
**Role:** Android production branch  
**Desktop branch:** `main`

## Completed bootstrap work

- Android and desktop delivery lines are separated by branch.
- QML, Qt Android packaging and CXX-Qt bridge source are removed from this branch.
- `mukei-core` remains the shared platform-neutral engine.

## Active construction

- Kotlin/Jetpack Compose application scaffold.
- Transport-neutral Protocol V2 Kotlin models.
- Dedicated `mukei-android-jni` runtime boundary.

## Release status

This is an architecture bootstrap, not a release candidate. Native build, protocol contract, APK packaging, device lifecycle, Keystore/SQLCipher and physical-device validation remain open gates.
