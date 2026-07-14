//! Process-owned RAG retrieval and embedding authority.
//!
//! Embedder construction is deliberately non-fatal to application boot. The
//! runtime reports a typed unavailable state until a verified artifact bundle
//! is loaded, and then reports `IndexUnavailable` until a compatible vector
//! generation is published by the indexing slice.

use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::RwLock;

#[cfg(any(feature = "candle", test))]
use mukei_core::error::{MukeiError, Result};
#[cfg(feature = "candle")]
use mukei_core::rag::{CandleMiniLmEmbedder, ALL_MINILM_L6_V2_MANIFEST};
#[cfg(any(feature = "candle", test))]
use mukei_core::rag::{Embedder, VerifiedEmbeddingArtifacts};
use mukei_core::rag::{
    IndexCompatibilityState, RagCapabilitySnapshot, RetrievalRequest, RetrievalResponse,
    RetrievalStatus, RetrievalUnavailableReason, RetrieverError, RetrieverResult,
    StructuredRetriever,
};

#[derive(Clone, Copy)]
enum UnavailableState {
    Retriever,
    LoadingEmbedder,
    Embedder,
}

#[derive(Clone)]
enum RuntimeState {
    Unavailable(UnavailableState),
    #[cfg(any(feature = "candle", test))]
    EmbedderReady {
        embedder: Arc<dyn Embedder>,
        artifacts: VerifiedEmbeddingArtifacts,
    },
}

pub(crate) struct RagRuntime {
    state: RwLock<RuntimeState>,
}

impl Default for RagRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl RagRuntime {
    pub(crate) fn new() -> Self {
        Self {
            state: RwLock::new(RuntimeState::Unavailable(UnavailableState::Retriever)),
        }
    }

    pub(crate) fn ensure_embedder_initialization(self: &Arc<Self>, models_dir: PathBuf) {
        let should_start = {
            let mut state = self.state.write();
            if matches!(
                &*state,
                RuntimeState::Unavailable(UnavailableState::Retriever)
            ) {
                *state = RuntimeState::Unavailable(UnavailableState::LoadingEmbedder);
                true
            } else {
                false
            }
        };
        if !should_start {
            return;
        }

        #[cfg(feature = "candle")]
        {
            let runtime = Arc::clone(self);
            let model_dir = models_dir
                .join("embeddings")
                .join("all-MiniLM-L6-v2")
                .join(ALL_MINILM_L6_V2_MANIFEST.revision);
            mukei_core::runtime::get().spawn(async move {
                let load = tokio::task::spawn_blocking(move || {
                    let artifacts = ALL_MINILM_L6_V2_MANIFEST.verify_model_dir(model_dir)?;
                    let embedder = CandleMiniLmEmbedder::from_verified_artifacts(&artifacts)?;
                    Ok::<_, MukeiError>((Arc::new(embedder) as Arc<dyn Embedder>, artifacts))
                })
                .await
                .map_err(|error| MukeiError::BlockingJoinFailed(error.to_string()))
                .and_then(|result| result);

                match load {
                    Ok((embedder, artifacts)) => {
                        if let Err(error) = runtime.install_verified_embedder(embedder, artifacts) {
                            tracing::warn!(
                                code = error.error_code(),
                                "verified RAG embedder failed runtime identity checks"
                            );
                            runtime.mark_embedder_unavailable();
                        } else {
                            tracing::info!(
                                revision = ALL_MINILM_L6_V2_MANIFEST.revision,
                                "verified RAG embedder is ready; index remains unavailable"
                            );
                        }
                    }
                    Err(error) => {
                        tracing::warn!(
                            code = error.error_code(),
                            "RAG embedder artifacts are unavailable or invalid"
                        );
                        runtime.mark_embedder_unavailable();
                    }
                }
            });
        }

        #[cfg(not(feature = "candle"))]
        {
            let _ = models_dir;
            self.mark_embedder_unavailable();
        }
    }

    #[cfg(any(feature = "candle", test))]
    fn install_verified_embedder(
        &self,
        embedder: Arc<dyn Embedder>,
        artifacts: VerifiedEmbeddingArtifacts,
    ) -> Result<()> {
        if embedder.dim() != artifacts.embedding_dim() as usize
            || embedder.embedder_id() != artifacts.embedder_id()
        {
            return Err(MukeiError::ModelCorrupted);
        }
        *self.state.write() = RuntimeState::EmbedderReady {
            embedder,
            artifacts,
        };
        Ok(())
    }

    fn mark_embedder_unavailable(&self) {
        *self.state.write() = RuntimeState::Unavailable(UnavailableState::Embedder);
    }

    fn unavailable_capability(state: UnavailableState) -> RagCapabilitySnapshot {
        let reason = match state {
            UnavailableState::Retriever => RetrievalUnavailableReason::RetrieverUnavailable,
            UnavailableState::LoadingEmbedder | UnavailableState::Embedder => {
                RetrievalUnavailableReason::EmbedderUnavailable
            }
        };
        RagCapabilitySnapshot {
            code_present: true,
            index_available: false,
            embedding_backend_available: false,
            index_compatibility: IndexCompatibilityState::Unknown,
            retrieval_state: RetrievalStatus::Unavailable(reason),
        }
    }
}

#[async_trait::async_trait]
impl StructuredRetriever for RagRuntime {
    async fn retrieve_structured(
        &self,
        _request: &RetrievalRequest,
    ) -> RetrieverResult<RetrievalResponse> {
        match self.state.read().clone() {
            RuntimeState::Unavailable(UnavailableState::Retriever) => {
                Err(RetrieverError::Unavailable)
            }
            RuntimeState::Unavailable(
                UnavailableState::LoadingEmbedder | UnavailableState::Embedder,
            ) => Err(RetrieverError::EmbedderUnavailable),
            #[cfg(any(feature = "candle", test))]
            RuntimeState::EmbedderReady { .. } => Err(RetrieverError::IndexUnavailable),
        }
    }

    fn capability_snapshot(&self) -> RagCapabilitySnapshot {
        match self.state.read().clone() {
            RuntimeState::Unavailable(state) => Self::unavailable_capability(state),
            #[cfg(any(feature = "candle", test))]
            RuntimeState::EmbedderReady {
                embedder,
                artifacts,
            } => {
                debug_assert_eq!(embedder.dim(), artifacts.embedding_dim() as usize);
                debug_assert_eq!(embedder.embedder_id(), artifacts.embedder_id());
                let status =
                    RetrievalStatus::Unavailable(RetrievalUnavailableReason::IndexUnavailable);
                RagCapabilitySnapshot {
                    code_present: true,
                    index_available: false,
                    embedding_backend_available: true,
                    index_compatibility: IndexCompatibilityState::Missing,
                    retrieval_state: status,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use mukei_core::rag::{
        Embedding, EmbeddingArtifactManifest, EmbeddingArtifactSpec, RetrievalStatus,
    };

    use super::*;

    struct VerifiedTestEmbedder {
        id: String,
    }

    #[async_trait::async_trait]
    impl Embedder for VerifiedTestEmbedder {
        async fn embed(&self, _text: &str) -> Result<Embedding> {
            Ok(Embedding(vec![1.0; 384]).l2_normalise())
        }

        fn dim(&self) -> usize {
            384
        }

        fn embedder_id(&self) -> &str {
            &self.id
        }
    }

    fn verified_fixture() -> (tempfile::TempDir, VerifiedEmbeddingArtifacts) {
        static FILES: [EmbeddingArtifactSpec; 3] = [
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
        static MANIFEST: EmbeddingArtifactManifest = EmbeddingArtifactManifest {
            repository: "test/minilm",
            revision: "1110a243fdf4706b3f48f1d95db1a4f5529b4d41",
            embedding_dim: 384,
            files: &FILES,
        };
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("config.json"), b"{\"hidden_size\":384}").unwrap();
        fs::write(dir.path().join("tokenizer.json"), b"{\"version\":\"1.0\"}").unwrap();
        fs::write(dir.path().join("model.safetensors"), b"weights").unwrap();
        let artifacts = MANIFEST.verify_model_dir(dir.path()).unwrap();
        (dir, artifacts)
    }

    #[tokio::test]
    async fn unavailable_runtime_is_typed_and_truthful() {
        let runtime = RagRuntime::new();
        let capability = runtime.capability_snapshot();
        assert!(capability.code_present);
        assert!(!capability.index_available);
        assert!(!capability.embedding_backend_available);
        assert!(matches!(
            capability.retrieval_state,
            RetrievalStatus::Unavailable(RetrievalUnavailableReason::RetrieverUnavailable)
        ));
        assert!(matches!(
            runtime
                .retrieve_structured(&RetrievalRequest::new("hello"))
                .await,
            Err(RetrieverError::Unavailable)
        ));
    }

    #[tokio::test]
    async fn verified_embedder_is_not_misreported_as_a_ready_index() {
        let runtime = RagRuntime::new();
        let (_dir, artifacts) = verified_fixture();
        let embedder = Arc::new(VerifiedTestEmbedder {
            id: artifacts.embedder_id().to_string(),
        });
        runtime
            .install_verified_embedder(embedder, artifacts)
            .unwrap();
        let capability = runtime.capability_snapshot();
        assert!(capability.embedding_backend_available);
        assert!(!capability.index_available);
        assert!(matches!(
            capability.retrieval_state,
            RetrievalStatus::Unavailable(RetrievalUnavailableReason::IndexUnavailable)
        ));
        assert!(matches!(
            runtime
                .retrieve_structured(&RetrievalRequest::new("hello"))
                .await,
            Err(RetrieverError::IndexUnavailable)
        ));
    }
}
