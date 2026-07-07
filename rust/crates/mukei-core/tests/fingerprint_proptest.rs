//! Architect review GH #43 — property-based fuzz of
//! `FailureTracker::fingerprint`.
//!
//! Three invariants:
//!
//! 1. **Key-order invariance.** Any two JSON objects with the same
//!    `(key, value)` pairs but different key orderings hash to the
//!    same fingerprint. The agent loop relies on this to defeat the
//!    "shuffle keys to dodge abuse blocker" attack vector flagged in
//!    PRD §8.2.
//!
//! 2. **Stability.** Calling `fingerprint` twice on byte-identical
//!    inputs returns byte-identical outputs.
//!
//! 3. **Tool-name binding.** Two calls with the same arguments but
//!    different tool names hash differently (so the abuse blocker
//!    isolates per-tool failure streaks).

use mukei_core::agent::tools::FailureTracker;
use proptest::prelude::*;
use serde_json::{Map, Value};

/// Generate a small JSON object with stringly-typed keys and scalar
/// values. Sufficient to exercise key-order invariance.
fn arb_json_object() -> impl Strategy<Value = Value> {
    proptest::collection::vec(
        (
            "[a-z]{1,6}",
            prop_oneof![
                any::<i64>().prop_map(Value::from),
                any::<bool>().prop_map(Value::from),
                "[a-z]{0,8}".prop_map(Value::from),
            ],
        ),
        0..6,
    )
    .prop_map(|pairs| {
        let mut m = Map::new();
        for (k, v) in pairs {
            m.insert(k, v);
        }
        Value::Object(m)
    })
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 4096,
        .. ProptestConfig::default()
    })]

    /// Property #1 — key-order invariance. Shuffling the keys of an
    /// object must not change the fingerprint.
    #[test]
    fn fingerprint_is_key_order_invariant(obj in arb_json_object()) {
        let Value::Object(map) = obj.clone() else { unreachable!() };
        let mut reversed = Map::new();
        let mut pairs: Vec<_> = map.into_iter().collect();
        pairs.reverse();
        for (k, v) in pairs {
            reversed.insert(k, v);
        }
        let reversed_obj = Value::Object(reversed);

        let fp_a = FailureTracker::fingerprint("web_search", &obj);
        let fp_b = FailureTracker::fingerprint("web_search", &reversed_obj);
        prop_assert_eq!(fp_a, fp_b);
    }

    /// Property #2 — stability. Identical inputs hash identically.
    #[test]
    fn fingerprint_is_stable(obj in arb_json_object()) {
        let fp1 = FailureTracker::fingerprint("web_search", &obj);
        let fp2 = FailureTracker::fingerprint("web_search", &obj);
        prop_assert_eq!(fp1, fp2);
    }

    /// Property #3 — tool-name binding. Same args but different tool
    /// names must hash differently.
    #[test]
    fn fingerprint_binds_to_tool_name(obj in arb_json_object()) {
        let fp_a = FailureTracker::fingerprint("web_search", &obj);
        let fp_b = FailureTracker::fingerprint("read_file", &obj);
        prop_assert_ne!(fp_a, fp_b);
    }
}

// ---------------------------------------------------------------------
// Architect review GH #40 — grammar-shaped sanity tests.
//
// The GBNF grammar emits arguments in a *positional* order (e.g. the
// `web_search_args` production fixes `"query"` as the only key, and
// `read_file_args` fixes `"path"`). However, future grammar revisions
// or future multi-field tool calls might emit keys in arbitrary order.
// `FailureTracker::fingerprint` canonicalises keys alphabetically
// before hashing, so the agent loop's abuse blocker stays consistent
// regardless of emission order.
//
// The proptest above proves this for arbitrary objects. The two
// concrete tests below pin the canonical examples for the four tools
// in the v0.7.5 grammar so that a regression of the canonicalisation
// path is caught by hand-readable assertions, not just by a property
// failure shrink.
// ---------------------------------------------------------------------

#[test]
fn fingerprint_matches_grammar_emission_order_for_web_search() {
    // Both spellings produce the same fingerprint because
    // `web_search_args` only carries one field. This is the trivial
    // case but also the most-emitted one in production.
    let a = serde_json::json!({ "query": "latest news" });
    let b = serde_json::json!({ "query": "latest news" });
    assert_eq!(
        FailureTracker::fingerprint("web_search", &a),
        FailureTracker::fingerprint("web_search", &b),
    );
}

#[test]
fn fingerprint_is_alphabetical_key_canonical_for_multi_field_tools() {
    // Hypothetical future multi-field tool. The grammar might emit
    // `{ "a": 1, "b": 2 }` or `{ "b": 2, "a": 1 }` depending on
    // alternation order in the production. The fingerprint MUST be
    // identical because the abuse blocker treats keys as a set.
    let a = serde_json::json!({ "a": 1, "b": 2, "c": 3 });
    let b = serde_json::json!({ "c": 3, "a": 1, "b": 2 });
    let c = serde_json::json!({ "b": 2, "c": 3, "a": 1 });
    let fp_a = FailureTracker::fingerprint("future_tool", &a);
    let fp_b = FailureTracker::fingerprint("future_tool", &b);
    let fp_c = FailureTracker::fingerprint("future_tool", &c);
    assert_eq!(fp_a, fp_b);
    assert_eq!(fp_b, fp_c);
}

#[test]
fn fingerprint_distinguishes_grammar_tool_names() {
    // The four v0.7.5 grammar tools must each produce a distinct
    // fingerprint for the same argument shape — otherwise the abuse
    // blocker leaks failure streaks across tools.
    let args = serde_json::json!({ "x": 1 });
    let names = ["web_search", "read_file", "get_hardware_info", "math_eval"];
    let fps: Vec<_> = names
        .iter()
        .map(|n| FailureTracker::fingerprint(n, &args))
        .collect();
    for i in 0..fps.len() {
        for j in (i + 1)..fps.len() {
            assert_ne!(
                fps[i], fps[j],
                "fingerprint collision between {} and {}",
                names[i], names[j],
            );
        }
    }
}
