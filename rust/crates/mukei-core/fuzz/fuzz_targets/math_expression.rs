//! Fuzzing harness for math expression validation.
//!
//! This fuzzer tests the math expression parser (meval crate integration)
//! to find edge cases that could cause panics, infinite loops, or unexpected
//! behavior in untrusted input handling.

#![no_main]

use libfuzzer_sys::fuzz_target;
use mukei_core::types; // Assuming math validation is in types module

fuzz_target!(|data: &[u8]| {
    if let Ok(input) = std::str::from_utf8(data) {
        // Skip empty inputs
        if input.is_empty() {
            return;
        }
        
        // Test math expression validation
        // The meval crate should handle all inputs gracefully
        let _result = meval::eval_str(input);
        
        // If we have a custom validation layer, test it here
        // For example, if there's a whitelist of allowed functions:
        // let _validated = validate_math_expression(input);
        
        // Additional checks can be added:
        // - Ensure no stack overflow on deeply nested expressions
        // - Ensure no infinite loops on pathological inputs
        // - Ensure consistent error messages for invalid input
    }
});
