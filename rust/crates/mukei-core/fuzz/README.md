# Fuzzing Guide for mukei-core

This directory contains fuzzing harnesses for testing the security and robustness of mukei-core against untrusted inputs.

## Prerequisites

Install cargo-fuzz:

```bash
cargo install cargo-fuzz
```

Ensure you have a C++ compiler installed (required by libfuzzer-sys):
- Linux: `apt-get install build-essential` or `yum groupinstall "Development Tools"`
- macOS: Xcode Command Line Tools (`xcode-select --install`)
- Windows: MSVC Build Tools

## Available Fuzz Targets

### 1. Math Expression Fuzzer
Tests the math expression parser for edge cases, panics, and denial-of-service vulnerabilities.

```bash
cd fuzz
cargo fuzz run math_expression_fuzzer
```

### 2. Query Input Fuzzer
Tests query processing pipelines for injection vulnerabilities, panics, and edge cases.

```bash
cd fuzz
cargo fuzz run query_input_fuzzer
```

## Running Fuzzers

### Basic Usage

Run a fuzzer indefinitely:
```bash
cargo fuzz run <target_name>
```

Run with a specific timeout (in seconds):
```bash
cargo fuzz run <target_name> --timeout=60
```

Run with multiple jobs (parallel fuzzing):
```bash
cargo fuzz run <target_name> -j4
```

### Reproducing Crashes

If the fuzzer finds a crash, it will be saved to `fuzz/artifacts/<target>/`. To reproduce:

```bash
cargo fuzz run <target_name> fuzz/artifacts/<target>/<crash_file>
```

### Minimizing Test Cases

To minimize a crashing input:
```bash
cargo fuzz tmin <target_name> fuzz/artifacts/<target>/<crash_file>
```

## Continuous Fuzzing

For continuous fuzzing in CI/CD:

1. **GitHub Actions**: Add a workflow that runs fuzzers periodically
2. **OSS-Fuzz**: Consider integrating with Google's OSS-Fuzz for continuous coverage
3. **Local Automation**: Run fuzzers overnight or during idle time

Example cron job for nightly fuzzing:
```bash
0 2 * * * cd /path/to/mukei-core/fuzz && cargo fuzz run math_expression_fuzzer -- -max_total_time=3600 >> /var/log/fuzz.log 2>&1
```

## Adding New Fuzz Targets

1. Create a new file in `fuzz_targets/`:
```rust
#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Your fuzzing logic here
});
```

2. Add a new `[[bin]]` entry in `fuzz/Cargo.toml`

3. Run with: `cargo fuzz run <new_target>`

## Best Practices

- **Focus on boundaries**: Test FFI boundaries, serialization/deserialization, and external API interfaces
- **Validate assumptions**: Ensure all untrusted input is validated before use
- **Check for panics**: Use `catch_unwind` where appropriate
- **Monitor coverage**: Use coverage-guided fuzzing for better path exploration
- **Document findings**: Keep track of discovered issues and fixes

## Reporting Security Issues

If you discover a security vulnerability through fuzzing, please report it responsibly following the project's security policy.

## Additional Resources

- [cargo-fuzz documentation](https://github.com/rust-fuzz/cargo-fuzz)
- [libFuzzer documentation](https://llvm.org/docs/LibFuzzer.html)
- [Rust Fuzz Book](https://rust-fuzz.github.io/book/)
