#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root/rust"

cargo test -p mukei-core --lib
cargo test -p mukei-core --lib tools::

# Isolated encrypted-storage contract: hardening is covered by the production
# composition below, where Candle and USearch are necessarily enabled.
cargo check -p mukei-core \
  --no-default-features \
  --features std,tokio,sqlcipher

# Isolated JNI lifecycle/bootstrap contracts do not require the production RAG
# dependency graph.
cargo test -p mukei-android-jni --lib \
  --no-default-features \
  --features secure_runtime

# Hardened production RAG composition must include Candle and USearch.
cargo check -p mukei-android-jni \
  --no-default-features \
  --features secure_runtime,rag_runtime,runtime_hardening

cd "$repo_root"
cmake -S rust/llama-cpp-prebuilt \
  -B build/llama-host \
  -G Ninja \
  -DCMAKE_BUILD_TYPE=Release \
  -DMUKEI_LLAMA_FORCE_REBUILD=ON
cmake --build build/llama-host --target mukei_llama_native_smoke
ctest --test-dir build/llama-host --output-on-failure

cd "$repo_root/rust"
MUKEI_LLAMA_NATIVE_LIB_DIR="$repo_root/rust/llama-cpp-prebuilt/prebuilt/host" \
  cargo check -p mukei-android-jni \
    --no-default-features \
    --features native_inference
