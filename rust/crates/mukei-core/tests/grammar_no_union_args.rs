//! Architect review GH #39 — confirm the tool-calling grammar uses
//! **per-tool typed argument productions**, never a single union of
//! argument shapes.
//!
//! A `arguments ::= web_search_args | read_file_args | ...` production
//! would let the GBNF sampler accept a `web_search` call carrying a
//! `read_file` argument shape, which the Rust post-parse validator
//! (REQ-AGT-08 / PRD §8.2) would then have to reject — defeating the
//! whole point of the grammar gate. The v0.6 grammar fix split the
//! union into per-tool args; this test locks that fix.
//!
//! The test is structural (it reads the grammar source), not parser-
//! coupled, so it survives future GBNF parser refactors.

use std::fs;
use std::path::PathBuf;

fn grammar_path() -> PathBuf {
    // CARGO_MANIFEST_DIR is set per-package; the grammar lives at the
    // workspace root under `grammars/tool_calling.gbnf`. Walk two
    // levels up from `crates/mukei-core/`.
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop(); // crates/
    p.pop(); // workspace root (rust/)
    p.push("grammars");
    p.push("tool_calling.gbnf");
    p
}

#[test]
fn grammar_file_exists() {
    let p = grammar_path();
    assert!(p.exists(), "grammar missing at {p:?}");
}

#[test]
fn each_tool_has_its_own_typed_args_production() {
    let src = fs::read_to_string(grammar_path()).expect("read grammar");
    // Each call production names a *distinct* args production. The
    // exact identifiers are the canonical reference; they MUST appear
    // verbatim in the grammar.
    for name in [
        "web_search_args",
        "read_file_args",
        "hardware_args",
        "math_eval_args",
    ] {
        assert!(
            src.contains(&format!("{name} ::=")),
            "grammar is missing the per-tool args production `{name} ::=`"
        );
    }
}

#[test]
fn no_union_of_arg_shapes_in_arguments_rule() {
    // A regression of the v0.5 grammar would look like:
    //
    //   arguments ::= web_search_args | read_file_args | hardware_args
    //
    // The pipe-union of `*_args` shapes is the exact thing GH #39
    // forbids. We grep for any line containing `arguments` on the LHS
    // that also pipe-unions two or more `_args` identifiers.
    let src = fs::read_to_string(grammar_path()).expect("read grammar");
    for line in src.lines() {
        // Find production LHS exactly equal to "arguments".
        let Some(lhs_end) = line.find("::=") else {
            continue;
        };
        let lhs = line[..lhs_end].trim();
        if lhs != "arguments" {
            continue;
        }
        let rhs = &line[lhs_end + 3..];
        let union_arg_shapes = rhs.matches("_args").count();
        assert!(
            union_arg_shapes < 2,
            "GH #39 regression: `arguments` production unions {union_arg_shapes} arg shapes \
             (line: {line:?}). Use per-tool call productions instead."
        );
    }
}
