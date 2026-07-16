use std::path::Path;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use zeroize::Zeroizing;

use crate::error::MukeiError;

/// Encrypted authoritative projection store injected by the platform composition root.
#[async_trait]
pub trait RuntimeProjectionStore: Send + Sync {
    async fn load(&self, key: &str) -> Result<Option<Value>, MukeiError>;
    async fn save(&self, key: &str, value: Value) -> Result<(), MukeiError>;
    async fn delete(&self, key: &str) -> Result<(), MukeiError>;
}

/// Result of one completed document ingestion operation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RagIngestResult {
    pub chunk_count: usize,
}

/// Production RAG boundary injected by the native Android composition root.
#[async_trait]
pub trait RuntimeRagService: Send + Sync {
    async fn ingest_document(
        &self,
        document_id: &str,
        staged_path: &Path,
        mime_type: &str,
    ) -> Result<RagIngestResult, MukeiError>;

    async fn retrieve(&self, query: &str, top_k: usize) -> Result<Vec<String>, MukeiError>;

    async fn revoke_document(&self, document_id: &str) -> Result<usize, MukeiError>;
}

struct RemoteToolSecrets {
    brave_key: Zeroizing<String>,
    tavily_key: Zeroizing<String>,
}
