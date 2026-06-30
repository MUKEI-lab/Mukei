# Vendored llama.cpp snapshot

- Upstream repository: <https://github.com/ggerganov/llama.cpp>
- Pinned upstream commit: `7c082bc417bbe53210a83df4ba5b49e18ce6193c`
- Vendored path: `rust/llama-cpp-prebuilt/vendor/llama.cpp`
- Vendoring model: checked-in source snapshot, not a Git submodule or gitlink.

## Included upstream content

The snapshot keeps the upstream layout for the library build:

- Root CMake/license metadata required by `add_subdirectory()`.
- `cmake/` build helper modules.
- `ggml/` core/backends, public headers, CMake files, and GGUF implementation.
- `include/` llama public headers.
- `src/` llama library sources and model implementations.
- `common/` utility sources, retained for compatibility with upstream CMake options even though the wrapper disables the common/tools/examples fallback by default.
- `licenses/` third-party license metadata needed by the retained sources.
- `vendor/cpp-httplib`, `vendor/miniaudio`, `vendor/nlohmann`, `vendor/sheredom`, and `vendor/stb` third-party dependencies present in the pinned upstream snapshot.

## Omitted upstream content

These paths are omitted because `rust/llama-cpp-prebuilt/CMakeLists.txt` builds only the llama library fallback and explicitly disables upstream apps, tools, examples, tests, and ggml tests/examples. They are not required to produce `libllama.a`.

| Path | Reason |
| --- | --- |
| `.clang-format`, `.clang-tidy`, `.editorconfig`, `.flake8`, `.pre-commit-config.yaml`, `.ecrc`, `.dockerignore`, `mypy.ini`, `pyrightconfig.json`, `ty.toml` | Upstream formatting/lint/development configuration; not used by the embedded library build. |
| `.devops/`, `.github/`, `.gemini/`, `.pi/`, `ci/` | Upstream CI, automation, and agent metadata; not used by Mukei's vendored build. |
| `.git/`, `.gitignore`, `.gitmodules` | Git repository/submodule metadata must not be vendored; this package is intentionally self-contained without gitlinks. |
| `AGENTS.md`, `AUTHORS`, `CLAUDE.md`, `CODEOWNERS`, `CONTRIBUTING.md`, `README.md`, `SECURITY.md` equivalents not needed by the build | Upstream repository governance or broad documentation; Mukei records vendoring details here instead. |
| `Makefile`, `build-xcframework.sh` | Standalone upstream build entry points not used by the CMake library fallback. |
| `app/`, `examples/`, `pocs/`, `tools/` | Standalone applications, demos, tools, and proof-of-concepts; wrapper disables these targets. |
| `benches/` | Upstream benchmarks; not part of the library target. |
| `tests/`, `ggml/tests/`, `ggml/examples/` | Upstream test/example targets; disabled for this embedded fallback. Binary test fixtures are always excluded with the tests. |
| `docs/`, `media/`, `models/` | Documentation, screenshots/media, templates, and sample model metadata; not required for `libllama.a`. |
| `conversion/`, `gguf-py/`, `convert_hf_to_gguf.py`, `convert_hf_to_gguf_update.py`, `convert_llama_ggml_to_gguf.py`, `convert_lora_to_gguf.py`, `requirements/`, `requirements.txt`, `pyproject.toml`, `scripts/` | Python conversion, packaging, and maintenance tooling; not invoked by the embedded library build. |
| `grammars/` | Runtime/demo grammar assets; not required to compile the library. |

No upstream `.gguf` model fixtures, audio/image/data fixtures, or other binary test blobs are vendored. Dropping those fixtures is safe because Mukei does not run upstream llama.cpp tests from this crate, and the wrapper disables upstream test targets.

## Future update procedure

1. Clone or fetch llama.cpp outside this repository, then check out the desired upstream commit.
2. Record the new commit hash at the top of this file.
3. Replace `vendor/llama.cpp` with a filtered source snapshot that preserves upstream paths for the included directories listed above.
4. Keep omitting Git metadata, upstream CI/development assets, standalone apps/tools/examples/benchmarks/tests, docs/media, conversion scripts, and binary fixtures unless the wrapper starts building a target that requires them.
5. Verify `git ls-files -s rust/llama-cpp-prebuilt/vendor/llama.cpp` shows normal `100644`/`100755` file modes and never a `160000` gitlink.
6. Do not add a `.gitmodules` entry for `rust/llama-cpp-prebuilt/vendor/llama.cpp`.
7. Commit the refreshed snapshot incrementally by top-level upstream directory and by each `vendor/` dependency, not as one monolithic commit.
8. Run the ZIP/offline/CMake/cargo checks documented in the PR before merging.
