//! Source trust classification — migration §9.
//!
//! # Invariants
//!
//! - The classifier is **closed**: every URL falls into exactly one of
//!   the four [`SourceTrust`] variants. `Unsafe` rejects the hit
//!   BEFORE ranking.
//! - The trusted / semi-trusted / unsafe domain lists are intentionally
//!   small and hand-maintained. Edits go through review.

use serde::{Deserialize, Serialize};

/// Trust level of a single source URL.
#[derive(Copy, Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum SourceTrust {
    /// Reputable wire / journal — surfaced with full confidence.
    Trusted,
    /// Useful but crowd-sourced; surfaced with a caveat (Wikipedia, etc.).
    SemiTrusted,
    /// Unknown blog / forum. May still be cited but ranker down-weights.
    Untrusted,
    /// Known prompt-injection / SEO-spam domain. Hits are dropped.
    Unsafe,
}

impl SourceTrust {
    /// Stable tag for the FFI / cache key.
    pub fn as_tag(self) -> &'static str {
        match self {
            Self::Trusted => "trusted",
            Self::SemiTrusted => "semi_trusted",
            Self::Untrusted => "untrusted",
            Self::Unsafe => "unsafe",
        }
    }
}

/// Hand-maintained classifier. The lists are deliberately short so a
/// reviewer can audit them at a glance. Adding a domain to `UNSAFE`
/// drops every hit from it — handle with care.
pub struct TrustClassifier {
    trusted: Vec<&'static str>,
    semi_trusted: Vec<&'static str>,
    unsafe_domains: Vec<&'static str>,
}

impl Default for TrustClassifier {
    fn default() -> Self {
        Self {
            trusted: vec![
                "reuters.com",
                "apnews.com",
                "bbc.com",
                "bbc.co.uk",
                "nytimes.com",
                "ft.com",
                "wsj.com",
                "bloomberg.com",
                "nature.com",
                "science.org",
                "ieee.org",
                "acm.org",
                "arxiv.org",
                "rust-lang.org",
                "doc.rust-lang.org",
            ],
            semi_trusted: vec![
                "wikipedia.org",
                "en.wikipedia.org",
                "stackoverflow.com",
                "stackexchange.com",
                "github.com",
                "developer.mozilla.org",
                "github.io",
                "kernel.org",
            ],
            unsafe_domains: vec![
                // Known prompt-injection / SEO-spam sentinels. Extend
                // as the security team confirms new ones.
                "promptinject.example",
                "seo-spam.example",
            ],
        }
    }
}

impl TrustClassifier {
    /// Construct the default classifier.
    pub fn new() -> Self {
        Self::default()
    }

    /// Classify a URL.
    ///
    /// Uses a stdlib-only host parser (no `url` crate dependency) so
    /// this module compiles in the sandbox feature set. The parser
    /// accepts only `scheme://host[/...]` form; anything else falls
    /// through to `Untrusted`.
    pub fn classify(&self, raw_url: &str) -> SourceTrust {
        let host = match extract_host(raw_url) {
            Some(h) => h,
            None => return SourceTrust::Untrusted,
        };
        if host.is_empty() {
            return SourceTrust::Untrusted;
        }
        for d in &self.unsafe_domains {
            if host == *d || host.ends_with(&format!(".{d}")) {
                return SourceTrust::Unsafe;
            }
        }
        for d in &self.trusted {
            if host == *d || host.ends_with(&format!(".{d}")) {
                return SourceTrust::Trusted;
            }
        }
        for d in &self.semi_trusted {
            if host == *d || host.ends_with(&format!(".{d}")) {
                return SourceTrust::SemiTrusted;
            }
        }
        SourceTrust::Untrusted
    }
}

/// Minimal `scheme://host` parser. Returns the lowercase host on
/// success, `None` on malformed input.
fn extract_host(raw_url: &str) -> Option<String> {
    let idx = raw_url.find("://")?;
    let after_scheme = &raw_url[idx + 3..];
    if after_scheme.is_empty() {
        return None;
    }
    let host_end = after_scheme
        .find(['/', '?', '#', ':'])
        .unwrap_or(after_scheme.len());
    let host = &after_scheme[..host_end];
    if host.is_empty() {
        return None;
    }
    Some(host.to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trusted_domain_passes() {
        let c = TrustClassifier::new();
        assert_eq!(
            c.classify("https://www.reuters.com/article"),
            SourceTrust::Trusted
        );
        assert_eq!(
            c.classify("https://arxiv.org/abs/2401.00001"),
            SourceTrust::Trusted
        );
    }

    #[test]
    fn semi_trusted_wikipedia() {
        let c = TrustClassifier::new();
        assert_eq!(
            c.classify("https://en.wikipedia.org/wiki/Rust"),
            SourceTrust::SemiTrusted
        );
    }

    #[test]
    fn unknown_falls_to_untrusted() {
        let c = TrustClassifier::new();
        assert_eq!(
            c.classify("https://some-random-blog.example/post"),
            SourceTrust::Untrusted
        );
    }

    #[test]
    fn unsafe_domain_is_blocked() {
        let c = TrustClassifier::new();
        assert_eq!(
            c.classify("https://promptinject.example/payload"),
            SourceTrust::Unsafe
        );
    }

    #[test]
    fn malformed_url_falls_to_untrusted() {
        let c = TrustClassifier::new();
        assert_eq!(c.classify("not a url"), SourceTrust::Untrusted);
    }
}
