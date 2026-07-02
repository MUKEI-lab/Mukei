//! Fuzzing harness for math expression validation.
//!
//! This fuzzer tests the math expression parser (meval crate integration)
//! to find edge cases that could cause panics, infinite loops, or unexpected
//! behavior in untrusted input handling.

#![no_main]

use libfuzzer_sys::fuzz_target;
use mukei_core::tools::math::validate_math_expression;

fuzz_target!(|data: &[u8]| {
    if let Ok(input) = std::str::from_utf8(data) {
        // Skip empty inputs
        if input.is_empty() {
            return;
        }

        // Test the REAL production math expression validator from mukei-core
        // This validates character whitelist and allowed identifiers before
        // passing to meval for evaluation.
        let _validation_result = validate_math_expression(input);

        // Also test raw meval parsing directly for comparison
        // The meval crate should handle all inputs gracefully without panicking
        let _result = meval::eval_str(input);

        // Additional checks can be added:
        // - Ensure no stack overflow on deeply nested expressions
        // - Ensure no infinite loops on pathological inputs
        // - Ensure consistent error messages for invalid input
    }
});
