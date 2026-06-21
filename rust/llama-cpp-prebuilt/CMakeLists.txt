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
add_subdirectory("${MUKEI_VENDOR_DIR}" "${CMAKE_BINARY_DIR}/llama-build")

add_custom_command(TARGET llama POST_BUILD
  COMMAND ${CMAKE_COMMAND} -E copy_if_different
          $<TARGET_FILE:llama>
          "${MUKEI_PREBUILT_LIB}"
  COMMENT "Persisting one-shot llama build to ${MUKEI_PREBUILT_LIB}"
)

add_library(mukei_llama ALIAS llama)
