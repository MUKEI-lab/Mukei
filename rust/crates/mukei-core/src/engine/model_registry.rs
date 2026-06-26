//! `mukei_core::engine::model_registry` — TRD §8.1 / PRD REQ-MOD-01.
//!
//! Canonical catalogue of GGUF models Mukei knows how to download +
//! load on a tester's device. Today we ship **two** Gemma 4 variants:
//!
//!   * `gemma-4-e2b-it`  — 2B effective params, 3.46 GB Q4_K_M GGUF.
//!     Preferred on lower-end devices (Snapdragon 7xx / 4–6 GB RAM).
//!   * `gemma-4-e4b-it`  — 4B effective params, 5.41 GB Q4_K_M GGUF.
//!     Preferred on higher-end devices (Snapdragon 8 Gen 2+ / 8 GB+ RAM).
//!
//! # Why we model the catalogue here (not in `config.toml`)
//!
//! Three reasons:
//!
//!   1. The catalogue is *part of the binary contract* — QML's
//!      "Download model" picker must show the same names the engine
//!      knows how to load. A user-editable TOML drift would silently
//!      break model selection.
//!   2. The expected SHA-256 is a release-blocker integrity invariant
//!      (REQ-SEC-01). Hard-coding it next to the engine keeps the
//!      audit surface tight.
//!   3. The device-tier picker (`recommended_for_device`) needs RAM /
//!      ABI knowledge that lives in Rust. The TOML loader would
//!      duplicate it.
//!
//! # Download source policy
//!
//! The checked-in URLs below are pinned Hugging Face `resolve/main`
//! links for the exact GGUF artifacts we currently test against. The
//! accompanying SHA-256 values come from the Hub's linked-object hash,
//! and `LlamaEngine::load_model` re-verifies the full file before mmap.
//!
//! If release engineering later mirrors these files into a first-party
//! CDN, only the `download_url` field needs to change; the digest stays
//! the contract that keeps downloads honest.

use serde::{Deserialize, Serialize};

/// Stable identifier referenced from QML and `config.toml`.
///
/// Values are kebab-case, ASCII-only, and embed the upstream provider's
/// canonical model identifier. Never rename a variant — add a new one
/// and deprecate the old via a comment. The error code that surfaces
/// to QML when `from_id` rejects an unknown string is
/// `ERR_TOOL_ARGUMENT` with field `model_id`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ModelId {
    /// Gemma 4 E2B Instruct, Q4_K_M GGUF. Lower-tier default.
    Gemma4E2bIt,
    /// Gemma 4 E4B Instruct, Q4_K_M GGUF. Higher-tier default.
    Gemma4E4bIt,
}

impl ModelId {
    /// String form used by QML, `config.toml`, and the download UI.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Gemma4E2bIt => "gemma-4-e2b-it",
            Self::Gemma4E4bIt => "gemma-4-e4b-it",
        }
    }

    /// Parse a QML-supplied identifier back to a variant. Returns
    /// `None` for unknown strings so the bridge can map it into a
    /// stable `ERR_TOOL_ARGUMENT` error.
    ///
    /// Deprecated `gemma-3n-*` aliases are accepted for one migration
    /// window so an older QML build can still talk to a newer bridge.
    pub fn from_id(s: &str) -> Option<Self> {
        match s {
            "gemma-4-e2b-it" | "gemma-3n-e2b-it" => Some(Self::Gemma4E2bIt),
            "gemma-4-e4b-it" | "gemma-3n-e4b-it" => Some(Self::Gemma4E4bIt),
            _ => None,
        }
    }
}

impl std::fmt::Display for ModelId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Static descriptor for a downloadable model. All fields are owned
/// by the binary, never read from disk.
#[derive(Debug, Clone)]
pub struct ModelDescriptor {
    /// Canonical identifier (also the QML picker key).
    pub id: ModelId,
    /// Display name shown in the QML picker.
    pub display_name: &'static str,
    /// One-line UX description.
    pub description: &'static str,
    /// HTTPS URL of the GGUF artifact.
    pub download_url: &'static str,
    /// Hex-encoded SHA-256 of the GGUF file `download_url` serves.
    /// Verified by `LlamaEngine::load_model` BEFORE the file is mmapped
    /// (REQ-SEC-01).
    pub expected_sha256: &'static str,
    /// Approximate on-disk size in bytes. Surfaced by QML so the user
    /// can confirm before kicking a multi-gigabyte download on cellular.
    pub approximate_bytes: u64,
    /// Recommended minimum device RAM in MiB. Below this, the tier
    /// picker downgrades to the smaller variant.
    pub min_device_ram_mib: u32,
    /// Recommended n_ctx for this model on a mid-tier device. The
    /// `MukeiConfig.n_ctx` is the source of truth at runtime; this is
    /// only a hint surfaced to the device-tier picker.
    pub recommended_n_ctx: usize,
    /// Filename `download_url` resolves to inside the model directory.
    /// Used by the bridge to compute the final dest path.
    pub filename: &'static str,
}

/// Catalogue of known models. Order is significant: the lower-tier
/// variant comes first so `recommended_for_device` falls through to
/// it when RAM is unknown / very small.
pub const MODELS: &[ModelDescriptor] = &[
    ModelDescriptor {
        id: ModelId::Gemma4E2bIt,
        display_name: "Gemma 4 E2B Instruct (Q4_K_M)",
        description:
            "2B effective-parameter Gemma 4, instruction-tuned. Recommended for phones with \
             4-6 GB RAM (Snapdragon 7xx-class, mid-tier MediaTek).",
        download_url:
            "https://huggingface.co/bartowski/google_gemma-4-E2B-it-GGUF/resolve/main/google_gemma-4-E2B-it-Q4_K_M.gguf?download=true",
        expected_sha256: "b5310340b3a23d31655d7119d100d5df1b2d8ee17b3ca8b0a23ad7e9eb5fa705",
        approximate_bytes: 3_462_678_272,
        min_device_ram_mib: 4096,
        recommended_n_ctx: 4096,
        filename: "google_gemma-4-E2B-it-Q4_K_M.gguf",
    },
    ModelDescriptor {
        id: ModelId::Gemma4E4bIt,
        display_name: "Gemma 4 E4B Instruct (Q4_K_M)",
        description:
            "4B effective-parameter Gemma 4, instruction-tuned. Recommended for phones with \
             8+ GB RAM (Snapdragon 8 Gen 2+, flagship Tensor / Dimensity).",
        download_url:
            "https://huggingface.co/bartowski/google_gemma-4-E4B-it-GGUF/resolve/main/google_gemma-4-E4B-it-Q4_K_M.gguf?download=true",
        expected_sha256: "51865750adafd22de56994a343d5a887cc1a589b9bae41d62b748c8bd0ca9c76",
        approximate_bytes: 5_405_168_384,
        min_device_ram_mib: 7168,
        recommended_n_ctx: 8192,
        filename: "google_gemma-4-E4B-it-Q4_K_M.gguf",
    },
];

/// Look up a model descriptor by its canonical identifier.
pub fn lookup(id: ModelId) -> &'static ModelDescriptor {
    MODELS
        .iter()
        .find(|m| m.id == id)
        .expect("every ModelId variant has a MODELS entry — keep the table exhaustive")
}

/// Look up a model descriptor by its QML-string identifier. Returns
/// `None` for unknown strings so the bridge crate can map it into a
/// typed `ToolArgumentInvalid` error (field `model_id`).
pub fn lookup_str(id: &str) -> Option<&'static ModelDescriptor> {
    ModelId::from_id(id).map(lookup)
}

/// Pick the recommended model for a device with `total_ram_mib`.
///
/// Policy:
///
///   * `≥ 7168 MiB` (≈ 8 GB +) → `Gemma4E4bIt`.
///   * Below that → `Gemma4E2bIt`.
///
/// The thresholds intentionally use the same numbers as the
/// `min_device_ram_mib` field on the descriptors so a future tier
/// change only has to touch the MODELS table.
pub fn recommended_for_device(total_ram_mib: u32) -> &'static ModelDescriptor {
    let e4b = lookup(ModelId::Gemma4E4bIt);
    if total_ram_mib >= e4b.min_device_ram_mib {
        e4b
    } else {
        lookup(ModelId::Gemma4E2bIt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalogue_has_both_gemma_variants() {
        assert!(MODELS.iter().any(|m| m.id == ModelId::Gemma4E2bIt));
        assert!(MODELS.iter().any(|m| m.id == ModelId::Gemma4E4bIt));
        assert_eq!(MODELS.len(), 2);
    }

    #[test]
    fn lookup_returns_matching_descriptor() {
        assert_eq!(lookup(ModelId::Gemma4E2bIt).id, ModelId::Gemma4E2bIt);
        assert_eq!(lookup(ModelId::Gemma4E4bIt).id, ModelId::Gemma4E4bIt);
    }

    #[test]
    fn id_round_trips_through_string() {
        for m in MODELS {
            let s = m.id.as_str();
            assert_eq!(ModelId::from_id(s), Some(m.id));
        }
    }

    #[test]
    fn deprecated_gemma_3n_aliases_still_resolve() {
        assert_eq!(
            ModelId::from_id("gemma-3n-e2b-it"),
            Some(ModelId::Gemma4E2bIt)
        );
        assert_eq!(
            ModelId::from_id("gemma-3n-e4b-it"),
            Some(ModelId::Gemma4E4bIt)
        );
    }

    #[test]
    fn unknown_id_returns_none() {
        assert!(ModelId::from_id("gpt-5").is_none());
        assert!(ModelId::from_id("").is_none());
        assert!(lookup_str("not-a-real-model").is_none());
    }

    #[test]
    fn manifest_hashes_are_full_sha256_hex() {
        for m in MODELS {
            assert_eq!(m.expected_sha256.len(), 64);
            assert!(m.expected_sha256.chars().all(|c| c.is_ascii_hexdigit()));
        }
    }

    #[test]
    fn manifest_urls_are_https_resolve_links() {
        for m in MODELS {
            assert!(m.download_url.starts_with("https://huggingface.co/"));
            assert!(m.download_url.contains("/resolve/main/"));
            assert!(m.download_url.ends_with(".gguf?download=true"));
        }
    }

    #[test]
    fn descriptor_filenames_match_urls() {
        for m in MODELS {
            assert!(m.download_url.contains(m.filename));
        }
    }

    #[test]
    fn device_tier_picks_e4b_on_high_ram() {
        // 8 GB phone.
        assert_eq!(recommended_for_device(8192).id, ModelId::Gemma4E4bIt);
        // 12 GB phone.
        assert_eq!(recommended_for_device(12288).id, ModelId::Gemma4E4bIt);
    }

    #[test]
    fn device_tier_picks_e2b_on_lower_ram() {
        // 4 GB phone.
        assert_eq!(recommended_for_device(4096).id, ModelId::Gemma4E2bIt);
        // 6 GB phone.
        assert_eq!(recommended_for_device(6144).id, ModelId::Gemma4E2bIt);
    }

    #[test]
    fn descriptor_threshold_matches_picker_policy() {
        let e4b = lookup(ModelId::Gemma4E4bIt);
        assert_eq!(e4b.min_device_ram_mib, 7168);
        assert_eq!(
            recommended_for_device(e4b.min_device_ram_mib).id,
            ModelId::Gemma4E4bIt
        );
        assert_eq!(
            recommended_for_device(e4b.min_device_ram_mib.saturating_sub(1)).id,
            ModelId::Gemma4E2bIt
        );
    }
}
