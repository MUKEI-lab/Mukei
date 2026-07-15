#!/usr/bin/env bash
set -Eeuo pipefail

fail() {
    printf 'error: %s\n' "$*" >&2
    exit 1
}

[[ $# -eq 1 ]] || fail "usage: $0 path/to/mukei.apk"
apk_path="$1"
[[ -f "${apk_path}" ]] || fail "APK not found: ${apk_path}"
command -v unzip >/dev/null 2>&1 || fail "required command not found: unzip"

unzip -tqq "${apk_path}" || fail "APK ZIP integrity check failed"

entries_file="$(mktemp)"
trap 'rm -f "${entries_file}"' EXIT
unzip -Z1 "${apk_path}" | LC_ALL=C sort > "${entries_file}"

grep -qx 'AndroidManifest.xml' "${entries_file}" || fail "compiled AndroidManifest.xml is missing"
grep -qx 'resources.arsc' "${entries_file}" || fail "compiled Android resources are missing"
grep -qx 'lib/arm64-v8a/libmukei_llama_native.so' "${entries_file}" || \
    fail "arm64-v8a llama native capsule is missing from APK"
grep -Eq '^lib/arm64-v8a/libmukei([^/]*)\.so$' "${entries_file}" || \
    fail "Mukei Qt application library is missing from APK"

mapfile -t packaged_abis < <(
    awk -F/ '/^lib\/[^/]+\// { print $2 }' "${entries_file}" | LC_ALL=C sort -u
)
((${#packaged_abis[@]} > 0)) || fail "APK contains no native ABI libraries"

for abi in "${packaged_abis[@]}"; do
    [[ "${abi}" == 'arm64-v8a' ]] || fail "unexpected ABI packaged in APK-first artifact: ${abi}"
done

if command -v apksigner >/dev/null 2>&1; then
    if apksigner verify "${apk_path}" >/dev/null 2>&1; then
        signature_state="verified"
    else
        signature_state="unsigned-or-unverified"
    fi
else
    signature_state="not-checked"
fi

printf 'APK validation passed\n'
printf '  file: %s\n' "${apk_path}"
printf '  ABI: arm64-v8a\n'
printf '  native capsule: present\n'
printf '  signature: %s\n' "${signature_state}"
