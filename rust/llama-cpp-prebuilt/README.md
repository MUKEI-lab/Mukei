# Mukei llama.cpp native capsule

Mukei ships llama.cpp through one Mukei-owned shared ABI capsule rather than
linking a lone `libllama.a` into downstream consumers. The capsule absorbs the
pinned llama/GGML static dependency closure and exports only the stable C
surface in `shim/mukei_llama_native.h`.

## Provenance contract

- Vendored llama.cpp commit: `7c082bc417bbe53210a83df4ba5b49e18ce6193c`
- Mukei native ABI version: `1`
- Runtime activation rejects a capsule whose ABI version or build ID differs.
- GGUF artifacts are full-file SHA-256 verified in Rust before the native loader
  is allowed to open/mmap them.

## Build for Android

```bash
for ABI in arm64-v8a armeabi-v7a x86_64; do
  cmake -S rust/llama-cpp-prebuilt \
        -B build/llama-$ABI \
        -DCMAKE_BUILD_TYPE=Release \
        -DCMAKE_TOOLCHAIN_FILE="$ANDROID_NDK/build/cmake/android.toolchain.cmake" \
        -DANDROID_ABI="$ABI" \
        -DANDROID_PLATFORM=android-29 \
        -DMUKEI_LLAMA_FORCE_REBUILD=ON
  cmake --build build/llama-$ABI --target mukei_llama_native_smoke
  ctest --test-dir build/llama-$ABI --output-on-failure
 done
```

The resulting capsule is written to:

`rust/llama-cpp-prebuilt/prebuilt/<abi>/libmukei_llama_native.so`

## Build for host validation

```bash
cmake -S rust/llama-cpp-prebuilt -B build/llama-host \
      -G Ninja -DCMAKE_BUILD_TYPE=Release -DMUKEI_LLAMA_FORCE_REBUILD=ON
cmake --build build/llama-host --target mukei_llama_native_smoke
ctest --test-dir build/llama-host --output-on-failure
```

## Rust production build

The `mukei-bridge/runtime_production` feature enables the real native inference
adapter and therefore requires the capsule to exist. Point Cargo at the ABI
output directory when it is not in the default `prebuilt/<abi>` location:

```bash
export MUKEI_LLAMA_NATIVE_LIB_DIR="$PWD/rust/llama-cpp-prebuilt/prebuilt/host"
cargo build -p mukei-bridge --no-default-features \
  --features "sqlcipher,network,runtime_production"
```

The bridge build fails closed if the required capsule is missing. Android
packaging additionally links and packages the same capsule through
`QT_ANDROID_EXTRA_LIBS`; source/host CI does not replace a real Android ABI
build and physical-device inference validation.
