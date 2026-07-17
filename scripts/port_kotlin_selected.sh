#!/usr/bin/env bash
set -euo pipefail
export GIT_EDITOR=true

branch="temp/universal-storage-workspace-v0.1"
git config user.name "mukei-storage-bot"
git config user.email "mukei-storage-bot@users.noreply.github.com"
git fetch --no-tags origin Kotlin:refs/remotes/origin/Kotlin

python <<'PY'
from pathlib import Path

registry_path = Path('rust/crates/mukei-android-jni/src/runtime_registry.rs')
registry = registry_path.read_text(encoding='utf-8')
old_imports = (
    'use jni::objects::{JByteArray, JObject};\n'
    'use jni::sys::{jbyteArray, jlong};\n'
    'use jni::JNIEnv;\n'
    'use mukei_core::application_runtime::MukeiRuntime;\n'
    'use zeroize::{Zeroize, Zeroizing};\n'
)
new_imports = (
    'use jni::sys::jlong;\n'
    'use mukei_core::application_runtime::MukeiRuntime;\n\n'
    '// Child JNI modules intentionally access these crate-root helpers through this\n'
    '// module. Keeping each transport in a real Rust module prevents textual\n'
    '// `include!` import collisions while preserving the exported JNI symbol names.\n'
    'use super::{\n'
    '    guarded_bytes, invalid_handle_payload, runtime_entry, runtime_services, serialize,\n'
    '    to_java_bytes, RUNTIMES,\n'
    '};\n'
)
if old_imports in registry:
    registry = registry.replace(old_imports, new_imports, 1)
registry = registry.replace(
    'include!("secure_runtime_jni.rs");\ninclude!("remote_tools_jni.rs");',
    '#[path = "secure_runtime_jni.rs"]\nmod secure_runtime_jni;\n#[path = "remote_tools_jni.rs"]\nmod remote_tools_jni;',
    1,
)
registry_path.write_text(registry, encoding='utf-8')

secure_path = Path('rust/crates/mukei-android-jni/src/secure_runtime_jni.rs')
secure = secure_path.read_text(encoding='utf-8')
secure_imports = (
    'use jni::objects::{JByteArray, JObject};\n'
    'use jni::sys::jbyteArray;\n'
    'use jni::JNIEnv;\n'
    'use zeroize::Zeroize;\n\n'
)
if not secure.startswith('use jni::'):
    secure = secure_imports + secure
secure_path.write_text(secure, encoding='utf-8')

remote_path = Path('rust/crates/mukei-android-jni/src/remote_tools_jni.rs')
remote = remote_path.read_text(encoding='utf-8')
remote_imports = (
    'use jni::objects::{JByteArray, JObject};\n'
    'use jni::sys::{jbyteArray, jlong};\n'
    'use jni::JNIEnv;\n'
    'use zeroize::{Zeroize, Zeroizing};\n\n'
)
if not remote.startswith('use jni::'):
    remote = remote_imports + remote
remote_path.write_text(remote, encoding='utf-8')
PY

rustfmt --edition 2021 rust/crates/mukei-android-jni/src/runtime_registry.rs
rustfmt --edition 2021 rust/crates/mukei-android-jni/src/secure_runtime_jni.rs
rustfmt --edition 2021 rust/crates/mukei-android-jni/src/remote_tools_jni.rs
if ! git diff --quiet -- rust/crates/mukei-android-jni/src/runtime_registry.rs rust/crates/mukei-android-jni/src/secure_runtime_jni.rs rust/crates/mukei-android-jni/src/remote_tools_jni.rs; then
  git add rust/crates/mukei-android-jni/src/runtime_registry.rs \
          rust/crates/mukei-android-jni/src/secure_runtime_jni.rs \
          rust/crates/mukei-android-jni/src/remote_tools_jni.rs
  git commit -m "fix(jni): isolate secure transport modules" \
    -m "Selective port of Kotlin commit 127524b089a7b4dee4ad28110be98488391550a1, reconciled with encrypted storage composition."
fi

picks=(
  587dc2e2b277ee43328b998c91f984fdb9e0f62f
  c0ef6cd10cc84e1f77abc6c2e3b2da5a192495ba
  f969f2aeca821994d7c50c19e917f431662d701c
  77c6f7fb3558329c096fc7d90f806a927004b3f4
  9c3c57a8b8faf4e26dd862fba7d20109da96a321
  94d32d4db8a2aa250b5e4afa618825aba5cb938f
  29f2b764768707b509ba04aad123baf721d93983
  2196c690d9ecfdee2a91f33741f280208d1f3762
  5674765ce56aea807f9a1048607935e41d31070a
  91f4c18bd4f0c870300a072d77c36680e5003e71
  ed532aefb80825aeb3b607f536f5ed963e2bcb78
  26ca8b2255e261167c87152dfdf9c50791d01cfb
  e9233b4b52a8f801769b72f4acfa60c465a32345
  65c68d6951ab4fd06195d783eca444aa41233d1a
)

for sha in "${picks[@]}"; do
  if git cherry-pick -x "$sha"; then
    continue
  fi
  case "$sha" in
    587dc2e2b277ee43328b998c91f984fdb9e0f62f)
      git checkout --theirs -- rust/crates/mukei-core/src/application_runtime/foundation_state.rs ;;
    c0ef6cd10cc84e1f77abc6c2e3b2da5a192495ba)
      git checkout --theirs -- rust/crates/mukei-core/src/application_runtime/persistence_flush.rs ;;
    f969f2aeca821994d7c50c19e917f431662d701c|77c6f7fb3558329c096fc7d90f806a927004b3f4)
      git checkout --theirs -- android/app/src/main/kotlin/ai/mukei/android/BackendRuntimeHost.kt ;;
    9c3c57a8b8faf4e26dd862fba7d20109da96a321|94d32d4db8a2aa250b5e4afa618825aba5cb938f|26ca8b2255e261167c87152dfdf9c50791d01cfb)
      git checkout --theirs -- rust/crates/mukei-core/src/application_runtime/tests.rs ;;
    29f2b764768707b509ba04aad123baf721d93983|2196c690d9ecfdee2a91f33741f280208d1f3762|5674765ce56aea807f9a1048607935e41d31070a|e9233b4b52a8f801769b72f4acfa60c465a32345|65c68d6951ab4fd06195d783eca444aa41233d1a)
      git checkout --theirs -- .github/workflows/android-kotlin-ci.yml ;;
    91f4c18bd4f0c870300a072d77c36680e5003e71)
      git checkout --ours -- rust/crates/mukei-core/src/application_runtime.rs ;;
    ed532aefb80825aeb3b607f536f5ed963e2bcb78)
      git checkout --theirs -- rust/crates/mukei-core/src/application_runtime/foundation_types.rs ;;
    *)
      git status --short
      exit 1 ;;
  esac
  git add -A
  if git diff --cached --quiet; then
    git cherry-pick --skip
  else
    git cherry-pick --continue
  fi
done

python <<'PY'
from pathlib import Path

runtime = Path('rust/crates/mukei-core/src/application_runtime.rs')
text = runtime.read_text(encoding='utf-8')
text = text.replace(
    'include!("application_runtime/durable.rs");\ninclude!("application_runtime/foundation_types.rs");',
    'include!("application_runtime/foundation_types.rs");\ninclude!("application_runtime/durable.rs");',
    1,
)
if 'include!("application_runtime/storage_import.rs");' not in text:
    text = text.replace(
        'include!("application_runtime/documents_snapshot.rs");\n',
        'include!("application_runtime/documents_snapshot.rs");\ninclude!("application_runtime/storage_import.rs");\n',
        1,
    )
if 'include!("application_runtime/storage_import_tests.rs");' not in text:
    text += 'include!("application_runtime/storage_import_tests.rs");\n'
runtime.write_text(text, encoding='utf-8')

host = Path('android/app/src/main/kotlin/ai/mukei/android/BackendRuntimeHost.kt')
text = host.read_text(encoding='utf-8')
if 'security.optString("object_store", "unknown")' not in text:
    text = text.replace(
        'security.optString("projections", "unknown"),\n',
        'security.optString("projections", "unknown"),\n                    security.optString("object_store", "unknown"),\n',
        1,
    )
host.write_text(text, encoding='utf-8')

workflow = Path('.github/workflows/android-kotlin-ci.yml')
text = workflow.read_text(encoding='utf-8')
text = text.replace(
    'branches: [ Kotlin ]',
    'branches: [ Kotlin, temp/universal-storage-workspace-v0.1 ]',
    1,
)
workflow.write_text(text, encoding='utf-8')
PY

rm -f .github/workflows/kotlin-selective-port-audit.yml \
      .github/workflows/kotlin-selective-port.yml \
      .github/workflows/kotlin-port-conflict-diagnostic.yml \
      .github/workflows/kotlin-port-resolution-diagnostic.yml \
      .github/workflows/kotlin-port-resolution-diagnostic-v2.yml \
      .github/workflows/kotlin-selective-port-v2.yml
rm -f scripts/port_kotlin_selected.sh

git add -A
rustfmt --edition 2021 rust/crates/mukei-android-jni/src/runtime_registry.rs
rustfmt --edition 2021 rust/crates/mukei-android-jni/src/secure_runtime_jni.rs
rustfmt --edition 2021 rust/crates/mukei-android-jni/src/remote_tools_jni.rs
rustfmt --edition 2021 rust/crates/mukei-core/src/application_runtime.rs
rustfmt --edition 2021 rust/crates/mukei-core/src/application_runtime/foundation_state.rs
rustfmt --edition 2021 rust/crates/mukei-core/src/application_runtime/foundation_types.rs
rustfmt --edition 2021 rust/crates/mukei-core/src/application_runtime/persistence_flush.rs
rustfmt --edition 2021 rust/crates/mukei-core/src/application_runtime/tests.rs
git add -A
git diff --cached --check

(
  cd rust
  export RUSTFLAGS='-D warnings'
  cargo check -p mukei-core --no-default-features --features std,tokio,rusqlite --all-targets
  cargo test -p mukei-core --no-default-features --features std,tokio,rusqlite --all-targets
  cargo clippy -p mukei-core --no-default-features --features std,tokio,rusqlite --all-targets -- -D warnings
  cargo test -p mukei-android-jni --lib --no-default-features --features secure_runtime
)

gradle -p android \
  :app:assembleDebug \
  :app:assembleRelease \
  :app:assembleOffline \
  :app:testDebugUnitTest \
  :app:lintDebug \
  --stacktrace

git commit -m "fix(port): reconcile Kotlin runtime fixes with universal storage"
git push origin HEAD:"$branch"
