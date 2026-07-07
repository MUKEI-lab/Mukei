//! Fuzzing harness for math expression validation.
//!
//! This fuzzer tests the math expression parser (meval crate integration)
//! to find edge cases that could cause panics, infinite loops, or unexpected
//! behavior in untrusted input handling.

#![no_main]

use libfuzzer_sys::fuzz_target;
use mukei_core::tools::math::validate_expression;

fuzz_target!(|data: &[u8]| {
    if let Ok(input) = std::str::from_utf8(data) {
        // Skip empty inputs
        if input.is_empty() {
            return;
        }
        
        // Test Mukei's production validation layer first. Only inputs
        // accepted by that layer should reach the third-party parser.
        if validate_expression(input).is_ok() {
            let _result = meval::eval_str(input);
        }
        
        // Additional checks can be added:
        // - Ensure no stack overflow on deeply nested expressions
        // - Ensure no infinite loops on pathological inputs
        // - Ensure consistent error messages for invalid input
    }
});
