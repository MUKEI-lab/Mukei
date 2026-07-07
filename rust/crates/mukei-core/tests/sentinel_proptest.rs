//! Architect review GH #18 — property-based fuzz of `escape_untrusted`.
//!
//! `tools::sentinel::escape_untrusted` is the load-bearing prompt-injection
//! defence: every untrusted string entering an `<external_data>` block
//! is funnelled through it. A bypass here defeats every downstream layer.
//!
//! The properties below pin the invariants the unit tests assert
//! anecdotally:
//!
//! * **No forged close tag.** For ANY input, the output MUST NOT contain
//!   the substring `</external_data>` (case-insensitive).
//! * **No forged open tag.** For ANY input, the output MUST NOT contain
//!   `<external_data` (case-insensitive).
//! * **No forged trust assertion.** Escaping MUST prevent a hostile
//!   `trust="trusted"` literal from surviving into the rendered block.
//! * **Fast path remains borrowed.** Clean inputs with no escapable bytes
//!   must stay on the borrowed `Cow` path to avoid silent perf regressions.
//!
//! Test budget: 4096 cases per property, capped via the standard
//! `proptest!` configuration. Anything that takes longer is a real bug.

use mukei_core::tools::sentinel::escape_untrusted;
use proptest::prelude::*;

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 4096,
        max_shrink_iters: 8192,
        .. ProptestConfig::default()
    })]

    /// Property #1 — no input can forge a close-tag substring.
    #[test]
    fn no_forged_close_tag(s in ".{0,4096}") {
        let out = escape_untrusted(&s);
        let lower = out.to_ascii_lowercase();
        prop_assert!(
            !lower.contains("</external_data>"),
            "escape_untrusted produced a close-tag substring for input: {s:?}\noutput: {out:?}",
        );
    }

    /// Property #2 — no input can forge an open-tag substring.
    #[test]
    fn no_forged_open_tag(s in ".{0,4096}") {
        let out = escape_untrusted(&s);
        let lower = out.to_ascii_lowercase();
        prop_assert!(
            !lower.contains("<external_data"),
            "escape_untrusted produced an open-tag substring for input: {s:?}\noutput: {out:?}",
        );
    }

    /// Property #3 — nothing the escape produces lets a `trust="trusted"`
    /// substring slip through (the classic injection payload).
    #[test]
    fn no_forged_trust_assertion(s in ".{0,4096}") {
        let out = escape_untrusted(&s);
        let lower = out.to_ascii_lowercase();
        prop_assert!(
            !lower.contains(r#"trust="trusted""#),
            "escape_untrusted left a literal trust=\"trusted\" substring for input: {s:?}\noutput: {out:?}",
        );
    }

    /// Property #4 — if the input contained no escapable byte, the output
    /// is a borrowed `Cow` pointing at the same bytes.
    #[test]
    fn clean_input_is_a_borrow(s in "[A-Za-z0-9 .,!?_/-]{0,256}") {
        let out = escape_untrusted(&s);
        prop_assert!(
            matches!(out, std::borrow::Cow::Borrowed(_)),
            "escape_untrusted allocated on a clean input: {s:?}",
        );
        prop_assert_eq!(&*out, &*s);
    }
}
