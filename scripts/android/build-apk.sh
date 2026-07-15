#!/usr/bin/env bash
set -Eeuo pipefail

# Canonical APK-first build for Mukei.
# Scope is intentionally arm64-v8a only. Multi-ABI packaging belongs to the
# later AAB phase and must not silently reuse an aarch64 Rust static library.

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
readonly SCRIPT_DIR
REPO_ROOT="$(cd -- "${SCRIPT_DIR}/../.." && pwd)"
readonly REPO_ROOT
readonly ABI="arm64-v8a"
readonly RUST_TARGET="aarch64-linux-android"
readonly ANDROID_API="${ANDROID_API:-29}"
readonly BUILD_TYPE="${BUILD_TYPE:-Release}"
readonly BUILD_ROOT="${BUILD_ROOT:-${REPO_ROOT}/build/android}"
readonly LLAMA_BUILD_DIR="${BUILD_ROOT}/llama-${ABI}"
readonly QML_BUILD_DIR="${BUILD_ROOT}/qml-${ABI}"
readonly DIST_DIR="${DIST_DIR:-${REPO_ROOT}/dist/android}"
readonly CXX_QT_EXPORT_DIR="${CXX_QT_EXPORT_DIR:-${REPO_ROOT}/rust/target/cxxqt-export}"
readonly BRIDGE_LIB="${REPO_ROOT}/rust/target/${RUST_TARGET}/android-release/libmukei_bridge.a"
readonly LLAMA_LIB="${REPO_ROOT}/rust/llama-cpp-prebuilt/prebuilt/${ABI}/libmukei_llama_native.so"
readonly ANDROID_INITIAL_CACHE="${REPO_ROOT}/qml/cmake/MukeiAndroidApkInitialCache.cmake"
readonly BRANDING_STATE="${BUILD_ROOT}/branding-materialization.json"

fail() {
    printf 'error: %s\n' "$*" >&2
    exit 1
}

require_command() {
    command -v "$1" >/dev/null 2>&1 || fail "required command not found: $1"
}

require_directory() {
    [[ -d "$1" ]] || fail "required directory not found: $1"
}

require_executable() {
    [[ -x "$1" ]] || fail "required executable not found: $1"
}

cleanup_branding() {
    python3 "${SCRIPT_DIR}/prepare-branding.py" cleanup \
        --repo-root "${REPO_ROOT}" \
        --state "${BRANDING_STATE}"
}
trap 'cleanup_branding || true' EXIT

: "${ANDROID_SDK_ROOT:=${ANDROID_HOME:-}}"
: "${ANDROID_NDK_ROOT:=${ANDROID_NDK_HOME:-}}"
: "${QT_ANDROID_ROOT:=}"
: "${QT_HOST_ROOT:=}"

[[ -n "${ANDROID_SDK_ROOT}" ]] || fail "set ANDROID_SDK_ROOT (or ANDROID_HOME)"
[[ -n "${ANDROID_NDK_ROOT}" ]] || fail "set ANDROID_NDK_ROOT (or ANDROID_NDK_HOME)"
[[ -n "${QT_ANDROID_ROOT}" ]] || fail "set QT_ANDROID_ROOT to the Qt android_arm64_v8a kit"
[[ -n "${QT_HOST_ROOT}" ]] || fail "set QT_HOST_ROOT to the matching desktop Qt host kit"

require_directory "${ANDROID_SDK_ROOT}"
require_directory "${ANDROID_NDK_ROOT}"
require_executable "${QT_ANDROID_ROOT}/bin/qt-cmake"
require_executable "${QT_ANDROID_ROOT}/bin/qmake"
require_directory "${QT_HOST_ROOT}"
require_command cargo
require_command cmake
require_command ninja
require_command python3
require_command unzip
[[ -f "${ANDROID_INITIAL_CACHE}" ]] || fail "Android CMake initial cache is missing"

if command -v rustup >/dev/null 2>&1; then
    rustup target add "${RUST_TARGET}"
fi

ndk_prebuilt_root="${ANDROID_NDK_ROOT}/toolchains/llvm/prebuilt"
require_directory "${ndk_prebuilt_root}"

ndk_host_tag="${ANDROID_NDK_HOST_TAG:-}"
if [[ -z "${ndk_host_tag}" ]]; then
    for candidate in linux-x86_64 darwin-arm64 darwin-x86_64 windows-x86_64; do
        if [[ -d "${ndk_prebuilt_root}/${candidate}" ]]; then
            ndk_host_tag="${candidate}"
            break
        fi
    done
fi
[[ -n "${ndk_host_tag}" ]] || fail "could not determine Android NDK host tag"

readonly NDK_TOOLCHAIN_BIN="${ndk_prebuilt_root}/${ndk_host_tag}/bin"
readonly TARGET_CC="${NDK_TOOLCHAIN_BIN}/aarch64-linux-android${ANDROID_API}-clang"
readonly TARGET_CXX="${NDK_TOOLCHAIN_BIN}/aarch64-linux-android${ANDROID_API}-clang++"
readonly TARGET_AR="${NDK_TOOLCHAIN_BIN}/llvm-ar"
readonly TARGET_RANLIB="${NDK_TOOLCHAIN_BIN}/llvm-ranlib"
readonly TOOLCHAIN_COMPAT_BIN="${BUILD_ROOT}/toolchain-compat-${RUST_TARGET}"

require_executable "${TARGET_CC}"
require_executable "${TARGET_CXX}"
require_executable "${TARGET_AR}"
require_executable "${TARGET_RANLIB}"

mkdir -p \
    "${BUILD_ROOT}" \
    "${DIST_DIR}" \
    "${CXX_QT_EXPORT_DIR}" \
    "${TOOLCHAIN_COMPAT_BIN}"

# openssl-src derives prefixed binutil names from the Android target triple.
# Modern NDKs ship LLVM binutils without those prefixed aliases, so expose
# deterministic compatibility links while also passing explicit Cargo env.
ln -sfn "${TARGET_AR}" "${TOOLCHAIN_COMPAT_BIN}/aarch64-linux-android-ar"
ln -sfn "${TARGET_RANLIB}" "${TOOLCHAIN_COMPAT_BIN}/aarch64-linux-android-ranlib"

printf '\n==> Verifying and materializing Mukei branding v3.2\n'
python3 "${SCRIPT_DIR}/prepare-branding.py" verify
python3 "${SCRIPT_DIR}/prepare-branding.py" materialize \
    --repo-root "${REPO_ROOT}" \
    --state "${BRANDING_STATE}"

printf '\n==> Building llama.cpp capsule for %s\n' "${ABI}"
cmake \
    -S "${REPO_ROOT}/rust/llama-cpp-prebuilt" \
    -B "${LLAMA_BUILD_DIR}" \
    -G Ninja \
    -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_TOOLCHAIN_FILE="${ANDROID_NDK_ROOT}/build/cmake/android.toolchain.cmake" \
    -DANDROID_ABI="${ABI}" \
    -DANDROID_PLATFORM="android-${ANDROID_API}" \
    -DMUKEI_LLAMA_FORCE_REBUILD=ON
cmake --build "${LLAMA_BUILD_DIR}" --target mukei_llama_native --parallel
[[ -f "${LLAMA_LIB}" ]] || fail "native capsule was not produced: ${LLAMA_LIB}"

printf '\n==> Cross-compiling Rust/CXX-Qt bridge for %s\n' "${RUST_TARGET}"
(
    cd "${REPO_ROOT}/rust"
    env \
        PATH="${TOOLCHAIN_COMPAT_BIN}:${PATH}" \
        QMAKE="${QT_ANDROID_ROOT}/bin/qmake" \
        QT_HOST_PATH="${QT_HOST_ROOT}" \
        CXX_QT_EXPORT_DIR="${CXX_QT_EXPORT_DIR}" \
        'CXX_QT_EXPORT_CRATE_mukei-bridge=1' \
        CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="${TARGET_CC}" \
        CARGO_TARGET_AARCH64_LINUX_ANDROID_AR="${TARGET_AR}" \
        CC_aarch64_linux_android="${TARGET_CC}" \
        CXX_aarch64_linux_android="${TARGET_CXX}" \
        AR_aarch64_linux_android="${TARGET_AR}" \
        RANLIB_aarch64_linux_android="${TARGET_RANLIB}" \
        RANLIB="${TARGET_RANLIB}" \
        NK_TARGET_NEON=1 \
        NK_TARGET_NEONHALF=0 \
        NK_TARGET_NEONSDOT=0 \
        NK_TARGET_NEONBFDOT=0 \
        NK_TARGET_NEONFHM=0 \
        NK_TARGET_SVE=0 \
        NK_TARGET_SVEHALF=0 \
        NK_TARGET_SVEBFDOT=0 \
        NK_TARGET_SVESDOT=0 \
        NK_TARGET_SVE2=0 \
        NK_TARGET_SVE2P1=0 \
        NK_TARGET_NEONFP8=0 \
        NK_TARGET_SME=0 \
        NK_TARGET_SME2=0 \
        NK_TARGET_SME2P1=0 \
        NK_TARGET_SMEF64=0 \
        NK_TARGET_SMEHALF=0 \
        NK_TARGET_SMEBF16=0 \
        NK_TARGET_SMEBI32=0 \
        NK_TARGET_SMELUT2=0 \
        NK_TARGET_SMEFA64=0 \
        cargo build \
            -p mukei-bridge \
            --profile android-release \
            --target "${RUST_TARGET}" \
            --no-default-features \
            --features "shipping_native,android_keystore,runtime_hardening"
)
[[ -f "${BRIDGE_LIB}" ]] || fail "Rust bridge static library was not produced: ${BRIDGE_LIB}"
[[ -f "${CXX_QT_EXPORT_DIR}/crates/mukei-bridge/include/mukei-bridge/src/lib.cxxqt.h" ]] || \
    fail "CXX-Qt export header was not produced"

printf '\n==> Configuring Qt Android application\n'
"${QT_ANDROID_ROOT}/bin/qt-cmake" \
    -DANDROID_ABI="${ABI}" \
    -C "${ANDROID_INITIAL_CACHE}" \
    -S "${REPO_ROOT}/qml" \
    -B "${QML_BUILD_DIR}" \
    -G Ninja \
    -DCMAKE_BUILD_TYPE="${BUILD_TYPE}" \
    -DANDROID_PLATFORM="android-${ANDROID_API}" \
    -DANDROID_SDK_ROOT="${ANDROID_SDK_ROOT}" \
    -DANDROID_NDK_ROOT="${ANDROID_NDK_ROOT}" \
    -DQT_HOST_PATH="${QT_HOST_ROOT}" \
    -DQT_ANDROID_BUILD_ALL_ABIS=OFF \
    -DMUKEI_BRIDGE_LIB="${BRIDGE_LIB}" \
    -DMUKEI_LLAMA_NATIVE_LIB="${LLAMA_LIB}" \
    -DMUKEI_CXX_QT_EXPORT_DIR="${CXX_QT_EXPORT_DIR}"

cmake --build "${QML_BUILD_DIR}" --parallel
cmake --build "${QML_BUILD_DIR}" --target apk --parallel

mapfile -t apk_candidates < <(find "${QML_BUILD_DIR}" -type f -name '*.apk' -print | sort)
((${#apk_candidates[@]} > 0)) || fail "Qt/Gradle completed without producing an APK"

apk_path="${apk_candidates[-1]}"
product_version="$(python3 - "${REPO_ROOT}/rust/Cargo.toml" <<'PY'
import pathlib
import re
import sys

text = pathlib.Path(sys.argv[1]).read_text(encoding="utf-8")
match = re.search(r'^version\s*=\s*"([0-9]+\.[0-9]+\.[0-9]+)"', text, re.MULTILINE)
if not match:
    raise SystemExit("workspace version not found")
print(match.group(1))
PY
)"

final_apk="${DIST_DIR}/mukei-${product_version}-${ABI}.apk"
cp -f "${apk_path}" "${final_apk}"
bash "${SCRIPT_DIR}/validate-apk.sh" "${final_apk}"

cleanup_branding
trap - EXIT
printf '\nAPK ready: %s\n' "${final_apk}"
