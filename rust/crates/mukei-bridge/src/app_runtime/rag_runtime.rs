//! Process-owned RAG retrieval authority.
//!
//! This first construction slice is intentionally conservative: the service is
//! real and injectable, but reports `RetrieverUnavailable` until a verified
//! embedder/vector-store generation is published by a later remediation slice.
//! Returning a typed unavailable state lets context assembly degrade safely
//! without misrepresenting an absent index as a successful empty retrieval.

use mukei_core::rag::{
    IndexCompatibilityState, RagCapabilitySnapshot, RetrievalRequest, RetrievalResponse,
    RetrievalStatus, RetrievalUnavailableReason, RetrieverError, RetrieverResult,
    StructuredRetriever,
};

#[derive(Debug, Default)]
pub(crate) struct RagRuntime;

impl RagRuntime {
    pub(crate) fn new() -> Self {
        Self
    }

    fn unavailable_capability() -> RagCapabilitySnapshot {
        let status = RetrievalStatus::Unavailable(RetrievalUnavailableReason::RetrieverUnavailable);
        RagCapabilitySnapshot {
            code_present: true,
            index_available: false,
            embedding_backend_available: false,
            index_compatibility: IndexCompatibilityState::Unknown,
            retrieval_state: status,
        }
    }
}

#[async_trait::async_trait]
impl StructuredRetriever for RagRuntime {
    async fn retrieve_structured(
        &self,
        _request: &RetrievalRequest,
    ) -> RetrieverResult<RetrievalResponse> {
        Err(RetrieverError::Unavailable)
    }

    fn capability_snapshot(&self) -> RagCapabilitySnapshot {
        Self::unavailable_capability()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
