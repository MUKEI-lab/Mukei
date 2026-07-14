//! Pinned embedding-artifact provenance and fail-closed verification.
//!
//! Runtime code must never construct the production Candle embedder from an
//! unpinned or partially verified directory. This module owns the immutable
//! repository revision, byte sizes, and SHA-256 digests for the supported
//! MiniLM bundle and verifies every file before the bridge may load it.

use std::fs::{self, File};
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::error::{MukeiError, Result};

/// One required file in a pinned embedding-model bundle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EmbeddingArtifactSpec {
    /// Simple filename inside the model directory. Separators are forbidden.
    pub filename: &'static str,
    /// Exact expected byte length.
    pub size_bytes: u64,
    /// Lowercase SHA-256 hex digest over the complete file.
    pub sha256: &'static str,
}

/// Immutable provenance contract for one embedding-model bundle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EmbeddingArtifactManifest {
    /// Upstream model repository identity.
    pub repository: &'static str,
    /// Immutable upstream revision.
    pub revision: &'static str,
    /// Expected embedding dimension.
    pub embedding_dim: u32,
    /// Required files and their complete digests.
    pub files: &'static [EmbeddingArtifactSpec],
}

/// Proof that every required artifact was verified under one canonical root.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedEmbeddingArtifacts {
    model_dir: PathBuf,
    repository: &'static str,
    revision: &'static str,
    embedding_dim: u32,
    embedder_id: String,
}

impl VerifiedEmbeddingArtifacts {
    /// Canonical model directory whose direct children were verified.
    pub fn model_dir(&self) -> &Path {
        &self.model_dir
    }

    /// Immutable upstream repository identity.
    pub fn repository(&self) -> &'static str {
        self.repository
    }

    /// Immutable upstream revision.
    pub fn revision(&self) -> &'static str {
        self.revision
    }

    /// Expected embedding dimension.
    pub fn embedding_dim(&self) -> u32 {
        self.embedding_dim
    }

    /// Stable vector-space identity derived from the pinned weights digest.
    pub fn embedder_id(&self) -> &str {
        &self.embedder_id
    }
}

/// Exact file manifest captured from the immutable upstream revision.
pub static ALL_MINILM_L6_V2_FILES: [EmbeddingArtifactSpec; 3] = [
    EmbeddingArtifactSpec {
        filename: "config.json",
        size_bytes: 612,
        sha256: "953f9c0d463486b10a6871cc2fd59f223b2c70184f49815e7efbcab5d8908b41",
    },
    EmbeddingArtifactSpec {
        filename: "tokenizer.json",
        size_bytes: 466_247,
        sha256: "be50c3628f2bf5bb5e3a7f17b1f74611b2561a3a27eeab05e5aa30f411572037",
    },
    EmbeddingArtifactSpec {
        filename: "model.safetensors",
        size_bytes: 90_868_376,
        sha256: "53aa51172d142c89d9012cce15ae4d6cc0ca6895895114379cacb4fab128d9db",
    },
];

/// Production MiniLM bundle accepted by Mukei.
pub static ALL_MINILM_L6_V2_MANIFEST: EmbeddingArtifactManifest = EmbeddingArtifactManifest {
    repository: "sentence-transformers/all-MiniLM-L6-v2",
    revision: "1110a243fdf4706b3f48f1d95db1a4f5529b4d41",
    embedding_dim: 384,
    files: &ALL_MINILM_L6_V2_FILES,
};

impl EmbeddingArtifactManifest {
    /// Verify filenames, regular-file identity, direct-child containment, exact
    /// size, and complete SHA-256 digests before returning a proof object.
    pub fn verify_model_dir(
        &self,
        model_dir: impl AsRef<Path>,
    ) -> Result<VerifiedEmbeddingArtifacts> {
        self.validate_contract()?;
        let model_dir = model_dir.as_ref();
        let root_metadata = fs::symlink_metadata(model_dir).map_err(|_| {
            MukeiError::ModelLoadFailed("embedding artifact directory is unavailable".into())
        })?;
        if root_metadata.file_type().is_symlink() || !root_metadata.is_dir() {
            return Err(MukeiError::ModelLoadFailed(
                "embedding artifact root must be a real directory".into(),
            ));
        }
        let canonical_root = fs::canonicalize(model_dir).map_err(|_| {
            MukeiError::ModelLoadFailed("embedding artifact directory could not be resolved".into())
        })?;

        for spec in self.files {
            let candidate = model_dir.join(spec.filename);
            let metadata = fs::symlink_metadata(&candidate).map_err(|_| {
                MukeiError::ModelLoadFailed(format!(
                    "required embedding artifact is missing: {}",
                    spec.filename
                ))
            })?;
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(MukeiError::ModelLoadFailed(format!(
                    "embedding artifact is not a regular file: {}",
                    spec.filename
                )));
            }
            if metadata.len() != spec.size_bytes {
                return Err(MukeiError::ModelCorrupted);
            }

            let canonical_file = fs::canonicalize(&candidate).map_err(|_| {
                MukeiError::ModelLoadFailed(format!(
                    "embedding artifact could not be resolved: {}",
                    spec.filename
                ))
            })?;
            if canonical_file.parent() != Some(canonical_root.as_path()) {
                return Err(MukeiError::ModelLoadFailed(
                    "embedding artifact escaped its verified directory".into(),
                ));
            }

            let actual = sha256_file(&canonical_file, spec.filename)?;
            if actual != spec.sha256 {
                return Err(MukeiError::ModelCorrupted);
            }
        }

        let weights = self
            .files
            .iter()
            .find(|spec| spec.filename == "model.safetensors")
            .ok_or_else(|| {
                MukeiError::Invariant(
                    "embedding artifact manifest is missing model.safetensors".into(),
                )
            })?;
        Ok(VerifiedEmbeddingArtifacts {
            model_dir: canonical_root,
            repository: self.repository,
            revision: self.revision,
            embedding_dim: self.embedding_dim,
            embedder_id: format!("minilm-candle:sha256:{}", weights.sha256),
        })
    }

    fn validate_contract(&self) -> Result<()> {
        if self.repository.trim().is_empty()
            || self.revision.len() != 40
            || !self.revision.bytes().all(|byte| byte.is_ascii_hexdigit())
            || self.embedding_dim == 0
            || self.files.is_empty()
        {
            return Err(MukeiError::Invariant(
                "embedding artifact manifest metadata is invalid".into(),
            ));
        }

        let mut names = std::collections::BTreeSet::new();
        for spec in self.files {
            let path = Path::new(spec.filename);
            if path.file_name().and_then(|name| name.to_str()) != Some(spec.filename)
                || path.components().count() != 1
                || spec.size_bytes == 0
                || spec.sha256.len() != 64
                || !spec.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
                || !names.insert(spec.filename)
            {
                return Err(MukeiError::Invariant(
                    "embedding artifact manifest contains an invalid file entry".into(),
                ));
            }
        }
        Ok(())
    }
}

fn sha256_file(path: &Path, safe_name: &str) -> Result<String> {
    let file = File::open(path).map_err(|_| {
        MukeiError::ModelLoadFailed(format!(
            "embedding artifact could not be opened: {safe_name}"
        ))
    })?;
    let mut reader = BufReader::new(file);
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = reader.read(&mut buffer).map_err(|_| {
            MukeiError::ModelLoadFailed(format!(
                "embedding artifact could not be read: {safe_name}"
            ))
        })?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(crate::diagnostics::crash_logger::hex_helper(
        &hasher.finalize(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    static TEST_FILES: [EmbeddingArtifactSpec; 3] = [
        EmbeddingArtifactSpec {
            filename: "config.json",
            size_bytes: 19,
            sha256: "6249e8142ebd803aa19013aaf414a5437a861116970ae33e07d311d83cccd975",
        },
        EmbeddingArtifactSpec {
            filename: "tokenizer.json",
            size_bytes: 17,
            sha256: "c2823fb776dfaab48bfa06a33005d02a60492d87762cdb66c9c4155f97fbaa5d",
        },
        EmbeddingArtifactSpec {
            filename: "model.safetensors",
            size_bytes: 7,
            sha256: "9a129038d9a00aed0cf6a7ea059ca50a813449061ab87848cf1a13eafdf33b2c",
        },
    ];

    static TEST_MANIFEST: EmbeddingArtifactManifest = EmbeddingArtifactManifest {
        repository: "test/minilm",
        revision: "1110a243fdf4706b3f48f1d95db1a4f5529b4d41",
        embedding_dim: 384,
        files: &TEST_FILES,
    };

    fn fixture() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("config.json"), b"{\"hidden_size\":384}").unwrap();
        fs::write(dir.path().join("tokenizer.json"), b"{\"version\":\"1.0\"}").unwrap();
        fs::write(dir.path().join("model.safetensors"), b"weights").unwrap();
        dir
    }

    #[test]
    fn verifies_complete_bundle_and_derives_stable_identity() {
        let dir = fixture();
        let verified = TEST_MANIFEST.verify_model_dir(dir.path()).unwrap();
        assert_eq!(verified.model_dir(), fs::canonicalize(dir.path()).unwrap());
        assert_eq!(verified.repository(), "test/minilm");
        assert_eq!(verified.revision(), TEST_MANIFEST.revision);
        assert_eq!(verified.embedding_dim(), 384);
        assert_eq!(
            verified.embedder_id(),
            "minilm-candle:sha256:9a129038d9a00aed0cf6a7ea059ca50a813449061ab87848cf1a13eafdf33b2c"
        );
    }

    #[test]
    fn rejects_same_size_hash_mismatch() {
        let dir = fixture();
        fs::write(dir.path().join("model.safetensors"), b"weightx").unwrap();
        assert!(matches!(
            TEST_MANIFEST.verify_model_dir(dir.path()),
            Err(MukeiError::ModelCorrupted)
        ));
    }

    #[test]
    fn rejects_size_mismatch_before_loading() {
        let dir = fixture();
        fs::write(dir.path().join("tokenizer.json"), b"short").unwrap();
        assert!(matches!(
            TEST_MANIFEST.verify_model_dir(dir.path()),
            Err(MukeiError::ModelCorrupted)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlinked_artifact() {
        use std::os::unix::fs::symlink;

        let dir = fixture();
        let outside = tempfile::NamedTempFile::new().unwrap();
        fs::remove_file(dir.path().join("config.json")).unwrap();
        symlink(outside.path(), dir.path().join("config.json")).unwrap();
        assert!(matches!(
            TEST_MANIFEST.verify_model_dir(dir.path()),
            Err(MukeiError::ModelLoadFailed(_))
        ));
    }

    #[test]
    fn rejects_manifest_path_escape() {
        static INVALID_FILES: [EmbeddingArtifactSpec; 1] = [EmbeddingArtifactSpec {
            filename: "../model.safetensors",
            size_bytes: 7,
            sha256: "9a129038d9a00aed0cf6a7ea059ca50a813449061ab87848cf1a13eafdf33b2c",
        }];
        let manifest = EmbeddingArtifactManifest {
            repository: "test/minilm",
            revision: TEST_MANIFEST.revision,
            embedding_dim: 384,
            files: &INVALID_FILES,
        };
        let dir = fixture();
        assert!(matches!(
            manifest.verify_model_dir(dir.path()),
            Err(MukeiError::Invariant(_))
        ));
    }
}
