from pathlib import Path


def replace_once(path: Path, old: str, new: str) -> None:
    text = path.read_text()
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{path}: expected one anchor, found {count}: {old!r}")
    path.write_text(text.replace(old, new, 1))


rag_path = Path("rust/crates/mukei-bridge/src/app_runtime/rag_runtime.rs")
rag_path.parent.mkdir(parents=True, exist_ok=True)
if rag_path.exists():
    raise SystemExit(f"refusing to overwrite existing {rag_path}")
rag_path.write_text(
    '''//! Process-owned RAG retrieval authority.
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
        let status =
            RetrievalStatus::Unavailable(RetrievalUnavailableReason::RetrieverUnavailable);
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
'''
)

app = Path("rust/crates/mukei-bridge/src/app_runtime.rs")
replace_once(
    app,
    "use crate::protocol::ProtocolRuntimeState;\n",
    "use crate::protocol::ProtocolRuntimeState;\n\nmod rag_runtime;\npub(crate) use rag_runtime::RagRuntime;\n",
)
replace_once(
    app,
    "    protocol: ProtocolServices,\n}",
    "    protocol: ProtocolServices,\n    rag: RagServices,\n}",
)
replace_once(
    app,
    "struct ProtocolServices {\n    runtime: ParkingMutex<ProtocolRuntimeState>,\n}\n",
    "struct ProtocolServices {\n    runtime: ParkingMutex<ProtocolRuntimeState>,\n}\n\nstruct RagServices {\n    runtime: Arc<RagRuntime>,\n}\n",
)
replace_once(
    app,
    "            protocol: ProtocolServices {\n                runtime: ParkingMutex::new(ProtocolRuntimeState::new()),\n            },\n",
    "            protocol: ProtocolServices {\n                runtime: ParkingMutex::new(ProtocolRuntimeState::new()),\n            },\n            rag: RagServices {\n                runtime: Arc::new(RagRuntime::new()),\n            },\n",
)
replace_once(
    app,
    "    pub(crate) fn protocol_state(&self) -> &ParkingMutex<ProtocolRuntimeState> {\n        &self.protocol.runtime\n    }\n",
    "    pub(crate) fn rag_runtime(&self) -> Arc<RagRuntime> {\n        self.rag.runtime.clone()\n    }\n\n    pub(crate) fn protocol_state(&self) -> &ParkingMutex<ProtocolRuntimeState> {\n        &self.protocol.runtime\n    }\n",
)
replace_once(
    app,
    "        assert!(Arc::ptr_eq(&first_activation, &second_activation));\n",
    "        assert!(Arc::ptr_eq(&first_activation, &second_activation));\n        let first_rag = application_runtime().rag_runtime();\n        let second_rag = application_runtime().rag_runtime();\n        assert!(Arc::ptr_eq(&first_rag, &second_rag));\n",
)

agent = Path("rust/crates/mukei-bridge/src/agent_runtime.rs")
replace_once(
    agent,
    "use mukei_core::error::Result;\n",
    "use mukei_core::error::Result;\nuse mukei_core::rag::{\n    RetrievalRequest, RetrievalResponse, RetrieverResult, StructuredRetriever,\n};\n",
)
replace_once(
    agent,
    "use crate::core_saf;\n",
    "use crate::app_runtime::{application_runtime, RagRuntime};\nuse crate::core_saf;\n",
)
replace_once(
    agent,
    "pub struct BridgeContextBackend {\n    pool: Arc<mukei_core::storage::DatabasePool>,\n    limit: i64,\n}\n",
    "pub struct BridgeContextBackend {\n    pool: Arc<mukei_core::storage::DatabasePool>,\n    limit: i64,\n    rag: Arc<RagRuntime>,\n}\n",
)
replace_once(
    agent,
    "    pub fn new(pool: Arc<mukei_core::storage::DatabasePool>, limit: i64) -> Self {\n        Self { pool, limit }\n    }\n",
    "    pub fn new(\n        pool: Arc<mukei_core::storage::DatabasePool>,\n        limit: i64,\n        rag: Arc<RagRuntime>,\n    ) -> Self {\n        Self { pool, limit, rag }\n    }\n",
)
replace_once(
    agent,
    "    async fn rag_lookup(&self, _q: &str, _k: usize) -> Result<Vec<String>> {\n        Ok(Vec::new())\n    }\n}\n\n#[cfg(not(feature = \"rusqlite\"))]",
    "    async fn rag_lookup(&self, _q: &str, _k: usize) -> Result<Vec<String>> {\n        Ok(Vec::new())\n    }\n\n    async fn retrieve_rag(\n        &self,\n        request: &RetrievalRequest,\n    ) -> RetrieverResult<RetrievalResponse> {\n        self.rag.retrieve_structured(request).await\n    }\n}\n\n#[cfg(not(feature = \"rusqlite\"))]",
)
replace_once(
    agent,
    "pub struct BridgeContextBackend {\n    limit: i64,\n}\n\n#[cfg(not(feature = \"rusqlite\"))]\nimpl BridgeContextBackend {\n    pub fn new(limit: i64) -> Self {\n        Self { limit }\n    }\n}\n",
    "pub struct BridgeContextBackend {\n    limit: i64,\n    rag: Arc<RagRuntime>,\n}\n\n#[cfg(not(feature = \"rusqlite\"))]\nimpl BridgeContextBackend {\n    pub fn new(limit: i64, rag: Arc<RagRuntime>) -> Self {\n        Self { limit, rag }\n    }\n}\n",
)
replace_once(
    agent,
    "    async fn rag_lookup(&self, _q: &str, _k: usize) -> Result<Vec<String>> {\n        Ok(Vec::new())\n    }\n}\n\n/// Build the shared",
    "    async fn rag_lookup(&self, _q: &str, _k: usize) -> Result<Vec<String>> {\n        Ok(Vec::new())\n    }\n\n    async fn retrieve_rag(\n        &self,\n        request: &RetrievalRequest,\n    ) -> RetrieverResult<RetrievalResponse> {\n        self.rag.retrieve_structured(request).await\n    }\n}\n\n/// Build the shared",
)
replace_once(
    agent,
    "    let backend = Arc::new(BridgeContextBackend::new(\n        pool.clone(),\n        cfg.agent.recovered_history_window as i64,\n    ));\n",
    "    let backend = Arc::new(BridgeContextBackend::new(\n        pool.clone(),\n        cfg.agent.recovered_history_window as i64,\n        application_runtime().rag_runtime(),\n    ));\n",
)
replace_once(
    agent,
    "    let backend = Arc::new(BridgeContextBackend::new(\n        cfg.agent.recovered_history_window as i64,\n    ));\n",
    "    let backend = Arc::new(BridgeContextBackend::new(\n        cfg.agent.recovered_history_window as i64,\n        application_runtime().rag_runtime(),\n    ));\n",
)
