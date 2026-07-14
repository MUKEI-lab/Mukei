#include "mukei_llama_native.h"

#include <cstring>
#include <iostream>

int main() {
    if (mukei_llama_abi_version() != 1) {
        std::cerr << "unexpected native ABI version\n";
        return 1;
    }
    if (std::strcmp(mukei_llama_build_id(), "7c082bc417bbe53210a83df4ba5b49e18ce6193c") != 0) {
        std::cerr << "unexpected native build provenance\n";
        return 2;
    }
    if (mukei_llama_status_message(MUKEI_LLAMA_ERR_MODEL_LOAD) == nullptr) {
        std::cerr << "status message contract missing\n";
        return 3;
    }
    return 0;
}
