//! `mukei_core::rag` — TRD §4 / PRD §9.
//!
//! Modules:
//! - [`embedder`] — Embedder trait + MockEmbedder + (feature `candle`)
//!   real on-device MiniLM forward pass.
//! - [`vector_store`] — atomic-rename persistence, optional usearch HNSW
//!   backend (feature `usearch_hnsw`), embedder-swap detection, shred /
//!   forget functionality.
//! - [`chunker`] — 256-token windows, 32-token overlap.
//! - [`indexer`] — `IndexingTransaction` with schema-faithful INSERTs,
//!   RAII rollback, broadcast progress.
//! - [`retriever`] — query embedding, vector search, injected chunk resolution,
//!   and structured ranked results.

pub mod chunker;
pub mod embedder;
pub mod indexer;
pub mod retriever;
pub mod vector_store;

#[cfg(feature = "candle")]
pub use embedder::{CandleConfig, CandleMiniLmEmbedder, Pooling};
pub use embedder::{Embedder, Embedding, MockEmbedder};

pub use indexer::{
    handle_revoke, BackgroundIndexer, FileSaw, IndexProgress, IndexingTransaction, StagedChunk,
};

pub use retriever::{
    normalize_and_budget_results, ChunkResolver, IndexCompatibilityRequirement,
    IndexCompatibilityState, IndexMetadata, RagCapabilitySnapshot, ResolvedChunk, RetrievalBudget,
    RetrievalDegradedReason, RetrievalDiagnostics, RetrievalRequest, RetrievalResponse,
    RetrievalScope, RetrievalStatus, RetrievalUnavailableReason, RetrievedChunk, Retriever,
    RetrieverError, RetrieverResult, SourceFilters, StructuredRetriever,
    CONTENT_HASH_DEDUPE_MIN_BYTES, DEFAULT_MAX_CHUNK_BYTES, DEFAULT_RETRIEVAL_TOP_K,
};

pub use vector_store::{
    RebuildVerdict, StoreHeader, VectorStore, VectorStoreError, ATOMIC_SUFFIX, STORE_FORMAT_VERSION,
};
