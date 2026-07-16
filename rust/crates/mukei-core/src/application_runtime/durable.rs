/// Encrypted authoritative projection store injected by the platform composition root.
#[async_trait::async_trait]
pub trait RuntimeProjectionStore: Send + Sync {
    async fn load(
        &self,
        key: &str,
    ) -> std::result::Result<Option<serde_json::Value>, crate::error::MukeiError>;

    async fn save(
        &self,
        key: &str,
        value: serde_json::Value,
    ) -> std::result::Result<(), crate::error::MukeiError>;

    async fn delete(&self, key: &str) -> std::result::Result<(), crate::error::MukeiError>;
}

/// Result of one completed document ingestion operation.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RagIngestResult {
    pub chunk_count: usize,
}

/// Production RAG boundary injected by the native Android composition root.
#[async_trait::async_trait]
pub trait RuntimeRagService: Send + Sync {
    async fn ingest_document(
        &self,
        document_id: &str,
        staged_path: &std::path::Path,
        mime_type: &str,
    ) -> std::result::Result<RagIngestResult, crate::error::MukeiError>;

    async fn retrieve(
        &self,
        query: &str,
        top_k: usize,
    ) -> std::result::Result<Vec<String>, crate::error::MukeiError>;

    async fn revoke_document(
        &self,
        document_id: &str,
    ) -> std::result::Result<usize, crate::error::MukeiError>;
}

struct RemoteToolSecrets {
    brave_key: zeroize::Zeroizing<String>,
    tavily_key: zeroize::Zeroizing<String>,
}
