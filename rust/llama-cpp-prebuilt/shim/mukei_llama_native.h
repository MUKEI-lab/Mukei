#pragma once

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#if defined(_WIN32)
#  if defined(MUKEI_LLAMA_NATIVE_BUILD)
#    define MUKEI_LLAMA_NATIVE_API __declspec(dllexport)
#  else
#    define MUKEI_LLAMA_NATIVE_API __declspec(dllimport)
#  endif
#else
#  define MUKEI_LLAMA_NATIVE_API __attribute__((visibility("default")))
#endif

#ifdef __cplusplus
extern "C" {
#endif

enum mukei_llama_status_code {
    MUKEI_LLAMA_OK = 0,
    MUKEI_LLAMA_ERR_INVALID_ARGUMENT = 1,
    MUKEI_LLAMA_ERR_MODEL_LOAD = 2,
    MUKEI_LLAMA_ERR_CONTEXT_CREATE = 3,
    MUKEI_LLAMA_ERR_TOKENIZE = 4,
    MUKEI_LLAMA_ERR_CONTEXT_OVERFLOW = 5,
    MUKEI_LLAMA_ERR_DECODE = 6,
    MUKEI_LLAMA_ERR_TOKEN_PIECE = 7,
    MUKEI_LLAMA_CANCELLED = 8,
    MUKEI_LLAMA_ERR_INTERNAL = 9,
};

typedef struct mukei_llama_model mukei_llama_model;
typedef void (*mukei_llama_token_callback)(const uint8_t * data, size_t len, void * user_data);
typedef bool (*mukei_llama_cancel_callback)(void * user_data);

MUKEI_LLAMA_NATIVE_API uint32_t mukei_llama_abi_version(void);
MUKEI_LLAMA_NATIVE_API const char * mukei_llama_build_id(void);
MUKEI_LLAMA_NATIVE_API const char * mukei_llama_status_message(int32_t code);

MUKEI_LLAMA_NATIVE_API int32_t mukei_llama_model_load(
    const char * path,
    uint32_t n_ctx,
    uint32_t n_threads,
    int32_t gpu_layers,
    mukei_llama_model ** out_model);

MUKEI_LLAMA_NATIVE_API void mukei_llama_model_free(mukei_llama_model * model);

MUKEI_LLAMA_NATIVE_API int32_t mukei_llama_generate(
    mukei_llama_model * model,
    const uint8_t * prompt,
    size_t prompt_len,
    uint32_t max_new_tokens,
    mukei_llama_token_callback token_callback,
    mukei_llama_cancel_callback cancel_callback,
    void * user_data,
    uint32_t * out_generated_tokens);

#ifdef __cplusplus
}
#endif
