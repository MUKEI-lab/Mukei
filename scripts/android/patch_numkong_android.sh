#!/usr/bin/env bash
set -euo pipefail

# NumKong 7.7.x redeclares bionic's syscall() with a C++ noexcept
# specification. Android NDK 27 declares the same C function without that
# specification, and Clang correctly rejects the mismatch.
#
# `cargo fetch` resolves the production graph immediately before this script
# runs and writes rust/Cargo.lock. Read the exact resolved NumKong version from
# that lockfile instead of hard-coding one patch release. Only the reviewed
# 7.7.x series and the exact known declaration are accepted; any broader
# upstream drift still fails closed.

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cargo_home="${CARGO_HOME:-$HOME/.cargo}"
registry_root="$cargo_home/registry/src"
lockfile="$repo_root/rust/Cargo.lock"
expected='extern "C" long syscall(long, ...) noexcept;'
replacement='extern "C" long syscall(long, ...);'

if [[ ! -d "$registry_root" ]]; then
  echo "Cargo registry source directory is missing: $registry_root" >&2
  exit 1
fi
if [[ ! -s "$lockfile" ]]; then
  echo "Resolved Cargo lockfile is missing: $lockfile" >&2
  exit 1
fi

resolved_version="$(python3 - "$lockfile" <<'PY'
from pathlib import Path
import re
import sys
import tomllib

path = Path(sys.argv[1])
data = tomllib.loads(path.read_text(encoding="utf-8"))
versions = [
    package.get("version", "")
    for package in data.get("package", [])
    if package.get("name") == "numkong"
]
if len(versions) != 1:
    raise SystemExit(
        f"Expected exactly one resolved NumKong package in {path}, found {versions}"
    )
version = versions[0]
if re.fullmatch(r"7\.7\.\d+", version) is None:
    raise SystemExit(
        f"Unsupported NumKong version {version}; reviewed Android patch series is 7.7.x"
    )
print(version)
PY
)"
expected_crate="numkong-$resolved_version"

mapfile -t headers < <(
  find "$registry_root" -type f \
    -path "*/${expected_crate}/include/numkong/capabilities.h" \
    -print | sort
)

if [[ "${#headers[@]}" -ne 1 ]]; then
  printf 'Expected exactly one %s capabilities header, found %s\n' \
    "$expected_crate" "${#headers[@]}" >&2
  printf '%s\n' "${headers[@]:-}" >&2
  exit 1
fi

header="${headers[0]}"
python3 - "$header" "$expected" "$replacement" <<'PY'
from pathlib import Path
import hashlib
import sys

path = Path(sys.argv[1])
expected = sys.argv[2]
replacement = sys.argv[3]
content = path.read_text(encoding="utf-8")
old_count = content.count(expected)
new_count = content.count(replacement)

if old_count == 1 and new_count == 0:
    patched = content.replace(expected, replacement, 1)
    path.write_text(patched, encoding="utf-8")
elif old_count == 0 and new_count == 1:
    patched = content
else:
    raise SystemExit(
        f"Unexpected NumKong syscall declaration state: "
        f"old={old_count}, replacement={new_count}, path={path}"
    )

if patched.count(expected) != 0 or patched.count(replacement) != 1:
    raise SystemExit(f"NumKong patch verification failed: {path}")

digest = hashlib.sha256(path.read_bytes()).hexdigest()
print(f"Resolved NumKong version {path.parents[2].name}")
print(f"Normalized Android syscall declaration in {path}")
print(f"Patched header sha256={digest}")
PY
