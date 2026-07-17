#!/usr/bin/env bash
set -euo pipefail

: "${ANDROID_HOME:?ANDROID_HOME is required}"
: "${ANDROID_NDK_VERSION:?ANDROID_NDK_VERSION is required}"
: "${GITHUB_WORKSPACE:?GITHUB_WORKSPACE is required}"

ndk="$ANDROID_HOME/ndk/${ANDROID_NDK_VERSION}"
test -d "$ndk"

output="$GITHUB_WORKSPACE/android/core/native/src/main/jniLibs"
rm -rf "$output"
mkdir -p "$output"

for abi in arm64-v8a x86_64; do
  echo "::group::Build native capsule for ${abi}"
  cmake -S rust/llama-cpp-prebuilt \
    -B "build/llama-$abi" \
    -G Ninja \
    -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_TOOLCHAIN_FILE="$ndk/build/cmake/android.toolchain.cmake" \
    -DANDROID_ABI="$abi" \
    -DANDROID_PLATFORM=android-26 \
    -DANDROID_STL=c++_static \
    -DMUKEI_LLAMA_FORCE_REBUILD=ON
  cmake --build "build/llama-$abi" --target mukei_llama_native
  echo "::endgroup::"

  export MUKEI_LLAMA_NATIVE_LIB_DIR="$GITHUB_WORKSPACE/rust/llama-cpp-prebuilt/prebuilt/$abi"
  test -s "$MUKEI_LLAMA_NATIVE_LIB_DIR/libmukei_llama_native.so"

  echo "::group::Build JNI runtime for ${abi}"
  (
    cd rust
    cargo ndk -t "$abi" -o ../android/core/native/src/main/jniLibs \
      build -p mukei-android-jni --release \
      --no-default-features \
      --features runtime_production,runtime_hardening
  )
  echo "::endgroup::"

  test -s "$output/$abi/libmukei_android.so"
  cp "$MUKEI_LLAMA_NATIVE_LIB_DIR/libmukei_llama_native.so" "$output/$abi/"
  test -s "$output/$abi/libmukei_llama_native.so"
done
