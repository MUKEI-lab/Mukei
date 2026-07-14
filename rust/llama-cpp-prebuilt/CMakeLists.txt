cmake_minimum_required(VERSION 3.22)
project(mukei_llama_native LANGUAGES C CXX)

include(CTest)
set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_POSITION_INDEPENDENT_CODE ON)

if(ANDROID)
    set(MUKEI_LLAMA_TARGET_ABI "${ANDROID_ABI}")
else()
    set(MUKEI_LLAMA_TARGET_ABI "host")
endif()

set(MUKEI_LLAMA_PREBUILT_DIR
    "${CMAKE_CURRENT_SOURCE_DIR}/prebuilt/${MUKEI_LLAMA_TARGET_ABI}"
    CACHE PATH "Output/cache directory for the Mukei llama native capsule")
set(MUKEI_LLAMA_NATIVE_LIBRARY
    "${MUKEI_LLAMA_PREBUILT_DIR}/${CMAKE_SHARED_LIBRARY_PREFIX}mukei_llama_native${CMAKE_SHARED_LIBRARY_SUFFIX}")
option(MUKEI_LLAMA_FORCE_REBUILD "Ignore a cached native capsule and rebuild from the pinned source snapshot" OFF)

if(EXISTS "${MUKEI_LLAMA_NATIVE_LIBRARY}" AND NOT MUKEI_LLAMA_FORCE_REBUILD)
    add_library(mukei_llama_native SHARED IMPORTED GLOBAL)
    set_target_properties(mukei_llama_native PROPERTIES
        IMPORTED_LOCATION "${MUKEI_LLAMA_NATIVE_LIBRARY}")
else()
    set(BUILD_SHARED_LIBS OFF CACHE BOOL "" FORCE)
    set(LLAMA_BUILD_COMMON OFF CACHE BOOL "" FORCE)
    set(LLAMA_BUILD_TESTS OFF CACHE BOOL "" FORCE)
    set(LLAMA_BUILD_EXAMPLES OFF CACHE BOOL "" FORCE)
    set(LLAMA_BUILD_SERVER OFF CACHE BOOL "" FORCE)
    set(LLAMA_CURL OFF CACHE BOOL "" FORCE)
    set(GGML_OPENMP OFF CACHE BOOL "" FORCE)
    set(GGML_VULKAN OFF CACHE BOOL "" FORCE)
    set(GGML_METAL OFF CACHE BOOL "" FORCE)
    set(GGML_CUDA OFF CACHE BOOL "" FORCE)

    add_subdirectory(vendor/llama.cpp EXCLUDE_FROM_ALL)

    add_library(mukei_llama_native SHARED
        shim/mukei_llama_native.cpp
        shim/mukei_llama_native.h)
    target_compile_definitions(mukei_llama_native PRIVATE MUKEI_LLAMA_NATIVE_BUILD=1)
    target_include_directories(mukei_llama_native
        PUBLIC "${CMAKE_CURRENT_SOURCE_DIR}/shim"
        PRIVATE "${CMAKE_CURRENT_SOURCE_DIR}/vendor/llama.cpp/include")
    target_link_libraries(mukei_llama_native PRIVATE llama)
    set_target_properties(mukei_llama_native PROPERTIES
        CXX_VISIBILITY_PRESET hidden
        VISIBILITY_INLINES_HIDDEN YES
        LIBRARY_OUTPUT_DIRECTORY "${MUKEI_LLAMA_PREBUILT_DIR}"
        RUNTIME_OUTPUT_DIRECTORY "${MUKEI_LLAMA_PREBUILT_DIR}")
    if(UNIX AND NOT APPLE)
        target_link_options(mukei_llama_native PRIVATE "-Wl,--exclude-libs,ALL")
    endif()
    if(CMAKE_CXX_COMPILER_ID MATCHES "GNU|Clang")
        target_compile_options(mukei_llama_native PRIVATE -Wall -Wextra -Wpedantic -Werror)
    endif()
endif()

add_executable(mukei_llama_native_smoke shim/smoke.cpp)
target_include_directories(mukei_llama_native_smoke PRIVATE "${CMAKE_CURRENT_SOURCE_DIR}/shim")
target_link_libraries(mukei_llama_native_smoke PRIVATE mukei_llama_native)
if(BUILD_TESTING)
    add_test(NAME mukei_llama_native_smoke COMMAND mukei_llama_native_smoke)
endif()
