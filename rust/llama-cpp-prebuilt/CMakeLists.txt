cmake_minimum_required(VERSION 3.22)
project(mukei_llama_prebuilt C CXX)

# TRD §8.2
# One-shot prebuild strategy:
#   1. Build libllama.a once per ABI outside per-PR CI.
#   2. Place the archive under prebuilt/<abi>/libllama.a.
#   3. Rust/CMake consumers import the static archive directly.

set(CMAKE_C_STANDARD 11)
set(CMAKE_CXX_STANDARD 17)

if(ANDROID)
  set(MUKEI_TARGET_ABI "${ANDROID_ABI}")
else()
  set(MUKEI_TARGET_ABI "host")
endif()

set(MUKEI_PREBUILT_ROOT "${CMAKE_CURRENT_LIST_DIR}/prebuilt")
set(MUKEI_PREBUILT_LIB  "${MUKEI_PREBUILT_ROOT}/${MUKEI_TARGET_ABI}/libllama.a")
set(MUKEI_VENDOR_DIR    "${CMAKE_CURRENT_LIST_DIR}/vendor/llama.cpp")

if(EXISTS "${MUKEI_PREBUILT_LIB}")
  add_library(mukei_llama STATIC IMPORTED GLOBAL)
  set_target_properties(mukei_llama PROPERTIES
    IMPORTED_LOCATION "${MUKEI_PREBUILT_LIB}"
    INTERFACE_INCLUDE_DIRECTORIES "${MUKEI_VENDOR_DIR}/include"
  )
  message(STATUS "Using prebuilt llama archive: ${MUKEI_PREBUILT_LIB}")
  return()
endif()

message(WARNING "Prebuilt libllama.a missing for ABI '${MUKEI_TARGET_ABI}'. Falling back to one-shot vendor build.")

if(NOT EXISTS "${MUKEI_VENDOR_DIR}/CMakeLists.txt")
  message(FATAL_ERROR "Missing vendor llama.cpp tree at ${MUKEI_VENDOR_DIR}")
endif()

file(MAKE_DIRECTORY "${MUKEI_PREBUILT_ROOT}/${MUKEI_TARGET_ABI}")

# Keep the fallback self-contained and library-only.  The vendored snapshot
# intentionally omits upstream apps, tools, examples, and tests, and the
# prebuilt cache expects a static archive at prebuilt/<abi>/libllama.a.
set(BUILD_SHARED_LIBS OFF CACHE BOOL "Build llama.cpp as static libraries" FORCE)
set(LLAMA_BUILD_COMMON OFF CACHE BOOL "Do not build upstream common utilities" FORCE)
set(LLAMA_BUILD_TESTS OFF CACHE BOOL "Do not build upstream tests" FORCE)
set(LLAMA_BUILD_TOOLS OFF CACHE BOOL "Do not build upstream tools" FORCE)
set(LLAMA_BUILD_EXAMPLES OFF CACHE BOOL "Do not build upstream examples" FORCE)
set(LLAMA_BUILD_SERVER OFF CACHE BOOL "Do not build upstream server" FORCE)
set(LLAMA_BUILD_APP OFF CACHE BOOL "Do not build upstream app" FORCE)
set(GGML_BUILD_TESTS OFF CACHE BOOL "Do not build upstream ggml tests" FORCE)
set(GGML_BUILD_EXAMPLES OFF CACHE BOOL "Do not build upstream ggml examples" FORCE)

# Persist the static fallback directly at the prebuilt cache path.  CMake
# does not allow attaching a POST_BUILD command to a target created by
# add_subdirectory() from this parent directory.
set(CMAKE_ARCHIVE_OUTPUT_DIRECTORY "${MUKEI_PREBUILT_ROOT}/${MUKEI_TARGET_ABI}")

add_subdirectory("${MUKEI_VENDOR_DIR}" "${CMAKE_BINARY_DIR}/llama-build")

add_library(mukei_llama ALIAS llama)
