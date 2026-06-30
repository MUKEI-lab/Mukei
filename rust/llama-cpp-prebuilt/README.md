# llama.cpp prebuilt artifacts

This directory hosts the **one-shot per-ABI** prebuilt `libllama.a` archive that
`mukei-bridge` links against (TRD §8.2). Building llama.cpp on every PR is
slow and brittle, so the workflow is:

1. Keep a self-contained llama.cpp snapshot under `vendor/llama.cpp/`.
2. Run the one-shot CMake build per Android ABI (or per desktop target).
3. Place the resulting archive at `prebuilt/<abi>/libllama.a`.
4. Rust consumers (under `feature = "llama_cpp"`) link the static archive
   directly. No per-PR recompilation.

## Vendored llama.cpp

The vendor tree is a checked-in source snapshot of
[`https://github.com/ggerganov/llama.cpp`](https://github.com/ggerganov/llama.cpp),
not a Git submodule. ZIP downloads and offline CI builds therefore contain the
llama.cpp sources needed for the library fallback build.

See [`VENDORED_SNAPSHOT.md`](VENDORED_SNAPSHOT.md) for the pinned upstream
commit, the omitted upstream paths, and the update procedure. Do not reintroduce
`.gitmodules` or a gitlink at `vendor/llama.cpp` when updating the snapshot.

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
├── VENDORED_SNAPSHOT.md  ← pinned snapshot and update notes
├── prebuilt/             ← .gitignored; holds the produced archives
│   ├── arm64-v8a/libllama.a
│   ├── armeabi-v7a/libllama.a
│   ├── x86_64/libllama.a
│   └── host/libllama.a
└── vendor/
    └── llama.cpp/        ← checked-in upstream source snapshot
```

The `prebuilt/` directory is intentionally **not** checked into git — the
archives are large (~30 MB per ABI) and a clean rebuild is reproducible from
the vendored snapshot. CI is expected to cache them out-of-band.
