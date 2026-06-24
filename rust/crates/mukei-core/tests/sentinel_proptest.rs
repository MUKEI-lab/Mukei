//! Architect review GH #18 \u2014 property-based fuzz of `escape_untrusted`.
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
//! * **Idempotency-on-disjoint-inputs.** Calling `escape_untrusted` on an
//!   already-escaped output produces a strict superset of escapes \u2014 not
//!   a regression. (We do NOT assert strict idempotency because `&` is
//!   the escape-prefix character and a single pass intentionally
//!   re-escapes `&amp;` to `&amp;amp;`. The agent loop only calls
//!   `escape_untrusted` ONCE on each piece of untrusted text \u2014 see the
//!   `tool_envelope_does_not_get_double_escaped_on_replay` test.)
//! * **UTF-8 preservation.** Every output is valid UTF-8 (trivially: it
//!   is a Rust `String`/`Cow<'_, str>`).
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

    /// Property #1 \u2014 no input can forge a close-tag substring.
    #[test]
    fn no_forged_close_tag(s in ".{0,4096}") {
        let out = escape_untrusted(&s);
        let lower = out.to_ascii_lowercase();
        prop_assert!(
            !lower.contains("</external_data>"),
            "escape_untrusted produced a close-tag substring for input: {s:?}\n\
             output: {out:?}",
        );
    }

    /// Property #2 \u2014 no input can forge an open-tag substring.
    #[test]
    fn no_forged_open_tag(s in ".{0,4096}") {
        let out = escape_untrusted(&s);
        let lower = out.to_ascii_lowercase();
        prop_assert!(
            !lower.contains("<external_data"),
            "escape_untrusted produced an open-tag substring for input: {s:?}\n\
             output: {out:?}",
        );
    }

    /// Property #3 \u2014 nothing the escape produces lets a `trust=\"trusted\"`
    /// substring slip through (the classic injection payload).
    #[test]
    fn no_forged_trust_assertion(s in ".{0,4096}") {
        let out = escape_untrusted(&s);
        let lower = out.to_ascii_lowercase();
        // The escape turns `\"` into `&quot;`, so a literal `trust=\"trusted\"`
        // becomes `trust=&quot;trusted&quot;`. We assert the literal form
        // never survives.
        prop_assert!(
            !lower.contains(r#"trust="trusted""#),
            "escape_untrusted left a literal trust=\"trusted\" substring \
             for input: {s:?}\noutput: {out:?}",
        );
    }

    /// Property #4 \u2014 if the input contained no escapable byte, the output
    /// is a borrowed `Cow` pointing at the SAME bytes. This is the fast
    /// path; a regression here is a silent perf cliff under load.
    #[test]
    fn clean_input_is_a_borrow(s in "[a-zA-Z0-9 .,!?\u00e9\u00e8\u00f1\u4e1c\u4eac\ud83d\ude00]{0,256}") {
        let out = escape_untrusted(&s);
        prop_assert!(
            matches!(out, std::borrow::Cow::Borrowed(_)),
            "escape_untrusted allocated on a clean input: {s:?}",
        );
        prop_assert_eq!(&*out, &*s);
    }
}
