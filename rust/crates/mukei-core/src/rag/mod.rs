//! `mukei_core::rag` — TRD §4.
//!
//! Modules:
//! - `embedder`         — wraps `candle` MiniLM into a typed API.
//! - `vector_store`     — wraps `usearch` HNSW into a save/load with
//!                         atomic-rename (TRD §4.2 — ` .tmp → `).
//! - `chunker`          — 256-token windows, 32-token overlap.
//! - `indexer`          — background tokio task with rollback safe-handle
//!                         for SAF-revoke (TRD §4.4, BUGFIX v0.7.4).

pub mod chunker;
pub mod embedder;
pub mod indexer;
pub mod vector_store;

pub use embedder::{Embedder, Embedding};
pub use indexer::{BackgroundIndexer, IndexingTransaction};
pub use vector_store::{VectorStore, VectorStoreError, ATOMIC_SUFFIX};
