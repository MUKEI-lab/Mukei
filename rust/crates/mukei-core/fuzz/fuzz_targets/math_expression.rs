//! Fuzzing harness for math expression validation.
//!
//! The production evaluator performs parsing in-process after this validation
//! layer. This target keeps untrusted-input validation panic-free and bounded;
//! parser behavior is covered by the core crate regression tests.

#![no_main]

use libfuzzer_sys::fuzz_target;
use mukei_core::tools::math::validate_expression;

fuzz_target!(|data: &[u8]| {
    if let Ok(input) = std::str::from_utf8(data) {
        if input.is_empty() {
            return;
        }

        // Validation must remain total for arbitrary UTF-8 input: accepted
        // expressions continue to the production parser in MathTool, while
        // rejected expressions fail closed without panicking.
        let _ = validate_expression(input);
    }
});
