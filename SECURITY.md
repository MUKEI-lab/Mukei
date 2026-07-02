# Security Hardening Guide for Mukei

This document outlines the security measures and best practices implemented in the Mukei project.

## Overview

Mukei implements multiple layers of security hardening to protect against common vulnerabilities and attack vectors. This guide covers:

1. FFI Boundary Safety
2. Input Validation & Fuzzing
3. Dependency Security
4. Compiler-Level Protections
5. Continuous Security Monitoring

## 1. FFI Boundary Safety

### CallbackGuard Pattern

All FFI callbacks are protected by the `CallbackGuard` mechanism which prevents use-after-free vulnerabilities:

```rust
use mukei_core::guard::{callback_with_guard, CallbackGuard, Inner};

// Every FFI callback must use this pattern
let inner = Inner::new();
let ptr = Arc::into_raw(Arc::clone(&inner));
let snapshot = inner.generation.load(Ordering::Acquire);

let result: Result<i32, GuardError> = callback_with_guard!(ptr, snapshot, {
    // Your callback logic here
    Ok::<_, GuardError>(42)
});
```

**Key Features:**
- Generation counter prevents stale callback execution
- `catch_unwind` wraps all FFI boundary crossings
- ABA mitigation via process-unique instance IDs
- Monotonic generation ensures forward progress only

### Testing FFI Safety

Run the FFI boundary integration tests:

```bash
cd rust
cargo test -p mukei-core --test ffi_boundaries
```

## 2. Input Validation & Fuzzing

### Fuzzing Harnesses

The project includes fuzzing targets for critical input validation paths:

**Location:** `rust/crates/mukei-core/fuzz/`

**Available Targets:**
- `math_expression_fuzzer` - Tests math expression parser
- `query_input_fuzzer` - Tests query processing pipeline

**Running Fuzzers:**

```bash
# Install prerequisites
cargo install cargo-fuzz

# Run a fuzzer
cd rust/crates/mukei-core/fuzz
cargo fuzz run math_expression_fuzzer

# Run with timeout
cargo fuzz run query_input_fuzzer --timeout=60

# Run multiple parallel jobs
cargo fuzz run math_expression_fuzzer -j4
```

### Best Practices for Input Validation

1. **Never trust external input** - All user-provided data must be validated
2. **Use whitelists over blacklists** - Define what's allowed, not what's forbidden
3. **Fail safely** - Invalid input should produce clear errors, not crashes
4. **Sanitize before use** - Escape special characters in SQL, shell, etc.

## 3. Dependency Security

### Cargo Audit

Regular dependency vulnerability scanning is automated via GitHub Actions:

```bash
# Install cargo-audit
cargo install cargo-audit

# Run audit
cd rust
cargo audit
```

**CI Integration:** The `security-audit.yml` workflow runs:
- On every PR and push to main
- Weekly scheduled scans
- Automatic artifact upload for reports

### Cargo Deny

Supply-chain security is enforced via `cargo-deny`:

```bash
# Install cargo-deny
cargo install cargo-deny

# Run all checks
cd rust
cargo deny check advisories sources licenses bans
```

**Policy Highlights:**
- Only permissive licenses allowed (MIT, Apache-2.0, BSD, etc.)
- Copyleft licenses (MPL-2.0, GPL) are denied
- Known vulnerable crates are blocked
- Wildcard dependencies are denied (except intra-workspace paths)

## 4. Compiler-Level Protections

### Release Hardening Profile

A special profile with enhanced security flags:

```toml
[profile.release-hardening]
inherits      = "release"
lto           = "fat"
codegen-units = 1
panic         = "unwind"
strip         = "symbols"
rustflags     = [
    "-C", "target-feature=+stack-protector-all",
    "-C", "link-arg=-Wl,-z,relro,-z,now"
]
```

**Build with hardening:**

```bash
cd rust
cargo build --profile release-hardening --features release-hardening
```

**Security Features:**
- **Stack Protector**: Detects stack buffer overflows
- **Full RELRO**: Makes GOT read-only after relocation
- **LTO**: Enables whole-program optimization and analysis
- **Symbol Stripping**: Reduces attack surface by removing debug symbols

### Clippy Security Lints

CI enforces additional security-focused lints:

```bash
cargo clippy -- -D clippy::unwrap_in_result \
               -D clippy::expect_used \
               -D clippy::arithmetic_side_effects \
               -D clippy::panic_in_result_fn
```

**Enforced Rules:**
- No `unwrap()` or `expect()` in production code
- No arithmetic side effects (overflow checking)
- No panics in functions returning `Result`
- Warnings on `todo!()` and `unimplemented!()`

## 5. Continuous Security Monitoring

### GitHub Actions Workflows

**lint.yml** - Code quality and security lints
- Runs on every PR
- Enforces rustfmt, clippy, and security lints
- Tests multiple feature combinations

**security-audit.yml** - Vulnerability scanning
- Runs on every PR and weekly
- Executes `cargo audit` for dependency vulnerabilities
- Runs fuzz regression tests
- Generates and uploads audit reports

### Recommended Schedule

| Task | Frequency | Automation |
|------|-----------|------------|
| Cargo Audit | Weekly + PR | GitHub Actions |
| Fuzzing | Continuous | Local/CI |
| Dependency Review | Per PR | Manual + CI |
| Security Lint | Every PR | GitHub Actions |
| Full Penetration Test | Quarterly | Manual |

## 6. Property-Based Testing

### Proptest Integration

The project uses `proptest` for property-based testing of edge cases:

```rust
// Example from existing tests
use proptest::prelude::*;

proptest! {
    #[test]
    fn test_property(input in any::<String>()) {
        // Test invariant holds for all inputs
        prop_assert!(some_invariant(&input));
    }
}
```

**Existing Proptest Suites:**
- `fingerprint_proptest.rs` - Fingerprint canonicalization
- `migrator_proptest.rs` - Database migration ordering
- `sentinel_proptest.rs` - Sentinel escape sequences

**Adding New Property Tests:**

1. Add test file in `crates/mukei-core/tests/`
2. Use `proptest!` macro to define properties
3. Ensure properties are deterministic and fast
4. Run with: `cargo test --test <name>_proptest`

## 7. Incident Response

### If a Vulnerability is Found

1. **Do not disclose publicly** until fixed
2. **Create a private security advisory** on GitHub
3. **Assess impact** using the following criteria:
   - Is user data at risk?
   - Can the vulnerability be exploited remotely?
   - Does it require authentication?
4. **Develop and test a fix**
5. **Release a patch** with appropriate disclosure

### Reporting Security Issues

For external security researchers:
- Use GitHub's private vulnerability reporting feature
- Or email: security@mukei.example.com (replace with actual)
- Include: affected version, reproduction steps, impact assessment

## 8. Checklist for Contributors

Before submitting a PR:

- [ ] No new `unwrap()` or `expect()` calls in production code
- [ ] All FFI boundaries use `CallbackGuard` and `catch_unwind`
- [ ] Input validation added for new user-facing APIs
- [ ] Dependencies reviewed for license compatibility
- [ ] Security-sensitive code has unit tests
- [ ] Proptest or fuzzing considered for complex parsing logic
- [ ] No TODOs or unimplemented!() in security-critical paths

## Resources

- [Rust Secure Code Working Group](https://github.com/rust-secure-code/)
- [Cargo Audit Documentation](https://docs.rs/cargo-audit/)
- [Cargo Fuzz Book](https://rust-fuzz.github.io/book/)
- [OWASP Rust Top 10](https://owasp.org/www-project-top-10-rust/)
- [RustSec Advisory Database](https://rustsec.org/)
