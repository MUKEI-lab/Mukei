//! Fuzzing harness for query input validation.
//!
//! This fuzzer tests the query processing pipeline to find edge cases
//! that could cause panics, injection vulnerabilities, or unexpected
//! behavior when processing untrusted user queries.

#![no_main]

use libfuzzer_sys::fuzz_target;
use mukei_core::tools::sentinel::escape_untrusted;

fuzz_target!(|data: &[u8]| {
    if let Ok(query) = std::str::from_utf8(data) {
        // Skip empty inputs
        if query.is_empty() {
            return;
        }

        // Test query sanitization using the REAL production sentinel escaper
        // This is the same function used by web_search.rs to neutralize
        // prompt-injection attacks in <external_data> blocks (REQ-SEC-04).
        let _sanitized = escape_untrusted(query);

        // Test URL parsing if query contains URLs
        if query.starts_with("http://") || query.starts_with("https://") {
            let _parsed = url::Url::parse(query);
        }

        // Test JSON parsing if query looks like JSON
        if query.trim().starts_with('{') || query.trim().starts_with('[') {
            let _parsed: Result<serde_json::Value, _> = serde_json::from_str(query);
        }

        // Additional checks:
        // - Ensure no SQL injection patterns bypass sanitization
        // - Ensure no command injection in shell operations
        // - Ensure consistent handling of unicode edge cases
        // - Ensure no ReDoS (Regular Expression Denial of Service)
    }
});
