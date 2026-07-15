# Initial cache for the APK-first Android build.
# Loaded with `cmake -C` before qml/CMakeLists.txt evaluates its cache defaults.

get_filename_component(_MUKEI_QML_DIR "${CMAKE_CURRENT_LIST_DIR}/.." ABSOLUTE)
get_filename_component(_MUKEI_REPO_ROOT "${_MUKEI_QML_DIR}/.." ABSOLUTE)

if(DEFINED ANDROID_ABI AND NOT ANDROID_ABI STREQUAL "arm64-v8a")
    message(FATAL_ERROR
        "Mukei APK-first packaging supports only ANDROID_ABI=arm64-v8a. "
        "Multi-ABI support is deferred to the AAB phase.")
endif()

set(MUKEI_BRIDGE_LIB
    "${_MUKEI_REPO_ROOT}/rust/target/aarch64-linux-android/android-release/libmukei_bridge.a"
    CACHE FILEPATH "Cargo-built arm64 Android mukei-bridge static library" FORCE)

set(MUKEI_CXX_QT_EXPORT_DIR
    "${_MUKEI_REPO_ROOT}/rust/target/cxxqt-export"
    CACHE PATH "Deterministic CXX-Qt export directory" FORCE)

set(MUKEI_LLAMA_NATIVE_LIB
    "${_MUKEI_REPO_ROOT}/rust/llama-cpp-prebuilt/prebuilt/arm64-v8a/libmukei_llama_native.so"
    CACHE FILEPATH "Mukei arm64 Android llama native capsule" FORCE)

set(MUKEI_USE_REAL_BRIDGE ON CACHE BOOL
    "Use the production CXX-Qt bridge for APK packaging" FORCE)
set(MUKEI_USE_NATIVE_INFERENCE ON CACHE BOOL
    "Package the production llama native capsule" FORCE)
