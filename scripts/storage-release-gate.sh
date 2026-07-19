#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

V015_PATH="rust/migrations/V015__workspace_scope_isolation_guards.sql"
EXPECTED_V015_BLOB="9804b7f325ac49c86fe0799f3f8d0bddc4cac57f"

printf '==> Verifying frozen V015 migration bytes\n'
actual_v015_blob="$(git hash-object "$V015_PATH")"
if [[ "$actual_v015_blob" != "$EXPECTED_V015_BLOB" ]]; then
  printf 'ERROR: canonical V015 changed. expected=%s actual=%s\n' \
    "$EXPECTED_V015_BLOB" "$actual_v015_blob" >&2
  exit 1
fi

printf '==> Verifying V016 exists and declares schema version 16\n'
grep -q "PRAGMA user_version = 16;" \
  rust/migrations/V016__storage_identity_and_recovery_hardening.sql

cd rust

printf '==> rustfmt\n'
cargo fmt --all -- --check

printf '==> Embedded migration contract\n'
cargo test --locked -p mukei-core --features rusqlite --test embedded_migrations

printf '==> Canonical V015 -> V016 forward upgrade\n'
cargo test --locked -p mukei-core --features rusqlite --test universal_storage_v016_forward_upgrade

printf '==> V016 adversarial invariant guards\n'
cargo test --locked -p mukei-core --features rusqlite --test universal_storage_v016_guards

printf '==> Full mukei-core storage-capable test suite\n'
cargo test --locked -p mukei-core --features rusqlite

printf '==> Clippy with warnings denied\n'
cargo clippy --locked -p mukei-core --features rusqlite --all-targets -- -D warnings

printf '==> Release gate runner passed\n'
