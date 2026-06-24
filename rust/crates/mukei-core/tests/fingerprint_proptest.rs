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
