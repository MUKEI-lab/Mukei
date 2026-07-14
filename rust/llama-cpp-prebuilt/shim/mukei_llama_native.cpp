#include "mukei_llama_native.h"

#include "llama.h"

#include <algorithm>
#include <climits>
#include <cstdint>
#include <cstring>
#include <mutex>
#include <new>
#include <string>
#include <utility>
#include <vector>

namespace {
constexpr uint32_t kAbiVersion = 1;
constexpr const char * kBuildId = "7c082bc417bbe53210a83df4ba5b49e18ce6193c";
std::once_flag g_backend_once;

void ensure_backend_initialized() {
    std::call_once(g_backend_once, [] { llama_backend_init(); });
}

int32_t bounded_thread_count(uint32_t requested) {
    if (requested == 0) {
        return 1;
    }
    return static_cast<int32_t>(std::min<uint32_t>(requested, static_cast<uint32_t>(INT32_MAX)));
}

struct ContextGuard {
    llama_context * value = nullptr;
    ~ContextGuard() {
        if (value != nullptr) {
            llama_free(value);
        }
    }
};

struct SamplerGuard {
    llama_sampler * value = nullptr;
    ~SamplerGuard() {
        if (value != nullptr) {
            llama_sampler_free(value);
        }
    }
};

bool is_cancelled(mukei_llama_cancel_callback callback, void * user_data) {
    return callback != nullptr && callback(user_data);
}

int32_t tokenize_prompt(
    const llama_vocab * vocab,
    const uint8_t * prompt,
    size_t prompt_len,
    std::vector<llama_token> & tokens) {
    if (prompt_len > static_cast<size_t>(INT32_MAX)) {
        return MUKEI_LLAMA_ERR_INVALID_ARGUMENT;
    }
    const char * text = reinterpret_cast<const char *>(prompt);
    int32_t required = llama_tokenize(
        vocab,
        text,
        static_cast<int32_t>(prompt_len),
        nullptr,
        0,
        true,
        true);
    if (required == INT32_MIN) {
        return MUKEI_LLAMA_ERR_TOKENIZE;
    }
    if (required < 0) {
        required = -required;
    }
    if (required <= 0) {
        return MUKEI_LLAMA_ERR_TOKENIZE;
    }
    tokens.resize(static_cast<size_t>(required));
    const int32_t actual = llama_tokenize(
        vocab,
        text,
        static_cast<int32_t>(prompt_len),
        tokens.data(),
        required,
        true,
        true);
    if (actual <= 0) {
        return MUKEI_LLAMA_ERR_TOKENIZE;
    }
    tokens.resize(static_cast<size_t>(actual));
    return MUKEI_LLAMA_OK;
}

int32_t emit_piece(
    const llama_vocab * vocab,
    llama_token token,
    mukei_llama_token_callback callback,
    void * user_data) {
    char small[256];
    int32_t length = llama_token_to_piece(vocab, token, small, sizeof(small), 0, false);
    if (length == 0) {
        return MUKEI_LLAMA_OK;
    }
    if (length > 0) {
        callback(reinterpret_cast<const uint8_t *>(small), static_cast<size_t>(length), user_data);
        return MUKEI_LLAMA_OK;
    }
    if (length == INT32_MIN) {
        return MUKEI_LLAMA_ERR_TOKEN_PIECE;
    }
    const int32_t required = -length;
    std::vector<char> buffer(static_cast<size_t>(required));
    length = llama_token_to_piece(vocab, token, buffer.data(), required, 0, false);
    if (length < 0) {
        return MUKEI_LLAMA_ERR_TOKEN_PIECE;
    }
    if (length > 0) {
        callback(
            reinterpret_cast<const uint8_t *>(buffer.data()),
            static_cast<size_t>(length),
            user_data);
    }
    return MUKEI_LLAMA_OK;
}
}  // namespace

struct mukei_llama_model {
    llama_model * model = nullptr;
    uint32_t n_ctx = 0;
    uint32_t n_threads = 1;
    std::mutex inference_mutex;
};

extern "C" uint32_t mukei_llama_abi_version(void) {
    return kAbiVersion;
}

extern "C" const char * mukei_llama_build_id(void) {
    return kBuildId;
}

extern "C" const char * mukei_llama_status_message(int32_t code) {
    switch (code) {
        case MUKEI_LLAMA_OK: return "native inference completed";
        case MUKEI_LLAMA_ERR_INVALID_ARGUMENT: return "native inference received an invalid argument";
        case MUKEI_LLAMA_ERR_MODEL_LOAD: return "native inference could not load the selected model";
        case MUKEI_LLAMA_ERR_CONTEXT_CREATE: return "native inference could not create a model context";
        case MUKEI_LLAMA_ERR_TOKENIZE: return "native inference could not tokenize the prompt";
        case MUKEI_LLAMA_ERR_CONTEXT_OVERFLOW: return "the prompt exceeds the configured context window";
        case MUKEI_LLAMA_ERR_DECODE: return "native inference failed while evaluating model tokens";
        case MUKEI_LLAMA_ERR_TOKEN_PIECE: return "native inference could not decode a generated token";
        case MUKEI_LLAMA_CANCELLED: return "native inference was cancelled";
        default: return "native inference failed internally";
    }
}

extern "C" int32_t mukei_llama_model_load(
    const char * path,
    uint32_t n_ctx,
    uint32_t n_threads,
    int32_t gpu_layers,
    mukei_llama_model ** out_model) {
    if (path == nullptr || path[0] == '\0' || n_ctx == 0 || out_model == nullptr) {
        return MUKEI_LLAMA_ERR_INVALID_ARGUMENT;
    }
    *out_model = nullptr;
    try {
        ensure_backend_initialized();
        llama_model_params model_params = llama_model_default_params();
        model_params.n_gpu_layers = llama_supports_gpu_offload() ? gpu_layers : 0;
        model_params.check_tensors = true;
        llama_model * loaded = llama_model_load_from_file(path, model_params);
        if (loaded == nullptr) {
            return MUKEI_LLAMA_ERR_MODEL_LOAD;
        }

        llama_context_params context_params = llama_context_default_params();
        context_params.n_ctx = n_ctx;
        context_params.n_batch = std::min<uint32_t>(n_ctx, 512);
        context_params.n_ubatch = std::min<uint32_t>(context_params.n_batch, 512);
        context_params.n_seq_max = 1;
        context_params.n_threads = bounded_thread_count(n_threads);
        context_params.n_threads_batch = bounded_thread_count(n_threads);
        ContextGuard probe{llama_init_from_model(loaded, context_params)};
        if (probe.value == nullptr) {
            llama_model_free(loaded);
            return MUKEI_LLAMA_ERR_CONTEXT_CREATE;
        }

        auto * handle = new (std::nothrow) mukei_llama_model;
        if (handle == nullptr) {
            llama_model_free(loaded);
            return MUKEI_LLAMA_ERR_INTERNAL;
        }
        handle->model = loaded;
        handle->n_ctx = n_ctx;
        handle->n_threads = n_threads;
        *out_model = handle;
        return MUKEI_LLAMA_OK;
    } catch (...) {
        return MUKEI_LLAMA_ERR_INTERNAL;
    }
}

extern "C" void mukei_llama_model_free(mukei_llama_model * model) {
    if (model == nullptr) {
        return;
    }
    try {
        std::lock_guard<std::mutex> lock(model->inference_mutex);
        if (model->model != nullptr) {
            llama_model_free(model->model);
            model->model = nullptr;
        }
    } catch (...) {
        // Destruction is best-effort and must never throw across the C ABI.
    }
    delete model;
}

extern "C" int32_t mukei_llama_generate(
    mukei_llama_model * model,
    const uint8_t * prompt,
    size_t prompt_len,
    uint32_t max_new_tokens,
    mukei_llama_token_callback token_callback,
    mukei_llama_cancel_callback cancel_callback,
    void * user_data,
    uint32_t * out_generated_tokens) {
    if (model == nullptr || model->model == nullptr || prompt == nullptr || prompt_len == 0
        || max_new_tokens == 0 || token_callback == nullptr || out_generated_tokens == nullptr) {
        return MUKEI_LLAMA_ERR_INVALID_ARGUMENT;
    }
    *out_generated_tokens = 0;
    try {
        std::lock_guard<std::mutex> lock(model->inference_mutex);
        if (is_cancelled(cancel_callback, user_data)) {
            return MUKEI_LLAMA_CANCELLED;
        }

        const llama_vocab * vocab = llama_model_get_vocab(model->model);
        if (vocab == nullptr) {
            return MUKEI_LLAMA_ERR_INTERNAL;
        }
        std::vector<llama_token> prompt_tokens;
        const int32_t tokenize_status = tokenize_prompt(vocab, prompt, prompt_len, prompt_tokens);
        if (tokenize_status != MUKEI_LLAMA_OK) {
            return tokenize_status;
        }
        if (prompt_tokens.size() >= static_cast<size_t>(model->n_ctx)) {
            return MUKEI_LLAMA_ERR_CONTEXT_OVERFLOW;
        }

        // n_batch is the logical batch capacity; n_ubatch bounds physical
        // micro-batches. Size the logical batch for the actual prompt so a
        // valid >512-token prompt is not rejected by a hidden batch ceiling.
        const uint32_t prompt_batch = static_cast<uint32_t>(prompt_tokens.size());
        const uint32_t baseline_batch = std::min<uint32_t>(model->n_ctx, 512);
        llama_context_params context_params = llama_context_default_params();
        context_params.n_ctx = model->n_ctx;
        context_params.n_batch = std::max(prompt_batch, baseline_batch);
        context_params.n_ubatch = std::min<uint32_t>(context_params.n_batch, 512);
        context_params.n_seq_max = 1;
        context_params.n_threads = bounded_thread_count(model->n_threads);
        context_params.n_threads_batch = bounded_thread_count(model->n_threads);
        ContextGuard context{llama_init_from_model(model->model, context_params)};
        if (context.value == nullptr) {
            return MUKEI_LLAMA_ERR_CONTEXT_CREATE;
        }

        if (llama_model_has_encoder(model->model)) {
            llama_batch prompt_batch = llama_batch_get_one(
                prompt_tokens.data(), static_cast<int32_t>(prompt_tokens.size()));
            if (llama_encode(context.value, prompt_batch) != 0) {
                return MUKEI_LLAMA_ERR_DECODE;
            }
            if (!llama_model_has_decoder(model->model)) {
                return MUKEI_LLAMA_ERR_DECODE;
            }
            llama_token decoder_start = llama_model_decoder_start_token(model->model);
            if (decoder_start < 0) {
                decoder_start = llama_vocab_bos(vocab);
            }
            llama_batch decoder_batch = llama_batch_get_one(&decoder_start, 1);
            if (llama_decode(context.value, decoder_batch) != 0) {
                return MUKEI_LLAMA_ERR_DECODE;
            }
        } else {
            llama_batch prompt_batch = llama_batch_get_one(
                prompt_tokens.data(), static_cast<int32_t>(prompt_tokens.size()));
            if (llama_decode(context.value, prompt_batch) != 0) {
                return MUKEI_LLAMA_ERR_DECODE;
            }
        }

        SamplerGuard sampler{llama_sampler_init_greedy()};
        if (sampler.value == nullptr) {
            return MUKEI_LLAMA_ERR_INTERNAL;
        }

        const uint32_t context_size = llama_n_ctx(context.value);
        const uint32_t prompt_token_count = static_cast<uint32_t>(prompt_tokens.size());
        const uint32_t context_remaining =
            context_size > prompt_token_count ? context_size - prompt_token_count : 0;
        const uint32_t token_budget = std::min(max_new_tokens, context_remaining);
        for (uint32_t generated = 0; generated < token_budget; ++generated) {
            if (is_cancelled(cancel_callback, user_data)) {
                *out_generated_tokens = generated;
                return MUKEI_LLAMA_CANCELLED;
            }
            llama_token token = llama_sampler_sample(sampler.value, context.value, -1);
            if (llama_vocab_is_eog(vocab, token)) {
                *out_generated_tokens = generated;
                return MUKEI_LLAMA_OK;
            }
            const int32_t piece_status = emit_piece(vocab, token, token_callback, user_data);
            if (piece_status != MUKEI_LLAMA_OK) {
                *out_generated_tokens = generated;
                return piece_status;
            }
            *out_generated_tokens = generated + 1;

            // llama_batch_get_one borrows this token pointer. Keep the sampled
            // token variable alive until llama_decode returns.
            llama_batch next_batch = llama_batch_get_one(&token, 1);
            if (llama_decode(context.value, next_batch) != 0) {
                return MUKEI_LLAMA_ERR_DECODE;
            }
        }
        return MUKEI_LLAMA_OK;
    } catch (...) {
        return MUKEI_LLAMA_ERR_INTERNAL;
    }
}
