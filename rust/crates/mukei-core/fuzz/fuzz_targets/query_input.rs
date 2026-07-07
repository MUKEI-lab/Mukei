//! Fuzzing harness for query input validation.
//!
//! This fuzzer tests the query processing pipeline to find edge cases
//! that could cause panics, injection vulnerabilities, or unexpected
//! behavior when processing untrusted user queries.

#![no_main]

use libfuzzer_sys::fuzz_target;
use mukei_core::tools::sentinel::escape_untrusted;
use mukei_core::tools::web_search::validate_query_input;

fuzz_target!(|data: &[u8]| {
    if let Ok(query) = std::str::from_utf8(data) {
        // Skip empty inputs
        if query.is_empty() {
            return;
        }
        
        // Test production query escaping and validation. These calls
        // should handle all inputs gracefully without panicking.
        
        // Test JSON parsing if query looks like JSON
        if query.trim().starts_with('{') || query.trim().starts_with('[') {
            let _parsed: Result<serde_json::Value, _> = serde_json::from_str(query);
        }
        
        let escaped = escape_untrusted(query);
        let _ = escaped.as_ref();

        let _ = validate_query_input(query);
        
        // Additional checks:
        // - Ensure no SQL injection patterns bypass sanitization
        // - Ensure no command injection in shell operations
        // - Ensure consistent handling of unicode edge cases
        // - Ensure no ReDoS (Regular Expression Denial of Service)
    }
});
