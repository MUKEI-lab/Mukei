//! Scope-safe, budget-aware structured RAG retrieval.
//!
//! The production contract in this module deliberately keeps authorization
//! scope explicit. A retriever request carries tenant/workspace scope, the
//! vector-store lookup is filtered by that scope, resolver output is validated
//! again, and callers can re-run [`normalize_and_budget_results`] as a final
//! context-assembly defense.
//!
//! Retrieved document text is data, never authority. Prompt formatting belongs
//! to `agent::context`; this module preserves provenance and enforces the
//! retrieval-side invariants required before any text reaches that layer.

use std::{
    collections::{BTreeMap, BTreeSet},
    future::Future,
    sync::Arc,
    time::Duration,
};

use serde::{Deserialize, Serialize};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

use crate::error::{MukeiError, Result};
use crate::rag::embedder::Embedder;
use crate::rag::vector_store::{RebuildVerdict, StoreHeader, VectorStore, STORE_FORMAT_VERSION};

/// Default maximum number of vector matches requested for a retrieval.
pub const DEFAULT_RETRIEVAL_TOP_K: usize = 8;
/// Default per-passage byte cap applied before prompt assembly.
pub const DEFAULT_MAX_CHUNK_BYTES: usize = 4096;
/// Minimum normalized content length before content-hash fallback dedupe is
/// enabled. This avoids collapsing distinct tiny chunks that happen to share a
/// short boilerplate sentence.
pub const CONTENT_HASH_DEDUPE_MIN_BYTES: usize = 32;

/// Explicit tenant/workspace authorization boundary for one retrieval.
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct RetrievalScope {
    /// Opaque tenant identifier. It is never logged by this module.
    pub tenant_id: String,
    /// Opaque workspace identifier. It is never logged by this module.
    pub workspace_id: String,
    /// Optional actor/user scope supported by the current caller.
    pub actor_id: Option<String>,
    /// Optional opaque authorization marker supplied by the caller.
    pub authorization_marker: Option<String>,
}

impl RetrievalScope {
    /// Build an explicit tenant/workspace scope.
    pub fn new(tenant_id: impl Into<String>, workspace_id: impl Into<String>) -> Self {
        Self {
            tenant_id: tenant_id.into(),
            workspace_id: workspace_id.into(),
            actor_id: None,
            authorization_marker: None,
        }
    }

    /// Compatibility scope for the current single-user local deployment.
    ///
    /// This is a fixed request value, not mutable process-global authorization.
    /// Multi-tenant callers should use [`Self::new`] and pass their real scope.
    pub fn local() -> Self {
        Self::new("local-tenant", "local-workspace")
    }

    /// Attach an explicit actor/user scope.
    pub fn with_actor_id(mut self, actor_id: impl Into<String>) -> Self {
        self.actor_id = Some(actor_id.into());
        self
    }

    /// Attach an opaque authorization marker.
    pub fn with_authorization_marker(mut self, marker: impl Into<String>) -> Self {
        self.authorization_marker = Some(marker.into());
        self
    }

    fn validate(&self) -> RetrieverResult<()> {
        if self.tenant_id.trim().is_empty()
            || self.workspace_id.trim().is_empty()
            || self
                .actor_id
                .as_ref()
                .is_some_and(|value| value.trim().is_empty())
            || self
                .authorization_marker
                .as_ref()
                .is_some_and(|value| value.trim().is_empty())
        {
            return Err(RetrieverError::ScopeInvalid);
        }
        Ok(())
    }
}

/// Optional source restrictions supplied with a retrieval request.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceFilters {
    /// Allowed opaque source identifiers. Empty means unrestricted.
    pub source_ids: BTreeSet<String>,
    /// Allowed opaque document identifiers. Empty means unrestricted.
    pub document_ids: BTreeSet<String>,
}

impl SourceFilters {
    fn allows(&self, chunk: &ResolvedChunk) -> bool {
        self.allows_values(chunk.source_id.as_ref(), chunk.document_id.as_ref())
    }

    fn allows_retrieved(&self, chunk: &RetrievedChunk) -> bool {
        self.allows_values(chunk.source_id.as_ref(), chunk.document_id.as_ref())
    }

    fn allows_values(&self, source_id: Option<&String>, document_id: Option<&String>) -> bool {
        let source_allowed = self.source_ids.is_empty()
            || source_id.is_some_and(|source| self.source_ids.contains(source));
        let document_allowed = self.document_ids.is_empty()
            || document_id.is_some_and(|document| self.document_ids.contains(document));
        source_allowed && document_allowed
    }
}

/// Explicit retrieval/context budget policy.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetrievalBudget {
    /// Maximum number of results after normalization and deduplication.
    pub max_results: usize,
    /// Maximum aggregate bytes of retrieved text admitted to context assembly.
    pub max_total_bytes: usize,
    /// Maximum bytes admitted from any one chunk.
    pub max_chunk_bytes: usize,
    /// Tokens callers must reserve for system/user/tool/history context.
    pub reserved_context_tokens: u32,
    /// Optional deterministic cap per document/source identity.
    pub per_document_cap: Option<usize>,
}

impl Default for RetrievalBudget {
    fn default() -> Self {
        Self {
            max_results: DEFAULT_RETRIEVAL_TOP_K,
            max_total_bytes: DEFAULT_MAX_CHUNK_BYTES * 4,
            max_chunk_bytes: DEFAULT_MAX_CHUNK_BYTES,
            reserved_context_tokens: 512,
            per_document_cap: Some(3),
        }
    }
}

/// Required index/embedder compatibility for a request.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexCompatibilityRequirement {
    /// Required on-disk vector-store format version.
    pub format_version: u32,
    /// Required embedding model/tokenizer identity.
    pub embedder_id: String,
    /// Required embedding dimension.
    pub embedding_dim: u32,
}

impl From<&StoreHeader> for IndexCompatibilityRequirement {
    fn from(value: &StoreHeader) -> Self {
        Self {
            format_version: value.format_version,
            embedder_id: value.embedder_id.clone(),
            embedding_dim: value.embedding_dim,
        }
    }
}

/// Index/embedding provenance attached to retrieved chunks when available.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexMetadata {
    /// Vector-store format version.
    pub format_version: u32,
    /// Embedding model/tokenizer identity.
    pub embedder_id: String,
    /// Embedding dimension.
    pub embedding_dim: u32,
}

impl From<&StoreHeader> for IndexMetadata {
    fn from(value: &StoreHeader) -> Self {
        Self {
            format_version: value.format_version,
            embedder_id: value.embedder_id.clone(),
            embedding_dim: value.embedding_dim,
        }
    }
}

/// Structured input for one retrieval operation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RetrievalRequest {
    /// Natural-language query to embed and search for.
    pub query: String,
    /// Explicit tenant/workspace authorization scope.
    #[serde(default = "RetrievalScope::local")]
    pub scope: RetrievalScope,
    /// Maximum number of vector matches requested before final budgeting.
    #[serde(default = "default_retrieval_top_k")]
    pub top_k: usize,
    /// Optional inclusive normalized relevance floor in `[0, 1]`.
    #[serde(default)]
    pub min_score: Option<f32>,
    /// Prompt/context budget policy.
    #[serde(default)]
    pub context_budget: RetrievalBudget,
    /// Optional source restrictions.
    #[serde(default)]
    pub source_filters: SourceFilters,
    /// Optional required index/embedder version.
    #[serde(default)]
    pub required_index: Option<IndexCompatibilityRequirement>,
    /// Optional caller-provided cancellation context. Runtime-only and never
    /// serialized into persisted request data.
    #[serde(skip, default)]
    pub cancellation: Option<CancellationToken>,
    /// Optional overall retrieval timeout. Runtime-only.
    #[serde(skip, default)]
    pub timeout: Option<Duration>,
}

const fn default_retrieval_top_k() -> usize {
    DEFAULT_RETRIEVAL_TOP_K
}

impl RetrievalRequest {
    /// Construct a compatibility request for the current local deployment.
    pub fn new(query: impl Into<String>) -> Self {
        Self::new_scoped(query, RetrievalScope::local())
    }

    /// Construct a request with explicit tenant/workspace scope.
    pub fn new_scoped(query: impl Into<String>, scope: RetrievalScope) -> Self {
        Self {
            query: query.into(),
            scope,
            top_k: DEFAULT_RETRIEVAL_TOP_K,
            min_score: None,
            context_budget: RetrievalBudget::default(),
            source_filters: SourceFilters::default(),
            required_index: None,
            cancellation: None,
            timeout: None,
        }
    }

    /// Override the maximum number of candidate/final chunks.
    pub fn with_top_k(mut self, top_k: usize) -> Self {
        self.top_k = top_k;
        self.context_budget.max_results = self.context_budget.max_results.min(top_k);
        self
    }

    /// Apply an inclusive normalized minimum relevance score in `[0, 1]`.
    pub fn with_min_score(mut self, min_score: f32) -> Self {
        self.min_score = Some(min_score);
        self
    }

    /// Override the explicit context budget.
    pub fn with_context_budget(mut self, budget: RetrievalBudget) -> Self {
        self.context_budget = budget;
        self
    }

    /// Apply source restrictions.
    pub fn with_source_filters(mut self, filters: SourceFilters) -> Self {
        self.source_filters = filters;
        self
    }

    /// Require a particular index/embedder compatibility tuple.
    pub fn requiring_index(mut self, requirement: IndexCompatibilityRequirement) -> Self {
        self.required_index = Some(requirement);
        self
    }

    /// Attach caller-provided cancellation.
    pub fn with_cancellation(mut self, cancellation: CancellationToken) -> Self {
        self.cancellation = Some(cancellation);
        self
    }

    /// Apply an overall retrieval timeout.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }
}

/// Chunk content and provenance produced by a [`ChunkResolver`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedChunk {
    /// Stable identifier used by the vector store.
    pub chunk_id: u64,
    /// Optional stable document identifier.
    #[serde(default)]
    pub document_id: Option<String>,
    /// Raw chunk text.
    pub content: String,
    /// Optional source identifier, such as a file token or source URI.
    pub source_id: Option<String>,
    /// Optional conversation relationship for conversation-derived chunks.
    pub conversation_id: Option<i64>,
    /// Optional message relationship for conversation-derived chunks.
    pub message_id: Option<i64>,
    /// Optional zero-based position within the source.
    pub ordinal: Option<u32>,
    /// Explicit tenant/workspace authorization scope attached by the resolver.
    /// Missing legacy serialized scope becomes invalid/empty and is rejected.
    #[serde(default)]
    pub scope: RetrievalScope,
    /// Optional opaque authorization marker associated with this record.
    #[serde(default)]
    pub authorization_marker: Option<String>,
    /// Index/embedder provenance when known to the resolver.
    #[serde(default)]
    pub index_metadata: Option<IndexMetadata>,
}

/// One normalized, scoped and provenance-preserving retrieval result.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RetrievedChunk {
    /// Stable chunk identifier.
    pub chunk_id: u64,
    /// Optional stable document identifier.
    #[serde(default)]
    pub document_id: Option<String>,
    /// Normalized relevance in `[0, 1]`; larger is better.
    pub score: f32,
    /// Retrieved text payload.
    pub content: String,
    /// Optional source identifier.
    pub source_id: Option<String>,
    /// Optional conversation relationship.
    pub conversation_id: Option<i64>,
    /// Optional message relationship.
    pub message_id: Option<i64>,
    /// Optional zero-based source position.
    pub ordinal: Option<u32>,
    /// Validated tenant/workspace authorization scope. Missing legacy
    /// serialized scope becomes invalid/empty and is rejected before use.
    #[serde(default)]
    pub scope: RetrievalScope,
    /// Optional opaque authorization marker.
    #[serde(default)]
    pub authorization_marker: Option<String>,
    /// Index/embedder provenance.
    #[serde(default)]
    pub index_metadata: Option<IndexMetadata>,
    /// True when the explicit chunk/aggregate byte policy truncated this item.
    #[serde(default)]
    pub truncated: bool,
}

/// Safe counters describing normalization and budget decisions.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetrievalDiagnostics {
    /// Backend candidates observed before validation.
    pub candidate_count: usize,
    /// Results rejected for tenant/workspace/actor/authorization mismatch.
    pub scope_mismatch_rejections: usize,
    /// Results rejected by explicit source filters.
    pub source_filter_rejections: usize,
    /// Results removed as duplicate ids or duplicate full-content hashes.
    pub deduplicated_count: usize,
    /// Results discarded because relevance was not finite.
    pub non_finite_score_rejections: usize,
    /// Results rejected because required index/embedder provenance was missing
    /// or incompatible.
    pub incompatible_index_rejections: usize,
    /// Results skipped because the aggregate context budget was exhausted.
    pub budget_skipped_count: usize,
    /// Included results truncated according to explicit budget policy.
    pub truncated_count: usize,
    /// Results skipped by deterministic per-document diversity policy.
    pub diversity_skipped_count: usize,
}

/// Why RAG is degraded while still potentially returning safe results.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RetrievalDegradedReason {
    /// Compatibility adapter supplied unscoped legacy strings.
    LegacyUnscopedAdapter,
    /// One or more backend records were rejected for scope mismatch.
    ScopeMismatchRejected,
    /// Some candidates were dropped by safe validation/budget policy.
    PartialValidation,
}

/// Why RAG cannot currently provide retrieval.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RetrievalUnavailableReason {
    /// No retriever implementation/service is currently available.
    RetrieverUnavailable,
    /// No usable index is currently available.
    IndexUnavailable,
    /// No embedding backend is currently available.
    EmbedderUnavailable,
    /// The retrieval backend failed.
    BackendError,
    /// The request scope is invalid.
    ScopeInvalid,
    /// Retrieval was cancelled by the caller.
    Cancelled,
    /// Retrieval exceeded its explicit timeout.
    Timeout,
}

/// Truthful state of one retrieval attempt.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RetrievalStatus {
    /// Retrieval completed and yielded one or more validated results.
    Ready,
    /// Retrieval completed successfully with no matches.
    Empty,
    /// Retrieval completed in a safe degraded mode.
    Degraded(RetrievalDegradedReason),
    /// Index/embedder compatibility requires a rebuild before retrieval.
    Rebuilding,
    /// Retrieval is unavailable for the stated reason.
    Unavailable(RetrievalUnavailableReason),
}

/// Compatibility state between the live embedder and persisted index.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum IndexCompatibilityState {
    /// Persisted index matches the live embedder and scope metadata contract.
    Compatible,
    /// Embedding metadata matches, but existing vectors lack the explicit scope
    /// metadata required for safe scoped retrieval.
    ScopeMetadataIncomplete,
    /// No persisted/headered index is available.
    Missing,
    /// A rebuild is required before vectors can be mixed safely.
    RebuildRequired,
    /// Capability could not be verified, typically through a legacy adapter.
    Unknown,
}

/// RAG capability snapshot at the core boundary.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RagCapabilitySnapshot {
    /// RAG code is compiled into this core.
    pub code_present: bool,
    /// A usable/indexed store is available.
    pub index_available: bool,
    /// An embedding backend is available.
    pub embedding_backend_available: bool,
    /// Compatibility between the active embedder and index.
    pub index_compatibility: IndexCompatibilityState,
    /// Current retrieval readiness.
    pub retrieval_state: RetrievalStatus,
}

impl RagCapabilitySnapshot {
    /// Conservative capability for a legacy adapter whose internals cannot be
    /// verified by the structured core interface.
    pub fn legacy_degraded() -> Self {
        Self {
            code_present: true,
            index_available: false,
            embedding_backend_available: false,
            index_compatibility: IndexCompatibilityState::Unknown,
            retrieval_state: RetrievalStatus::Degraded(
                RetrievalDegradedReason::LegacyUnscopedAdapter,
            ),
        }
    }
}

/// Structured result of one retrieval attempt.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RetrievalResponse {
    /// Validated, normalized and budgeted results.
    pub results: Vec<RetrievedChunk>,
    /// Truthful attempt state.
    pub status: RetrievalStatus,
    /// Capability snapshot used for this attempt.
    pub capability: RagCapabilitySnapshot,
    /// Safe internal counters; contains no query or chunk text.
    pub diagnostics: RetrievalDiagnostics,
}

impl RetrievalResponse {
    /// Construct a successful empty response with a supplied capability.
    pub fn empty(capability: RagCapabilitySnapshot) -> Self {
        Self {
            results: Vec::new(),
            status: RetrievalStatus::Empty,
            capability,
            diagnostics: RetrievalDiagnostics::default(),
        }
    }
}

/// Resolves scope-filtered vector-store chunk identifiers into content and
/// metadata. Implementations should apply the request scope in their own query
/// as the second defense layer; retriever validation still rejects mismatches.
#[async_trait::async_trait]
pub trait ChunkResolver: Send + Sync {
    /// Resolve requested ids under the explicit retrieval request.
    async fn resolve_chunks(
        &self,
        request: &RetrievalRequest,
        ids: &[u64],
    ) -> Result<Vec<ResolvedChunk>>;
}

/// Object-safe structured retriever interface consumed by context backends.
#[async_trait::async_trait]
pub trait StructuredRetriever: Send + Sync {
    /// Retrieve structured results with explicit scope/status/provenance.
    async fn retrieve_structured(
        &self,
        request: &RetrievalRequest,
    ) -> RetrieverResult<RetrievalResponse>;

    /// Return current capability without performing a query.
    fn capability_snapshot(&self) -> RagCapabilitySnapshot;
}

/// Errors produced by the core retrieval pipeline.
#[derive(Clone, Debug, thiserror::Error)]
pub enum RetrieverError {
    /// No retriever implementation/service is currently available.
    #[error("RAG retriever unavailable")]
    Unavailable,
    /// No embedding backend is currently available.
    #[error("RAG embedding backend unavailable")]
    EmbedderUnavailable,
    /// Retrieval requires non-empty query text when `top_k` is non-zero.
    #[error("RAG retrieval query must not be empty")]
    EmptyQuery,
    /// Score thresholds must be finite so filtering remains deterministic.
    #[error("RAG minimum relevance score must be finite")]
    NonFiniteMinimumScore,
    /// Normalized score thresholds must stay inside `[0, 1]`.
    #[error("RAG minimum relevance score must be within [0, 1]")]
    MinimumScoreOutOfRange,
    /// Tenant/workspace scope is missing or invalid.
    #[error("RAG retrieval scope is invalid")]
    ScopeInvalid,
    /// No usable index is available.
    #[error("RAG index unavailable")]
    IndexUnavailable,
    /// Persisted vectors are incompatible with the active embedding space.
    #[error("RAG index is incompatible with the active embedder; rebuild required")]
    IncompatibleIndex,
    /// Retrieval was cancelled by the caller.
    #[error("RAG retrieval cancelled")]
    Cancelled,
    /// Retrieval exceeded its explicit timeout.
    #[error("RAG retrieval timed out")]
    Timeout,
    /// An injected embedder or resolver failed. The user-visible error string
    /// deliberately omits dependency payloads that may contain private paths.
    #[error("RAG retrieval dependency failed")]
    Dependency(#[from] MukeiError),
}

/// Result alias for core retrieval operations.
pub type RetrieverResult<T> = std::result::Result<T, RetrieverError>;

/// First-class RAG service that embeds, scope-filters, resolves, validates,
/// normalizes, deduplicates, budgets and preserves provenance.
pub struct Retriever {
    embedder: Arc<dyn Embedder>,
    vector_store: Arc<VectorStore>,
    resolver: Arc<dyn ChunkResolver>,
}

impl Retriever {
    /// Construct a retriever from injected RAG-domain dependencies.
    pub fn new(
        embedder: Arc<dyn Embedder>,
        vector_store: Arc<VectorStore>,
        resolver: Arc<dyn ChunkResolver>,
    ) -> Self {
        Self {
            embedder,
            vector_store,
            resolver,
        }
    }

    fn expected_header(&self) -> StoreHeader {
        StoreHeader {
            format_version: STORE_FORMAT_VERSION,
            embedder_id: self.embedder.embedder_id().to_owned(),
            embedding_dim: self.embedder.dim() as u32,
        }
    }

    /// Return a truthful capability snapshot based on actual index/embedder
    /// compatibility rather than mere type presence.
    pub fn capability_snapshot(&self) -> RagCapabilitySnapshot {
        let expected = self.expected_header();
        match self.vector_store.needs_rebuild(&expected) {
            RebuildVerdict::Compatible => {
                let total = self.vector_store.count();
                let scoped = self.vector_store.scoped_count();
                if total > 0 && scoped == 0 {
                    RagCapabilitySnapshot {
                        code_present: true,
                        index_available: true,
                        embedding_backend_available: true,
                        index_compatibility: IndexCompatibilityState::ScopeMetadataIncomplete,
                        retrieval_state: RetrievalStatus::Rebuilding,
                    }
                } else if scoped < total {
                    RagCapabilitySnapshot {
                        code_present: true,
                        index_available: true,
                        embedding_backend_available: true,
                        index_compatibility: IndexCompatibilityState::ScopeMetadataIncomplete,
                        retrieval_state: RetrievalStatus::Degraded(
                            RetrievalDegradedReason::PartialValidation,
                        ),
                    }
                } else {
                    RagCapabilitySnapshot {
                        code_present: true,
                        index_available: true,
                        embedding_backend_available: true,
                        index_compatibility: IndexCompatibilityState::Compatible,
                        retrieval_state: RetrievalStatus::Ready,
                    }
                }
            }
            RebuildVerdict::NoHeader => RagCapabilitySnapshot {
                code_present: true,
                index_available: false,
                embedding_backend_available: true,
                index_compatibility: IndexCompatibilityState::Missing,
                retrieval_state: RetrievalStatus::Unavailable(
                    RetrievalUnavailableReason::IndexUnavailable,
                ),
            },
            RebuildVerdict::FormatMismatch { .. }
            | RebuildVerdict::EmbedderMismatch { .. }
            | RebuildVerdict::DimensionMismatch { .. } => RagCapabilitySnapshot {
                code_present: true,
                index_available: true,
                embedding_backend_available: true,
                index_compatibility: IndexCompatibilityState::RebuildRequired,
                retrieval_state: RetrievalStatus::Rebuilding,
            },
        }
    }

    /// Retrieve a compatibility vector of chunks. New callers should prefer
    /// [`Self::retrieve_structured`] so status/capability/diagnostics are not
    /// discarded.
    pub async fn retrieve(
        &self,
        request: &RetrievalRequest,
    ) -> RetrieverResult<Vec<RetrievedChunk>> {
        Ok(self.retrieve_structured(request).await?.results)
    }

    /// Retrieve structured, scope-safe and budgeted results.
    pub async fn retrieve_structured(
        &self,
        request: &RetrievalRequest,
    ) -> RetrieverResult<RetrievalResponse> {
        request.scope.validate()?;
        if request.top_k == 0 || request.context_budget.max_results == 0 {
            return Ok(RetrievalResponse::empty(self.capability_snapshot()));
        }

        let query = request.query.trim();
        if query.is_empty() {
            return Err(RetrieverError::EmptyQuery);
        }
        if request.min_score.is_some_and(|score| !score.is_finite()) {
            return Err(RetrieverError::NonFiniteMinimumScore);
        }
        if request
            .min_score
            .is_some_and(|score| !(0.0..=1.0).contains(&score))
        {
            return Err(RetrieverError::MinimumScoreOutOfRange);
        }

        let capability = self.capability_snapshot();
        match &capability.retrieval_state {
            RetrievalStatus::Rebuilding => return Err(RetrieverError::IncompatibleIndex),
            RetrievalStatus::Unavailable(RetrievalUnavailableReason::IndexUnavailable) => {
                return Err(RetrieverError::IndexUnavailable)
            }
            _ => {}
        }

        let expected = self.expected_header();
        if let Some(required) = request.required_index.as_ref() {
            let header = self
                .vector_store
                .header()
                .ok_or(RetrieverError::IndexUnavailable)?;
            if header.format_version != required.format_version
                || header.embedder_id != required.embedder_id
                || header.embedding_dim != required.embedding_dim
            {
                return Err(RetrieverError::IncompatibleIndex);
            }
        }

        let deadline = request.timeout.map(|timeout| Instant::now() + timeout);
        let query_embedding = run_controlled(
            self.embedder.embed(query),
            request.cancellation.as_ref(),
            deadline,
        )
        .await?
        .map_err(classify_dependency_error)?;

        let candidate_limit = request
            .top_k
            .max(request.context_budget.max_results)
            .min(self.vector_store.count());
        let mut matches =
            self.vector_store
                .search_scoped(&query_embedding.0, &request.scope, candidate_limit);

        matches.retain(|(_, raw_score)| raw_score.is_finite());
        let mut normalized_matches: Vec<(u64, f32)> = matches
            .into_iter()
            .map(|(chunk_id, raw_score)| (chunk_id, normalize_cosine_score(raw_score)))
            .filter(|(_, score)| request.min_score.is_none_or(|minimum| *score >= minimum))
            .collect();
        normalized_matches.sort_by(|(left_id, left_score), (right_id, right_score)| {
            right_score
                .total_cmp(left_score)
                .then_with(|| left_id.cmp(right_id))
        });

        let mut seen_ids = BTreeSet::new();
        normalized_matches.retain(|(chunk_id, _)| seen_ids.insert(*chunk_id));
        normalized_matches.truncate(request.top_k);

        if normalized_matches.is_empty() {
            return Ok(RetrievalResponse::empty(capability));
        }

        let ids: Vec<u64> = normalized_matches
            .iter()
            .map(|(chunk_id, _)| *chunk_id)
            .collect();
        let resolved = run_controlled(
            self.resolver.resolve_chunks(request, &ids),
            request.cancellation.as_ref(),
            deadline,
        )
        .await?
        .map_err(classify_dependency_error)?;

        let mut diagnostics = RetrievalDiagnostics {
            candidate_count: resolved.len(),
            ..RetrievalDiagnostics::default()
        };
        let mut resolved_by_id = BTreeMap::new();

        for candidate in resolved {
            if !seen_ids.contains(&candidate.chunk_id) {
                continue;
            }
            if candidate.scope != request.scope {
                diagnostics.scope_mismatch_rejections += 1;
                continue;
            }
            if !request.source_filters.allows(&candidate) {
                diagnostics.source_filter_rejections += 1;
                continue;
            }
            if let Some(metadata) = candidate.index_metadata.as_ref() {
                if metadata.format_version != expected.format_version
                    || metadata.embedder_id != expected.embedder_id
                    || metadata.embedding_dim != expected.embedding_dim
                {
                    diagnostics.incompatible_index_rejections += 1;
                    continue;
                }
            }

            match resolved_by_id.entry(candidate.chunk_id) {
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(candidate);
                }
                std::collections::btree_map::Entry::Occupied(mut entry) => {
                    diagnostics.deduplicated_count += 1;
                    if prefer_resolved_candidate(&candidate, entry.get()) {
                        entry.insert(candidate);
                    }
                }
            }
        }

        let store_header = self.vector_store.header();
        let index_metadata = store_header.as_ref().map(IndexMetadata::from);
        let mut results = Vec::with_capacity(normalized_matches.len());
        for (chunk_id, score) in normalized_matches {
            let Some(chunk) = resolved_by_id.remove(&chunk_id) else {
                continue;
            };
            results.push(RetrievedChunk {
                chunk_id,
                document_id: chunk.document_id,
                score,
                content: chunk.content,
                source_id: chunk.source_id,
                conversation_id: chunk.conversation_id,
                message_id: chunk.message_id,
                ordinal: chunk.ordinal,
                scope: chunk.scope,
                authorization_marker: chunk.authorization_marker,
                index_metadata: chunk.index_metadata.or_else(|| index_metadata.clone()),
                truncated: false,
            });
        }

        let (results, normalization) = normalize_and_budget_results(request, results);
        merge_diagnostics(&mut diagnostics, normalization);
        let status = if diagnostics.scope_mismatch_rejections > 0 {
            RetrievalStatus::Degraded(RetrievalDegradedReason::ScopeMismatchRejected)
        } else if diagnostics.source_filter_rejections > 0
            || diagnostics.deduplicated_count > 0
            || diagnostics.non_finite_score_rejections > 0
            || diagnostics.incompatible_index_rejections > 0
            || matches!(&capability.retrieval_state, RetrievalStatus::Degraded(_))
        {
            RetrievalStatus::Degraded(RetrievalDegradedReason::PartialValidation)
        } else if results.is_empty() {
            RetrievalStatus::Empty
        } else {
            RetrievalStatus::Ready
        };

        Ok(RetrievalResponse {
            results,
            status,
            capability,
            diagnostics,
        })
    }
}

#[async_trait::async_trait]
impl StructuredRetriever for Retriever {
    async fn retrieve_structured(
        &self,
        request: &RetrievalRequest,
    ) -> RetrieverResult<RetrievalResponse> {
        Retriever::retrieve_structured(self, request).await
    }

    fn capability_snapshot(&self) -> RagCapabilitySnapshot {
        Retriever::capability_snapshot(self)
    }
}

/// Re-validate, normalize, deduplicate, diversify and budget arbitrary
/// structured backend results. Context assembly calls this again so a custom
/// backend cannot bypass the core scope/budget invariants.
pub fn normalize_and_budget_results(
    request: &RetrievalRequest,
    mut results: Vec<RetrievedChunk>,
) -> (Vec<RetrievedChunk>, RetrievalDiagnostics) {
    let mut diagnostics = RetrievalDiagnostics {
        candidate_count: results.len(),
        ..RetrievalDiagnostics::default()
    };

    results.retain_mut(|result| {
        if result.scope != request.scope {
            diagnostics.scope_mismatch_rejections += 1;
            return false;
        }
        if !request.source_filters.allows_retrieved(result) {
            diagnostics.source_filter_rejections += 1;
            return false;
        }
        if let Some(marker) = request.scope.authorization_marker.as_ref() {
            if result.authorization_marker.as_ref() != Some(marker) {
                diagnostics.scope_mismatch_rejections += 1;
                return false;
            }
        }
        if let Some(required) = request.required_index.as_ref() {
            let compatible = result.index_metadata.as_ref().is_some_and(|metadata| {
                metadata.format_version == required.format_version
                    && metadata.embedder_id == required.embedder_id
                    && metadata.embedding_dim == required.embedding_dim
            });
            if !compatible {
                diagnostics.incompatible_index_rejections += 1;
                return false;
            }
        }
        if !result.score.is_finite() {
            diagnostics.non_finite_score_rejections += 1;
            return false;
        }
        result.score = result.score.clamp(0.0, 1.0);
        true
    });

    results.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| left.document_id.cmp(&right.document_id))
            .then_with(|| left.source_id.cmp(&right.source_id))
            .then_with(|| left.ordinal.cmp(&right.ordinal))
            .then_with(|| left.chunk_id.cmp(&right.chunk_id))
    });

    let mut seen_ids = BTreeSet::new();
    let mut seen_content = BTreeSet::new();
    results.retain(|result| {
        if !seen_ids.insert(result.chunk_id) {
            diagnostics.deduplicated_count += 1;
            return false;
        }
        let normalized = normalize_content_for_hash(&result.content);
        if normalized.len() >= CONTENT_HASH_DEDUPE_MIN_BYTES {
            let hash = blake3::hash(normalized.as_bytes());
            if !seen_content.insert(*hash.as_bytes()) {
                diagnostics.deduplicated_count += 1;
                return false;
            }
        }
        true
    });

    if let Some(per_document_cap) = request.context_budget.per_document_cap {
        if per_document_cap == 0 {
            diagnostics.diversity_skipped_count += results.len();
            results.clear();
        } else {
            let mut counts: BTreeMap<String, usize> = BTreeMap::new();
            results.retain(|result| {
                let identity = document_identity(result);
                let count = counts.entry(identity).or_default();
                if *count >= per_document_cap {
                    diagnostics.diversity_skipped_count += 1;
                    false
                } else {
                    *count += 1;
                    true
                }
            });
        }
    }

    let result_cap = request.top_k.min(request.context_budget.max_results);
    if results.len() > result_cap {
        diagnostics.budget_skipped_count += results.len() - result_cap;
        results.truncate(result_cap);
    }

    let mut admitted = Vec::with_capacity(results.len());
    let mut total_bytes = 0usize;
    for mut result in results {
        if total_bytes >= request.context_budget.max_total_bytes {
            diagnostics.budget_skipped_count += 1;
            continue;
        }

        let remaining = request
            .context_budget
            .max_total_bytes
            .saturating_sub(total_bytes);
        let cap = request.context_budget.max_chunk_bytes.min(remaining);
        if cap == 0 {
            diagnostics.budget_skipped_count += 1;
            continue;
        }

        if result.content.len() > cap {
            result.content = truncate_to_byte_boundary(&result.content, cap).to_owned();
            result.truncated = true;
            diagnostics.truncated_count += 1;
        }
        if result.content.is_empty() {
            diagnostics.budget_skipped_count += 1;
            continue;
        }

        total_bytes = total_bytes.saturating_add(result.content.len());
        admitted.push(result);
    }

    (admitted, diagnostics)
}

fn normalize_cosine_score(raw_score: f32) -> f32 {
    ((raw_score.clamp(-1.0, 1.0) + 1.0) * 0.5).clamp(0.0, 1.0)
}

fn normalize_content_for_hash(content: &str) -> String {
    content.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn document_identity(result: &RetrievedChunk) -> String {
    result
        .document_id
        .as_ref()
        .map(|value| format!("document:{value}"))
        .or_else(|| {
            result
                .source_id
                .as_ref()
                .map(|value| format!("source:{value}"))
        })
        .unwrap_or_else(|| format!("chunk:{}", result.chunk_id))
}

fn truncate_to_byte_boundary(value: &str, cap: usize) -> &str {
    if value.len() <= cap {
        return value;
    }
    let mut end = cap;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}

fn prefer_resolved_candidate(candidate: &ResolvedChunk, existing: &ResolvedChunk) -> bool {
    let candidate_completeness = metadata_completeness(candidate);
    let existing_completeness = metadata_completeness(existing);

    if candidate_completeness != existing_completeness {
        return candidate_completeness > existing_completeness;
    }

    (
        candidate.document_id.as_deref(),
        candidate.source_id.as_deref(),
        candidate.conversation_id,
        candidate.message_id,
        candidate.ordinal,
        candidate.content.as_str(),
    ) < (
        existing.document_id.as_deref(),
        existing.source_id.as_deref(),
        existing.conversation_id,
        existing.message_id,
        existing.ordinal,
        existing.content.as_str(),
    )
}

fn metadata_completeness(chunk: &ResolvedChunk) -> u8 {
    u8::from(chunk.document_id.is_some())
        + u8::from(chunk.source_id.is_some())
        + u8::from(chunk.conversation_id.is_some())
        + u8::from(chunk.message_id.is_some())
        + u8::from(chunk.ordinal.is_some())
        + u8::from(chunk.authorization_marker.is_some())
        + u8::from(chunk.index_metadata.is_some())
}

fn classify_dependency_error(error: MukeiError) -> RetrieverError {
    match error {
        MukeiError::Cancelled => RetrieverError::Cancelled,
        MukeiError::ToolTimeout(_) | MukeiError::NetworkTimeout { .. } => RetrieverError::Timeout,
        other => RetrieverError::Dependency(other),
    }
}

fn merge_diagnostics(target: &mut RetrievalDiagnostics, extra: RetrievalDiagnostics) {
    target.scope_mismatch_rejections += extra.scope_mismatch_rejections;
    target.source_filter_rejections += extra.source_filter_rejections;
    target.deduplicated_count += extra.deduplicated_count;
    target.non_finite_score_rejections += extra.non_finite_score_rejections;
    target.incompatible_index_rejections += extra.incompatible_index_rejections;
    target.budget_skipped_count += extra.budget_skipped_count;
    target.truncated_count += extra.truncated_count;
    target.diversity_skipped_count += extra.diversity_skipped_count;
}

async fn run_controlled<F, T>(
    future: F,
    cancellation: Option<&CancellationToken>,
    deadline: Option<Instant>,
) -> RetrieverResult<T>
where
    F: Future<Output = T>,
{
    tokio::pin!(future);
    match (cancellation, deadline) {
        (Some(cancel), Some(deadline)) => {
            tokio::select! {
                _ = cancel.cancelled() => Err(RetrieverError::Cancelled),
                _ = tokio::time::sleep_until(deadline) => Err(RetrieverError::Timeout),
                output = &mut future => Ok(output),
            }
        }
        (Some(cancel), None) => {
            tokio::select! {
                _ = cancel.cancelled() => Err(RetrieverError::Cancelled),
                output = &mut future => Ok(output),
            }
        }
        (None, Some(deadline)) => {
            tokio::select! {
                _ = tokio::time::sleep_until(deadline) => Err(RetrieverError::Timeout),
                output = &mut future => Ok(output),
            }
        }
        (None, None) => Ok(future.await),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rag::embedder::MockEmbedder;
    use tempfile::tempdir;

    fn scope(tenant: &str, workspace: &str) -> RetrievalScope {
        RetrievalScope::new(tenant, workspace)
    }

    fn result(
        chunk_id: u64,
        score: f32,
        content: &str,
        document: &str,
        scope: RetrievalScope,
    ) -> RetrievedChunk {
        RetrievedChunk {
            chunk_id,
            document_id: Some(document.to_owned()),
            score,
            content: content.to_owned(),
            source_id: Some(format!("source-{document}")),
            conversation_id: None,
            message_id: None,
            ordinal: Some(chunk_id as u32),
            scope,
            authorization_marker: None,
            index_metadata: None,
            truncated: false,
        }
    }

    #[test]
    fn sol05_scope_mismatch_is_rejected_before_context_budgeting() {
        let request = RetrievalRequest::new_scoped("q", scope("tenant-a", "workspace-a"));
        let (kept, diagnostics) = normalize_and_budget_results(
            &request,
            vec![result(
                1,
                0.9,
                "private content from tenant b that must never cross scope",
                "doc-b",
                scope("tenant-b", "workspace-a"),
            )],
        );
        assert!(kept.is_empty());
        assert_eq!(diagnostics.scope_mismatch_rejections, 1);
    }

    #[test]
    fn sol05_normalization_is_deterministic_and_deduplicates_id_and_content() {
        let request = RetrievalRequest::new_scoped("q", scope("t", "w"));
        let duplicate_text = "same sufficiently long normalized content payload for dedupe";
        let inputs = vec![
            result(2, 0.8, duplicate_text, "doc-b", scope("t", "w")),
            result(1, 0.9, "highest", "doc-a", scope("t", "w")),
            result(2, 0.7, "duplicate id", "doc-c", scope("t", "w")),
            result(3, 0.8, duplicate_text, "doc-d", scope("t", "w")),
        ];
        let (kept, diagnostics) = normalize_and_budget_results(&request, inputs);
        assert_eq!(
            kept.iter().map(|item| item.chunk_id).collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert_eq!(diagnostics.deduplicated_count, 2);
    }

    #[test]
    fn sol05_budget_enforces_total_chunk_and_result_caps_deterministically() {
        let budget = RetrievalBudget {
            max_results: 2,
            max_total_bytes: 12,
            max_chunk_bytes: 8,
            reserved_context_tokens: 777,
            per_document_cap: None,
        };
        let request = RetrievalRequest::new_scoped("q", scope("t", "w"))
            .with_top_k(5)
            .with_context_budget(budget.clone());
        let (kept, diagnostics) = normalize_and_budget_results(
            &request,
            vec![
                result(1, 0.9, "abcdefghijk", "a", scope("t", "w")),
                result(2, 0.8, "12345678", "b", scope("t", "w")),
                result(3, 0.7, "zzzz", "c", scope("t", "w")),
            ],
        );
        assert_eq!(request.context_budget.reserved_context_tokens, 777);
        assert_eq!(kept.len(), 2);
        assert_eq!(kept[0].content, "abcdefgh");
        assert_eq!(kept[1].content, "1234");
        assert!(kept[0].truncated && kept[1].truncated);
        assert_eq!(
            kept.iter().map(|item| item.content.len()).sum::<usize>(),
            12
        );
        assert!(diagnostics.truncated_count >= 2);
    }

    #[test]
    fn sol05_source_diversity_cap_is_deterministic() {
        let budget = RetrievalBudget {
            per_document_cap: Some(1),
            ..RetrievalBudget::default()
        };
        let request =
            RetrievalRequest::new_scoped("q", scope("t", "w")).with_context_budget(budget);
        let (kept, diagnostics) = normalize_and_budget_results(
            &request,
            vec![
                result(1, 0.99, "a1", "a", scope("t", "w")),
                result(2, 0.98, "a2", "a", scope("t", "w")),
                result(3, 0.97, "b1", "b", scope("t", "w")),
            ],
        );
        assert_eq!(
            kept.iter().map(|item| item.chunk_id).collect::<Vec<_>>(),
            vec![1, 3]
        );
        assert_eq!(diagnostics.diversity_skipped_count, 1);
    }

    #[test]
    fn sol05_vector_store_scope_filter_blocks_cross_tenant_and_workspace_hits() {
        let dir = tempdir().unwrap();
        let store = VectorStore::open(dir.path().join("scope.json"));
        store.set_header(StoreHeader {
            format_version: STORE_FORMAT_VERSION,
            embedder_id: "mock".into(),
            embedding_dim: 2,
        });
        store.add_scoped(1, vec![1.0, 0.0], "a".into(), scope("ta", "wa"));
        store.add_scoped(2, vec![1.0, 0.0], "b".into(), scope("tb", "wa"));
        store.add_scoped(3, vec![1.0, 0.0], "c".into(), scope("ta", "wb"));
        let hits = store.search_scoped(&[1.0, 0.0], &scope("ta", "wa"), 10);
        assert_eq!(hits.iter().map(|(id, _)| *id).collect::<Vec<_>>(), vec![1]);
    }

    struct EmptyResolver;
    #[async_trait::async_trait]
    impl ChunkResolver for EmptyResolver {
        async fn resolve_chunks(
            &self,
            _request: &RetrievalRequest,
            _ids: &[u64],
        ) -> Result<Vec<ResolvedChunk>> {
            Ok(Vec::new())
        }
    }

    #[tokio::test]
    async fn sol05_unavailable_index_is_explicit_and_no_matches_is_successful_empty() {
        let dir = tempdir().unwrap();
        let embedder = Arc::new(MockEmbedder { dim: 8 });
        let store = Arc::new(VectorStore::open(dir.path().join("empty.json")));
        let retriever = Retriever::new(embedder.clone(), store.clone(), Arc::new(EmptyResolver));
        let request = RetrievalRequest::new_scoped("query", scope("t", "w"));
        assert!(matches!(
            retriever.retrieve_structured(&request).await,
            Err(RetrieverError::IndexUnavailable)
        ));

        store.set_header(StoreHeader {
            format_version: STORE_FORMAT_VERSION,
            embedder_id: embedder.embedder_id().to_owned(),
            embedding_dim: embedder.dim() as u32,
        });
        let response = retriever.retrieve_structured(&request).await.unwrap();
        assert_eq!(response.status, RetrievalStatus::Empty);
        assert!(response.results.is_empty());
    }

    #[test]
    fn sol05_legacy_unscoped_vectors_are_not_reported_retrieval_ready() {
        let dir = tempdir().unwrap();
        let embedder = Arc::new(MockEmbedder { dim: 8 });
        let store = Arc::new(VectorStore::open(dir.path().join("legacy-unscoped.json")));
        store.set_header(StoreHeader {
            format_version: STORE_FORMAT_VERSION,
            embedder_id: embedder.embedder_id().to_owned(),
            embedding_dim: embedder.dim() as u32,
        });
        store.add(1, vec![0.0; 8], "legacy".into());
        let retriever = Retriever::new(embedder, store, Arc::new(EmptyResolver));
        let capability = retriever.capability_snapshot();
        assert_eq!(capability.retrieval_state, RetrievalStatus::Rebuilding);
        assert_eq!(
            capability.index_compatibility,
            IndexCompatibilityState::ScopeMetadataIncomplete
        );
    }

    #[tokio::test]
    async fn sol05_incompatible_index_reports_rebuild_required() {
        let dir = tempdir().unwrap();
        let embedder = Arc::new(MockEmbedder { dim: 8 });
        let store = Arc::new(VectorStore::open(dir.path().join("bad.json")));
        store.set_header(StoreHeader {
            format_version: STORE_FORMAT_VERSION,
            embedder_id: "different-embedder".into(),
            embedding_dim: 8,
        });
        let retriever = Retriever::new(embedder, store, Arc::new(EmptyResolver));
        assert_eq!(
            retriever.capability_snapshot().retrieval_state,
            RetrievalStatus::Rebuilding
        );
        assert!(matches!(
            retriever
                .retrieve_structured(&RetrievalRequest::new_scoped("q", scope("t", "w")))
                .await,
            Err(RetrieverError::IncompatibleIndex)
        ));
    }
}
