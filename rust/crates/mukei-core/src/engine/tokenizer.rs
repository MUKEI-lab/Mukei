//! `mukei_core::engine::tokenizer` — token counter used by the budget
//! manager. Implementations pluggable via [`TokenCount`] trait so the
//! real LLaMA BPE can be swapped in by the bridge crate.

#[async_trait::async_trait]
pub trait TokenCount: Send + Sync {
    async fn count(&self, s: &str) -> usize;
}

/// Heuristic tokenizer used in tests / on hosts without the BPE model.
/// Counts Unicode word boundaries (`char.is_alphanumeric()`) and adds
/// 1 for the boundary, plus one per digit.
pub struct CharCountTokenizer;

#[async_trait::async_trait]
impl TokenCount for CharCountTokenizer {
    async fn count(&self, s: &str) -> usize {
        let mut count = 0usize;
        let mut in_word = false;
        for c in s.chars() {
            if c.is_alphanumeric() {
                if !in_word { count += 1; in_word = true; }
            } else {
                in_word = false;
            }
        }
        count.max(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn char_counter_basic() {
        let t = CharCountTokenizer;
        assert_eq!(t.count("hello world").await, 2);
        assert_eq!(t.count("").await, 1);
        assert_eq!(t.count("a b c").await, 3);
    }
}
