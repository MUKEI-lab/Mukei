# Mukei Android APK build

This directory owns the APK-first packaging path. The current scope is deliberately limited to **arm64-v8a**. Multi-ABI packaging and AAB generation are deferred until the first APK builds, installs, and launches successfully.

## Required toolchains

Install compatible versions of:

- Android SDK with platform 35 and build tools;
- Android NDK compatible with the selected Qt Android kit;
- matching Qt host and `android_arm64_v8a` kits;
- Rust stable with the `aarch64-linux-android` target;
- CMake, Ninja, Python 3, `unzip`, and a compatible JDK.

Set these environment variables:

```bash
export ANDROID_SDK_ROOT=/path/to/Android/Sdk
export ANDROID_NDK_ROOT="$ANDROID_SDK_ROOT/ndk/<version>"
export QT_HOST_ROOT=/path/to/Qt/6.5.3/gcc_64
export QT_ANDROID_ROOT=/path/to/Qt/6.5.3/android_arm64_v8a
export JAVA_HOME=/path/to/jdk
```

Optional overrides:

```bash
export ANDROID_API=29
export BUILD_TYPE=Release
export BUILD_ROOT=/custom/build/directory
export DIST_DIR=/custom/output/directory
```

## Build

From the repository root:

```bash
bash scripts/android/build-apk.sh
```

The script performs these stages in order:

1. builds the pinned llama.cpp capsule for `arm64-v8a`;
2. cross-compiles `mukei-bridge` with the `android-release` Cargo profile;
3. exports deterministic CXX-Qt headers;
4. configures the Qt Android application with production bridge and inference enabled;
5. invokes the Qt/Gradle APK target;
6. copies the artifact to `dist/android/`;
7. validates ZIP integrity, manifest/resources presence, ABI purity, and native capsule packaging.

Expected output for the current version:

```text
dist/android/mukei-0.7.5-arm64-v8a.apk
```

## Signing boundary

The repository does not contain a production keystore or signing passwords. The initial APK may therefore be unsigned or toolchain test-signed. Production signing is intentionally performed outside source control.

## Models and OBB

GGUF models are not bundled into the APK or an OBB. They remain on-demand downloads with SHA-256 verification, avoiding multi-gigabyte application packages.
