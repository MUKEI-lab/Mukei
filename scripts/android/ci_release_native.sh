#!/usr/bin/env bash
set -euo pipefail

: "${ANDROID_HOME:?ANDROID_HOME is required}"
: "${ANDROID_NDK_VERSION:?ANDROID_NDK_VERSION is required}"
: "${GITHUB_WORKSPACE:?GITHUB_WORKSPACE is required}"

ndk="$ANDROID_HOME/ndk/${ANDROID_NDK_VERSION}"
test -d "$ndk"

# cargo-ndk otherwise auto-discovers the newest runner NDK, which can diverge
# from the version used by the CMake capsule build.
export ANDROID_NDK_HOME="$ndk"
export ANDROID_NDK_ROOT="$ndk"

# Resolve the exact production graph before cross-compilation, then normalize
# NumKong's known Android NDK 27 syscall declaration conflict. The patch script
# validates the exact crate version and declaration and fails closed on drift.
cargo fetch --manifest-path "$GITHUB_WORKSPACE/rust/Cargo.toml"
bash "$GITHUB_WORKSPACE/scripts/android/patch_numkong_android.sh"

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
    if [[ "$abi" == "x86_64" ]]; then
      # Android emulators do not need server-only AVX-512 BF16/AMX kernels.
      # NDK clang 18 can crash while selecting those NumKong instructions,
      # so retain the portable/Haswell/Skylake/Icelake dispatch set and omit
      # the problematic server generations for the emulator ABI.
      env \
        NK_TARGET_GENOA=0 \
        NK_TARGET_SAPPHIRE=0 \
        NK_TARGET_SAPPHIREAMX=0 \
        NK_TARGET_GRANITEAMX=0 \
        NK_TARGET_DIAMOND=0 \
        NK_TARGET_TURIN=0 \
        cargo ndk -t "$abi" -o ../android/core/native/src/main/jniLibs \
          build -p mukei-android-jni --release \
          --no-default-features \
          --features runtime_production,runtime_hardening
    else
      cargo ndk -t "$abi" -o ../android/core/native/src/main/jniLibs \
        build -p mukei-android-jni --release \
        --no-default-features \
        --features runtime_production,runtime_hardening
    fi
  )
  echo "::endgroup::"

  test -s "$output/$abi/libmukei_android.so"
  cp "$MUKEI_LLAMA_NATIVE_LIB_DIR/libmukei_llama_native.so" "$output/$abi/"
  test -s "$output/$abi/libmukei_llama_native.so"

  # libmukei_android.so links against the NDK shared C++ runtime through its
  # native inference dependency graph. Android does not provide libc++_shared.so
  # as a system library, so it must be packaged per ABI or System.loadLibrary()
  # fails on-device before JNI bootstrap can begin.
  case "$abi" in
    arm64-v8a) cxx_triple="aarch64-linux-android" ;;
    x86_64) cxx_triple="x86_64-linux-android" ;;
    *) echo "Unsupported ABI for libc++ runtime: $abi" >&2; exit 1 ;;
  esac
  cxx_shared="$(find "$ndk/toolchains/llvm/prebuilt" -path "*/sysroot/usr/lib/$cxx_triple/libc++_shared.so" -print -quit)"
  test -n "$cxx_shared"
  test -s "$cxx_shared"
  cp "$cxx_shared" "$output/$abi/libc++_shared.so"
  test -s "$output/$abi/libc++_shared.so"
done
