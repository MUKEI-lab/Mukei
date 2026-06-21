# llama.cpp prebuilt artifacts

This directory hosts the **one-shot per-ABI** prebuilt `libllama.a` archive that
`mukei-bridge` links against (TRD §8.2). Building llama.cpp on every PR is
slow and brittle, so the workflow is:

1. Vendor llama.cpp **once** as a git submodule under
   `vendor/llama.cpp/`.
2. Run the one-shot CMake build per Android ABI (or per desktop target).
3. Place the resulting archive at `prebuilt/<abi>/libllama.a`.
4. Rust consumers (under `feature = "llama_cpp"`) link the static archive
   directly. No per-PR recompilation.

## Vendored llama.cpp

The vendor tree is a **shallow submodule** of
[`https://github.com/ggerganov/llama.cpp`](https://github.com/ggerganov/llama.cpp).
Always clone the workspace with `--recurse-submodules`, or run
`git submodule update --init --depth 1` after a plain clone.

## Build (Android)

```bash
# From the workspace root, for each target ABI:
for ABI in arm64-v8a armeabi-v7a x86_64; do
  cmake -S rust/llama-cpp-prebuilt \
        -B build/llama-$ABI \
        -DCMAKE_TOOLCHAIN_FILE=$ANDROID_NDK/build/cmake/android.toolchain.cmake \
        -DANDROID_ABI=$ABI \
        -DANDROID_PLATFORM=android-29 \
        -DMUKEI_LLAMA_BUILD_SHARED=OFF
  cmake --build build/llama-$ABI --target llama
done
```

## Build (desktop host, for tests)

```bash
cmake -S rust/llama-cpp-prebuilt -B build/llama-host
cmake --build build/llama-host --target llama
```

## Layout

```
llama-cpp-prebuilt/
├── CMakeLists.txt        ← orchestrates the one-shot build
├── README.md             ← this file
├── prebuilt/             ← .gitignored; holds the produced archives
│   ├── arm64-v8a/libllama.a
│   ├── armeabi-v7a/libllama.a
│   ├── x86_64/libllama.a
│   └── host/libllama.a
└── vendor/
    └── llama.cpp/        ← shallow git submodule, pinned by SHA
```

The `prebuilt/` directory is intentionally **not** checked into git — the
archives are large (~30 MB per ABI) and a clean rebuild is reproducible from
the submodule SHA. CI is expected to cache them out-of-band.
