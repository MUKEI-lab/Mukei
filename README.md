# Mukei Android

This branch is the canonical Android production line for Mukei.

## Platform split

- `Kotlin`: Android, Kotlin, Jetpack Compose and JNI.
- `main`: desktop, Qt and QML.
- `rust/crates/mukei-core`: shared local-first product engine.

The Android branch does not ship Qt, QML or CXX-Qt. Android UI and lifecycle integration belong to Kotlin. Inference, agent execution, encrypted storage, RAG, tools, model management and protocol validation remain in Rust.

## Target architecture

```text
Jetpack Compose
      ↓
Kotlin state and repositories
      ↓
MukeiNativeGateway
      ↓ JNI
mukei-android-jni
      ↓
mukei-core
```

## Bootstrap status

1. Android branch cleanup: established.
2. Kotlin scaffold: see `android/`.
3. JNI bridge scaffold: see `rust/crates/mukei-android-jni/`.

This branch is under active migration and is not yet release-certified.
