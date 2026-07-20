use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use mukei_core::application_runtime::{RagIngestResult, RuntimeRagService};
use mukei_core::error::{MukeiError, Result};
use mukei_core::rag::chunker::Chunker;
use mukei_core::rag::{
    CandleMiniLmEmbedder, ChunkResolver, Embedder, IndexMetadata, ResolvedChunk, RetrievalRequest,
    RetrievalScope, Retriever, StoreHeader, VectorStore, ALL_MINILM_L6_V2_MANIFEST,
    STORE_FORMAT_VERSION,
};
use mukei_core::storage::{DatabasePool, DbError, PooledConnectionExt};
use tokio::sync::Mutex;
use uuid::Uuid;

const MAX_DOCUMENT_BYTES: u64 = 32 * 1024 * 1024;

struct SqlChunkResolver {
    pool: Arc<DatabasePool>,
    embedder_id: String,
}

#[async_trait]
impl ChunkResolver for SqlChunkResolver {
    async fn resolve_chunks(
        &self,
        request: &RetrievalRequest,
        ids: &[u64],
    ) -> Result<Vec<ResolvedChunk>> {
        let ids = ids.to_vec();
        let scope = request.scope.clone();
        let embedder_id = self.embedder_id.clone();
        self.pool
            .with_conn(move |connection| {
                let mut output = Vec::new();
                let mut statement = connection.prepare(
                    "SELECT chunk_uuid, content, file_token, conversation_id, message_id, ordinal, embedding_dim \
                     FROM chunks WHERE chunk_uuid = ?1",
                )?;
                for id in ids {
                    let row = statement.query_row([id.to_string()], |row| {
                        let chunk_uuid: String = row.get(0)?;
                        let document_id: Option<String> = row.get(2)?;
                        let dimension: i64 = row.get(6)?;
                        Ok(ResolvedChunk {
                            chunk_id: chunk_uuid.parse::<u64>().unwrap_or(id),
                            document_id: document_id.clone(),
                            content: row.get(1)?,
                            source_id: document_id,
                            conversation_id: row.get(3)?,
                            message_id: row.get(4)?,
                            ordinal: Some(row.get::<_, i64>(5)? as u32),
                            scope: scope.clone(),
                            authorization_marker: None,
                            index_metadata: Some(IndexMetadata {
                                format_version: STORE_FORMAT_VERSION,
                                embedder_id: embedder_id.clone(),
                                embedding_dim: dimension as u32,
                            }),
                        })
                    });
                    if let Ok(chunk) = row {
                        output.push(chunk);
                    }
                }
                Ok::<_, DbError>(output)
            })
            .await
    }
}

pub(crate) struct AndroidRagService {
    app_root: PathBuf,
    pool: Arc<DatabasePool>,
    embedder: Arc<dyn Embedder>,
    vector_store: Arc<VectorStore>,
    retriever: Arc<Retriever>,
    write_lock: Mutex<()>,
}

impl AndroidRagService {
    pub(crate) fn open(app_root: &Path, pool: Arc<DatabasePool>) -> Result<Arc<Self>> {
        let embedding_dir = app_root.join("models/embeddings/all-MiniLM-L6-v2");
        let verified = ALL_MINILM_L6_V2_MANIFEST.verify_model_dir(&embedding_dir)?;
        let embedder: Arc<dyn Embedder> =
            Arc::new(CandleMiniLmEmbedder::from_verified_artifacts(&verified)?);
        let vector_path = app_root.join("vectors/documents.vectors.json");
        let vector_store = Arc::new(VectorStore::open(vector_path));
        vector_store.load()?;
        let expected_header = StoreHeader {
            format_version: STORE_FORMAT_VERSION,
            embedder_id: embedder.embedder_id().to_owned(),
            embedding_dim: embedder.dim() as u32,
        };
        if vector_store.header().is_some() {
            vector_store.assert_compatible_with(&expected_header)?;
        } else {
            vector_store.set_header(expected_header);
        }
        let resolver: Arc<dyn ChunkResolver> = Arc::new(SqlChunkResolver {
            pool: Arc::clone(&pool),
            embedder_id: embedder.embedder_id().to_owned(),
        });
        let retriever = Arc::new(Retriever::new(
            Arc::clone(&embedder),
            Arc::clone(&vector_store),
            resolver,
        ));
        Ok(Arc::new(Self {
            app_root: app_root.to_path_buf(),
            pool,
            embedder,
            vector_store,
            retriever,
            write_lock: Mutex::new(()),
        }))
    }

    fn validate_document_path(&self, path: &Path) -> Result<PathBuf> {
        let canonical_root = self
            .app_root
            .canonicalize()
            .map_err(|error| MukeiError::Io(error.to_string()))?;
        let canonical = path
            .canonicalize()
            .map_err(|error| MukeiError::FileReadFailed(error.to_string()))?;
        if !canonical.starts_with(&canonical_root) || !canonical.is_file() {
            return Err(MukeiError::SandboxViolation);
        }
        Ok(canonical)
    }

    async fn persist_vectors(&self) -> Result<()> {
        let snapshot = self.vector_store.snapshot_for_save()?;
        let path = self.vector_store.path().to_path_buf();
        tokio::task::spawn_blocking(move || VectorStore::save_snapshot(&path, &snapshot))
            .await
            .map_err(|error| MukeiError::BlockingJoinFailed(error.to_string()))??;
        Ok(())
    }

    async fn index_text_locked(&self, document_id: &str, content: &str) -> Result<RagIngestResult> {
        if content.len() as u64 > MAX_DOCUMENT_BYTES {
            return Err(MukeiError::FileReadFailed(
                "document exceeds indexing limit".into(),
            ));
        }
        let chunks = Chunker::default().split(content);
        if chunks.is_empty() {
            return Err(MukeiError::FileReadFailed(
                "document contains no indexable text".into(),
            ));
        }

        let old_ids = self
            .pool
            .with_conn({
                let document_id = document_id.to_owned();
                move |connection| {
                    let mut statement = connection
                        .prepare("SELECT chunk_uuid FROM chunks WHERE file_token = ?1")?;
                    let ids = statement
                        .query_map([document_id], |row| row.get::<_, String>(0))?
                        .filter_map(|row| row.ok().and_then(|value| value.parse::<u64>().ok()))
                        .collect::<Vec<_>>();
                    Ok::<_, DbError>(ids)
                }
            })
            .await?;

        let mut staged = Vec::with_capacity(chunks.len());
        for chunk in chunks {
            let id = ((Uuid::new_v4().as_u128() & u128::from(u64::MAX)) as u64).max(1);
            let embedding = match self.embedder.embed(&chunk.text).await {
                Ok(value) => value,
                Err(error) => {
                    let ids = staged.iter().map(|(id, _)| *id).collect::<Vec<_>>();
                    self.vector_store.shred_many(&ids);
                    return Err(error);
                }
            };
            self.vector_store.add_scoped(
                id,
                embedding.0,
                chunk.digest.clone(),
                RetrievalScope::local(),
            );
            staged.push((id, chunk));
        }

        let insert_result = self
            .pool
            .with_conn({
                let document_id = document_id.to_owned();
                let dimension = self.embedder.dim() as i64;
                let staged = staged.clone();
                move |connection| {
                    let transaction = connection.transaction()?;
                    transaction.execute(
                        "DELETE FROM chunks WHERE file_token = ?1",
                        [&document_id],
                    )?;
                    {
                        let mut statement = transaction.prepare(
                            "INSERT INTO chunks \
                                (chunk_uuid, file_token, ordinal, sha256, token_count, embedding_dim, content) \
                             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                        )?;
                        for (id, chunk) in &staged {
                            statement.execute(rusqlite::params![
                                id.to_string(),
                                &document_id,
                                chunk.index as i64,
                                &chunk.digest,
                                chunk.text.split_whitespace().count() as i64,
                                dimension,
                                &chunk.text,
                            ])?;
                        }
                    }
                    transaction.commit()?;
                    Ok::<_, DbError>(())
                }
            })
            .await;
        if let Err(error) = insert_result {
            let ids = staged.iter().map(|(id, _)| *id).collect::<Vec<_>>();
            self.vector_store.shred_many(&ids);
            return Err(error);
        }
        self.vector_store.shred_many(&old_ids);
        self.persist_vectors().await?;
        Ok(RagIngestResult {
            chunk_count: staged.len(),
        })
    }
}

#[async_trait]
impl RuntimeRagService for AndroidRagService {
    async fn ingest_document(
        &self,
        document_id: &str,
        staged_path: &Path,
        mime_type: &str,
    ) -> Result<RagIngestResult> {
        if !matches!(
            mime_type,
            "text/plain" | "text/markdown" | "application/json" | "text/csv"
        ) {
            return Err(MukeiError::BinaryFile);
        }
        let canonical = self.validate_document_path(staged_path)?;
        let metadata = std::fs::metadata(&canonical)
            .map_err(|error| MukeiError::FileReadFailed(error.to_string()))?;
        if metadata.len() > MAX_DOCUMENT_BYTES {
            return Err(MukeiError::FileReadFailed(
                "document exceeds indexing limit".into(),
            ));
        }
        let content = tokio::task::spawn_blocking(move || std::fs::read_to_string(canonical))
            .await
            .map_err(|error| MukeiError::BlockingJoinFailed(error.to_string()))?
            .map_err(|error| MukeiError::FileReadFailed(error.to_string()))?;
        self.ingest_text(document_id, &content).await
    }

    async fn ingest_text(&self, document_id: &str, text: &str) -> Result<RagIngestResult> {
        let _guard = self.write_lock.lock().await;
        self.index_text_locked(document_id, text).await
    }

    async fn retrieve(&self, query: &str, top_k: usize) -> Result<Vec<String>> {
        let request = RetrievalRequest::new(query).with_top_k(top_k);
        let response = self
            .retriever
            .retrieve_structured(&request)
            .await
            .map_err(|error| MukeiError::ToolExecutionFailed(error.to_string()))?;
        Ok(response
            .results
            .into_iter()
            .map(|chunk| chunk.content)
            .collect())
    }

    async fn revoke_document(&self, document_id: &str) -> Result<usize> {
        let _guard = self.write_lock.lock().await;
        let document_id_owned = document_id.to_owned();
        let ids = self
            .pool
            .with_conn(move |connection| {
                let transaction = connection.transaction()?;
                let ids = {
                    let mut statement = transaction
                        .prepare("SELECT chunk_uuid FROM chunks WHERE file_token = ?1")?;
                    let rows =
                        statement.query_map([&document_id_owned], |row| row.get::<_, String>(0))?;
                    rows.filter_map(|row| row.ok().and_then(|value| value.parse::<u64>().ok()))
                        .collect::<Vec<_>>()
                };
                transaction.execute(
                    "DELETE FROM chunks WHERE file_token = ?1",
                    [&document_id_owned],
                )?;
                transaction.commit()?;
                Ok::<_, DbError>(ids)
            })
            .await?;
        let removed = self.vector_store.shred_many(&ids);
        self.persist_vectors().await?;
        Ok(removed)
    }
}
