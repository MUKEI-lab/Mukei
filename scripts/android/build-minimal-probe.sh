#!/usr/bin/env bash
set -Eeuo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
readonly SCRIPT_DIR
REPO_ROOT="$(cd -- "${SCRIPT_DIR}/../.." && pwd)"
readonly REPO_ROOT
readonly VARIANT="${1:?usage: build-minimal-probe.sh <qt-widgets-raster|inline-qml-software>}"
readonly ABI="arm64-v8a"
readonly ANDROID_API="${ANDROID_API:-29}"
readonly BUILD_TYPE="${BUILD_TYPE:-Release}"
readonly BUILD_ROOT="${BUILD_ROOT:-${REPO_ROOT}/build/android-probe-${VARIANT}}"
readonly DIST_DIR="${DIST_DIR:-${REPO_ROOT}/dist/android-probes}"
readonly BRANDING_STATE="${BUILD_ROOT}/branding-materialization.json"

case "${VARIANT}" in
    qt-widgets-raster|inline-qml-software) ;;
    *) echo "unsupported probe variant: ${VARIANT}" >&2; exit 2 ;;
esac

fail() { printf 'error: %s\n' "$*" >&2; exit 1; }
require_dir() { [[ -d "$1" ]] || fail "required directory not found: $1"; }
require_exe() { [[ -x "$1" ]] || fail "required executable not found: $1"; }
require_cmd() { command -v "$1" >/dev/null 2>&1 || fail "required command not found: $1"; }

cleanup_branding() {
    python3 "${SCRIPT_DIR}/prepare-branding.py" cleanup \
        --repo-root "${REPO_ROOT}" \
        --state "${BRANDING_STATE}" || true
}
trap cleanup_branding EXIT

: "${ANDROID_SDK_ROOT:=${ANDROID_HOME:-}}"
: "${ANDROID_NDK_ROOT:=${ANDROID_NDK_HOME:-}}"
: "${QT_ANDROID_ROOT:=}"
: "${QT_HOST_ROOT:=}"

[[ -n "${ANDROID_SDK_ROOT}" ]] || fail "set ANDROID_SDK_ROOT"
[[ -n "${ANDROID_NDK_ROOT}" ]] || fail "set ANDROID_NDK_ROOT"
[[ -n "${QT_ANDROID_ROOT}" ]] || fail "set QT_ANDROID_ROOT"
[[ -n "${QT_HOST_ROOT}" ]] || fail "set QT_HOST_ROOT"

require_dir "${ANDROID_SDK_ROOT}"
require_dir "${ANDROID_NDK_ROOT}"
require_exe "${QT_ANDROID_ROOT}/bin/qt-cmake"
require_dir "${QT_HOST_ROOT}"
require_cmd cmake
require_cmd ninja
require_cmd python3
require_cmd unzip

find "${ANDROID_SDK_ROOT}/platforms" -mindepth 1 -maxdepth 1 -type d \
    ! -name 'android-35' -exec rm -rf {} +
[[ -f "${ANDROID_SDK_ROOT}/platforms/android-35/android.jar" ]] || \
    fail "Android platform 35 is unavailable"

mkdir -p "${BUILD_ROOT}" "${DIST_DIR}"

printf '\n==> Materializing verified launcher resources\n'
python3 "${SCRIPT_DIR}/prepare-branding.py" verify
python3 "${SCRIPT_DIR}/prepare-branding.py" materialize \
    --repo-root "${REPO_ROOT}" \
    --state "${BRANDING_STATE}"

printf '\n==> Configuring minimal probe: %s\n' "${VARIANT}"
"${QT_ANDROID_ROOT}/bin/qt-cmake" \
    -S "${REPO_ROOT}/qml" \
    -B "${BUILD_ROOT}" \
    -G Ninja \
    -DCMAKE_BUILD_TYPE="${BUILD_TYPE}" \
    -DANDROID_ABI="${ABI}" \
    -DANDROID_PLATFORM="android-${ANDROID_API}" \
    -DANDROID_SDK_ROOT="${ANDROID_SDK_ROOT}" \
    -DANDROID_NDK_ROOT="${ANDROID_NDK_ROOT}" \
    -DQT_HOST_PATH="${QT_HOST_ROOT}" \
    -DQT_ANDROID_BUILD_ALL_ABIS=OFF \
    -DMUKEI_USE_REAL_BRIDGE=OFF \
    -DMUKEI_USE_NATIVE_INFERENCE=OFF

printf '\n==> Building target-specific Mukei APK\n'
if ! cmake --build "${BUILD_ROOT}" --target mukei_make_apk --parallel; then
    printf '\nAvailable APK-related targets:\n' >&2
    cmake --build "${BUILD_ROOT}" --target help 2>/dev/null | grep -E 'mukei|apk' >&2 || true
    fail "target-specific APK packaging failed"
fi

mapfile -t candidates < <(find "${BUILD_ROOT}" -type f -name '*.apk' -print | sort)
((${#candidates[@]} > 0)) || fail "mukei_make_apk produced no APK"
source_apk="${candidates[-1]}"
output_apk="${DIST_DIR}/mukei-0.7.5-${VARIANT}-unsigned.apk"
cp -f "${source_apk}" "${output_apk}"

unzip -tq "${output_apk}" >/dev/null
unzip -l "${output_apk}" | grep -q 'lib/arm64-v8a/libmukei_arm64-v8a.so'
unzip -l "${output_apk}" | grep -q 'AndroidManifest.xml'
if unzip -l "${output_apk}" | grep -q 'libmukei_llama_native.so'; then
    fail "minimal probe unexpectedly packaged llama native runtime"
fi

cleanup_branding
trap - EXIT
printf '\nMinimal probe APK ready: %s\n' "${output_apk}"
