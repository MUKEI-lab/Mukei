# Mukei v0.5 Baseline Audit

Date: 2026-07-10

Authoritative archive: `/mnt/sdcard/Codex-Mukei/Mukei_v0.5_sol.zip`

Clean extraction root: `/tmp/mukei-v05-sol-audit/Mukei_v0.5_sol`

## 1. Repository Structure Summary

`unzip -t /mnt/sdcard/Codex-Mukei/Mukei_v0.5_sol.zip` completed with `No errors detected in compressed data`.

After clean extraction:

- Total files: 1,665
- Rust workspace: present at `rust/Cargo.toml`
- `mukei-core`: present at `rust/crates/mukei-core`
- `mukei-bridge`: present at `rust/crates/mukei-bridge`
- `mukei-ffi-shim`: present at `rust/crates/mukei-ffi-shim`
- QML application: present at `qml/MainWindow.qml`
- Android files: present at `qml/android/AndroidManifest.xml` and `qml/android/src/com/mukei/security/MukeiSecretStore.java`
- Vendored `llama.cpp`: present at `rust/llama-cpp-prebuilt/vendor/llama.cpp`
- SQL migrations: present at `rust/migrations`
- SVG icons: 27 files under `qml/assets/icons`
- GitHub Actions workflow: `.github/workflows/ci.yml`

Required inventory commands were run after extraction:

```sh
find . -type f | sort
find qml/assets/icons -type f | sort
find rust/migrations -type f | sort
```

Release archive hygiene:

- `.git`: not present
- `target` directories: not present
- build caches such as `.gradle` or `build`: not present
- keystores/cert private material matching `*.keystore`, `*.jks`, `*.p12`, `*.pem`: not present
- local env files matching `.env` or `.env.*`: not present
- code files containing "secret" in their names are present by design: `rust/crates/mukei-bridge/src/android_secret_store.rs` and `rust/migrations/V008__settings_and_secret_refs.sql`

No production source was modified during this audit.

## 2. Toolchain Version Table

Host OS detected:

- `Linux localhost 6.17.0-PRoot-Distro ... aarch64 GNU/Linux`
- Ubuntu 26.04 LTS

| Tool | Status | Version / Path |
| --- | --- | --- |
| rustup | missing | not found in `PATH` |
| rustc | present | `rustc 1.93.1 (01f6ddf75 2026-02-11)`, host `aarch64-unknown-linux-gnu`, LLVM `21.1.8` |
| cargo | present | `cargo 1.93.1 (083ac5135 2025-12-15)` |
| rustfmt | present | `rustfmt 1.8.0` |
| clippy | present | `clippy 0.1.93` |
| CMake | present | `cmake version 4.2.3` |
| Ninja | present | `1.13.2` |
| C compiler | present | `gcc (Ubuntu 15.2.0-16ubuntu1) 15.2.0`; `clang 21.1.8` also present |
| C++ compiler | present | `g++ (Ubuntu 15.2.0-16ubuntu1) 15.2.0`; `clang++ 21.1.8` also present |
| Java/JDK | missing | `java` and `javac` not found |
| Gradle | missing | `gradle` not found |
| Qt 6 | present | `qtpaths6 --qt-version` reports `6.10.2` |
| qmake / qtpaths | present | `qmake6` present, Qt `6.10.2`; `qtpaths6` present |
| qmllint | missing | `qmllint` / `qmllint6` not found |
| Android SDK | missing | `sdkmanager` not found |
| Android NDK | missing | `ndk-build` not found |

## 3. Missing Tools

Install nothing automatically in this phase. Recommended commands for this Ubuntu-based environment:

```sh
# Rustup, if rustup-managed targets/toolchains are required:
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# JDK, Gradle, Android SDK package set available from apt:
sudo apt update
sudo apt install -y openjdk-21-jdk gradle android-sdk android-sdk-platform-tools android-sdk-build-tools

# Android command-line tools / NDK, after sdkmanager is available:
sdkmanager "platforms;android-35" "build-tools;35.0.0" "ndk;27.2.12479018" "cmdline-tools;latest"

# Qt linting support: package search found qt6-qmllint-plugins, but no qmllint executable package was visible.
sudo apt install -y qt6-qmllint-plugins
```

If the apt-provided Android SDK does not include `sdkmanager`, install Google's Android command-line tools into `$ANDROID_HOME` and add `$ANDROID_HOME/cmdline-tools/latest/bin` plus `$ANDROID_HOME/platform-tools` to `PATH`.

## 4. Source / Resource Integrity Result

Structured parsing:

- TOML parsed successfully: 11 files, 0 errors
- XML/QRC parsed successfully: 2 files, 0 errors
- `cargo metadata --no-deps --format-version 1` from `rust/`: succeeded
- Workspace members resolved: all present
- Local Cargo path dependencies resolved: all present

QML resources:

- `qml/qml.qrc` references 36 files
- Missing files from `qml/qml.qrc`: 0
- QML files declared in `qml/CMakeLists.txt`: all present
- QML test files referenced by CMake: `qml/tests/tst_EventDispatcher.qml` and `qml/tests/tst_Security.qml` are present

The archive is structurally complete for baseline inspection. Compilation was not claimed or attempted as successful because required Java/Android/qmllint tooling is missing.

## 5. Migration Inventory

Top-level migration/config files:

- `000_default_config.toml`
- `V001__schema.sql`
- `V001__down.sql`
- `V002__recovery_state.sql`
- `V002__down.sql`
- `V003__tooling_and_saf.sql`
- `V003__down.sql`
- `V004__branching.sql`
- `V004__down.sql`
- `V005__audit_chain_checks.sql`
- `V005__down.sql`
- `V006__branch_message_constraints.sql`
- `V007__message_status.sql`
- `V008__settings_and_secret_refs.sql`
- `V008__down.sql`
- `V009__schema_metadata_and_rag_tombstones.sql`
- `V009__down.sql`
- `V010__reliability_hardening.sql`
- `V010__down.sql`

Migration sequence result:

- Forward versions are exactly sequential from `V001` through `V010`
- No conflicting duplicate forward definitions detected
- Rollback files are present for V001, V002, V003, V004, V005, V008, V009, and V010
- `rust/migrations/legacy/V008__schema_metadata_and_rag_tombstones.sql` exists under `legacy/` and is not a top-level migration conflict

## 6. Feature and Dependency Inventory

Workspace members:

- `crates/mukei-core`
- `crates/mukei-bridge`
- `crates/mukei-ffi-shim`
- `llama-cpp-stub`

Workspace package metadata:

- Version: `0.7.5`
- Edition: `2021`
- Declared MSRV: `1.78`
- License: `Apache-2.0`
- `rust-toolchain.toml`: channel `stable`, components `rustfmt` and `clippy`, targets `aarch64-linux-android`, `x86_64-unknown-linux-gnu`, `aarch64-apple-darwin`

Crate features:

- `mukei-core`: `default`, `std`, `tokio`, `llama_cpp`, `usearch_hnsw`, `rusqlite`, `sqlcipher`, `network`, `candle`, `android_keystore`, `release-hardening`
- `mukei-bridge`: `default`, `android_keystore`, `desktop`, `rusqlite`, `sqlcipher`, `network`
- `mukei-ffi-shim`: no crate features
- `llama-cpp-stub`: `default`, `stub-acknowledged`, `release-hardening`

Important dependency observations:

- `mukei-bridge` defaults to `android_keystore`, `network`, and `sqlcipher`
- `mukei-bridge` has Android-only dependencies on `jni = 0.21`, `ndk-context = 0.1`, and `nix = 0.27`
- `sqlcipher` enables `rusqlite/bundled-sqlcipher`
- `llama-cpp-rs` currently resolves to local `llama-cpp-stub`
- `Cargo.lock` is present in the archive
- Patch notes warn that v0.5 adds Android-target deps and that the first normal Cargo command may update `Cargo.lock`; avoid `--locked` until that is intentionally handled

## 7. Compile-Risk Findings

The following areas should be treated as Phase 2 compile-risk targets:

- `mukei-bridge` contains an Android SQLCipher compile gate: Android release builds require the `sqlcipher` feature.
- Android-only JNI code is present in `rust/crates/mukei-bridge/src/android_secret_store.rs` and depends on `jni` plus `ndk-context`; Android SDK/NDK are currently missing locally.
- Runtime initialization is coordinated through `RuntimeCoordinator` / `RuntimePhase` in `rust/crates/mukei-bridge/src/bridge_state.rs`; initialization failure transitions to `Quarantined`.
- `mukei-bridge/src/lib.rs` has duplicate `set_database_cipher_key` Rust method definitions under complementary cfg paths; verify feature combinations explicitly.
- `tokio` and `parking_lot` locks are mixed in bridge paths; synchronous QML invokables should be checked for blocking runtime interactions.
- SQL migration APIs and pre-migration backup flow are in `rust/crates/mukei-core/src/storage/migrations.rs` and `rust/crates/mukei-bridge/src/agent_runtime.rs`; verify V001-V010 execution against SQLCipher.
- Durable interrupted-turn recovery APIs are declared in the bridge: `interrupted_turn_json`, `resume_interrupted_turn`, and `regenerate_interrupted_turn`.
- Download reservation lifecycle spans `rust/crates/mukei-bridge/src/lib.rs`, `rust/crates/mukei-bridge/src/bridge_state.rs`, and `rust/crates/mukei-core/src/storage/download_jobs.rs`.
- QML bridge declarations exist in Rust CXX-Qt blocks; the QML app currently uses stubs unless `MUKEI_USE_REAL_BRIDGE` is enabled, so Phase 2 must separately validate real bridge linkage.
- `qmllint` is missing, so QML lint validation is blocked locally.

Unfinished-marker scan, excluding vendored `llama.cpp`:

| Marker | Count |
| --- | ---: |
| `TODO` | 2 |
| `FIXME` | 0 |
| `unimplemented!` | 2, both in documentation/security checklist text |
| `todo!` | 1, documentation text |
| `panic!` | 11, mostly tests/diagnostics plus FFI-shim hard failures |
| `unreachable!` | 2 |
| `placeholder` | 27, mostly docs and `llama-cpp-stub` references |
| `no-op` | 14 |
| `.unwrap(` | 238 |
| `.expect(` | 27 |
| `unsafe` | 46 |
| ignored-Result-like patterns (`let _ =`, `.ok();`) | 127 |

Representative production-risk locations:

- `rust/crates/mukei-core/src/runtime.rs`: mandatory runtime initialization `expect`
- `rust/crates/mukei-core/src/storage/recovery.rs`: durable interrupted-turn recovery
- `rust/crates/mukei-core/src/storage/model_download.rs`: reservation/retry cleanup with ignored send/remove results
- `rust/crates/mukei-core/src/rag/vector_store.rs`: ignored vector index add/remove results
- `rust/crates/mukei-bridge/src/lib.rs`: bridge invokables, cancellation, download reservation, SQLCipher key path, panic-in-test code
- `rust/crates/mukei-ffi-shim/src/lib.rs`: unsafe FFI guard handling and C header checks

These findings are compile/review risks, not requested source changes.

## 8. Security-Sensitive Code Locations

- SQLCipher enforcement and DB opening:
  - `rust/crates/mukei-bridge/src/lib.rs`
  - `rust/crates/mukei-bridge/src/agent_runtime.rs`
  - `rust/crates/mukei-core/src/storage/pool.rs`
- Migration safety and encrypted backup:
  - `rust/crates/mukei-core/src/storage/migrations.rs`
  - `rust/crates/mukei-bridge/src/agent_runtime.rs`
- Android Keystore and provider secrets:
  - `rust/crates/mukei-bridge/src/android_secret_store.rs`
  - `qml/android/src/com/mukei/security/MukeiSecretStore.java`
  - `rust/migrations/V008__settings_and_secret_refs.sql`
- Redaction and diagnostics:
  - `rust/crates/mukei-core/src/diagnostics/redaction.rs`
  - `rust/crates/mukei-core/src/diagnostics/logger.rs`
  - `rust/crates/mukei-core/src/diagnostics/crash_logger.rs`
  - `rust/crates/mukei-core/src/diagnostics/panic_hook.rs`
- SAF tokens, revoke/delete, tombstones:
  - `rust/crates/mukei-core/src/storage/saf.rs`
  - `rust/crates/mukei-core/src/rag/indexer.rs`
  - `rust/crates/mukei-bridge/src/agent_runtime.rs`
- Audit chain:
  - `rust/crates/mukei-core/src/storage/audit_log.rs`
  - `rust/migrations/V005__audit_chain_checks.sql`
- Path validation and quota:
  - `rust/crates/mukei-core/src/storage/quota.rs`
  - `rust/crates/mukei-core/src/storage/model_download.rs`
  - `rust/crates/mukei-bridge/src/lib.rs`
- FFI and panic boundaries:
  - `rust/crates/mukei-core/src/guard.rs`
  - `rust/crates/mukei-ffi-shim/src/lib.rs`
  - `rust/crates/mukei-ffi-shim/include/mukei_ffi_shim.h`

## 9. Exact Commands Recommended for Phase 2

Use the clean extracted tree and avoid the archive-contaminating existing checkout:

```sh
cd /tmp/mukei-v05-sol-audit/Mukei_v0.5_sol
```

Rust baseline:

```sh
cd rust
CARGO_TARGET_DIR=/tmp/mukei-v05-target cargo fmt --all -- --check
CARGO_TARGET_DIR=/tmp/mukei-v05-target cargo check -p mukei-core --no-default-features --features 'std,tokio'
CARGO_TARGET_DIR=/tmp/mukei-v05-target cargo check -p mukei-core --features 'std,tokio,rusqlite'
CARGO_TARGET_DIR=/tmp/mukei-v05-target cargo check -p mukei-core --features 'std,tokio,sqlcipher'
CARGO_TARGET_DIR=/tmp/mukei-v05-target cargo test -p mukei-core --features 'std,tokio,rusqlite'
CARGO_TARGET_DIR=/tmp/mukei-v05-target cargo check -p mukei-ffi-shim
CARGO_TARGET_DIR=/tmp/mukei-v05-target cargo check -p mukei-bridge --no-default-features --features desktop
CARGO_TARGET_DIR=/tmp/mukei-v05-target cargo check -p mukei-bridge --features sqlcipher,network
```

Android Rust target, after installing rustup/SDK/NDK and setting linker variables:

```sh
rustup target add aarch64-linux-android
cd /tmp/mukei-v05-sol-audit/Mukei_v0.5_sol/rust
CARGO_TARGET_DIR=/tmp/mukei-v05-target cargo check -p mukei-bridge --target aarch64-linux-android --all-features
```

QML/CMake baseline, after installing `qmllint` if available:

```sh
cd /tmp/mukei-v05-sol-audit/Mukei_v0.5_sol
cmake -S qml -B /tmp/mukei-v05-qml-build -G Ninja
cmake --build /tmp/mukei-v05-qml-build
ctest --test-dir /tmp/mukei-v05-qml-build --output-on-failure
find qml -name '*.qml' -print0 | xargs -0 -n1 qmllint
```

Android packaging, after SDK/NDK/JDK/Gradle installation and environment setup:

```sh
export ANDROID_HOME="$HOME/Android/Sdk"
export ANDROID_NDK_HOME="$ANDROID_HOME/ndk/27.2.12479018"
export PATH="$ANDROID_HOME/cmdline-tools/latest/bin:$ANDROID_HOME/platform-tools:$PATH"
cd /tmp/mukei-v05-sol-audit/Mukei_v0.5_sol
cmake -S qml -B /tmp/mukei-v05-android-build -G Ninja \
  -DANDROID=ON \
  -DANDROID_ABI=arm64-v8a \
  -DANDROID_PLATFORM=android-29 \
  -DCMAKE_TOOLCHAIN_FILE="$ANDROID_NDK_HOME/build/cmake/android.toolchain.cmake"
cmake --build /tmp/mukei-v05-android-build
```

Do not use `--locked` until the v0.5 Android dependency lockfile note has been resolved deliberately.

## 10. Final Status

`BLOCKED_BY_MISSING_TOOLS`

Reason: the release archive is structurally complete and parse-valid, but local Phase 2 compile/package validation is blocked by missing `rustup`, Java/JDK, Gradle, `qmllint`, Android SDK, and Android NDK tooling.
