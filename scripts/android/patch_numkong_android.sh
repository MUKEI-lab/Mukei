#!/usr/bin/env bash
set -euo pipefail

# NumKong 7.7.0 redeclares bionic's syscall() with a C++ noexcept
# specification. Android NDK 27 declares the same C function without that
# specification, and Clang correctly rejects the mismatch. Patch only the
# exact known dependency/header/declaration; any upstream drift fails closed.

cargo_home="${CARGO_HOME:-$HOME/.cargo}"
registry_root="$cargo_home/registry/src"
expected_version="numkong-7.7.0"
expected='extern "C" long syscall(long, ...) noexcept;'
replacement='extern "C" long syscall(long, ...);'

if [[ ! -d "$registry_root" ]]; then
  echo "Cargo registry source directory is missing: $registry_root" >&2
  exit 1
fi

mapfile -t headers < <(
  find "$registry_root" -type f \
    -path "*/${expected_version}/include/numkong/capabilities.h" \
    -print | sort
)

if [[ "${#headers[@]}" -ne 1 ]]; then
  printf 'Expected exactly one %s capabilities header, found %s\n' \
    "$expected_version" "${#headers[@]}" >&2
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
print(f"Normalized Android syscall declaration in {path}")
print(f"Patched header sha256={digest}")
PY
