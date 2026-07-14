//! Model activation lifecycle and authoritative active-backend ownership.
//!
//! This module deliberately separates three facts that were previously easy
//! to conflate at the UI/runtime boundary:
//! - a model artifact exists,
//! - a model artifact has been verified,
//! - an inference backend for that exact artifact is active and ready.
//!
//! Activation is generation guarded. Expensive backend construction happens
//! outside the state lock; the final backend swap and readiness transition are
//! committed together only when the activation generation is still current.

use std::fmt;
use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use futures::FutureExt;
use parking_lot::RwLock;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::diagnostics::observability::{
    AttributeValue, EventScope, EventSeverity, FieldSensitivity, ObservabilityRecorder,
    OperationalEvent, OutcomeClass,
};
use crate::error::{MukeiError, Result};

use super::llama_wrapper::{
    BackendIdentity, BackendKind, BackendUnavailableReason, InferenceBackend, InferenceOutcome,
};

/// Stable activation failure taxonomy. These values are safe to expose as
/// machine-readable categories; the original error text is intentionally not
/// stored in activation state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActivationFailureCategory {
    BackendUnavailable,
    VerificationMismatch,
    ModelLoad,
    BackendRejected,
    Internal,
}

impl ActivationFailureCategory {
    pub const fn as_tag(self) -> &'static str {
        match self {
            Self::BackendUnavailable => "backend_unavailable",
            Self::VerificationMismatch => "verification_mismatch",
            Self::ModelLoad => "model_load",
            Self::BackendRejected => "backend_rejected",
            Self::Internal => "internal",
        }
    }
}

/// Path-bearing verified artifact wrapper. Its `Debug` implementation never
/// renders the local filesystem path, preventing accidental path disclosure in
/// diagnostics or user-facing snapshots.
#[derive(Clone)]
pub struct VerifiedModelArtifact {
    artifact_id: String,
    local_path: Arc<PathBuf>,
}

impl VerifiedModelArtifact {
    pub fn new(artifact_id: impl Into<String>, local_path: impl Into<PathBuf>) -> Result<Self> {
        let artifact_id = artifact_id.into();
        if artifact_id.trim().is_empty() {
            return Err(MukeiError::Invariant(
                "verified model artifact requires a stable artifact id".to_string(),
            ));
        }
        Ok(Self {
            artifact_id,
            local_path: Arc::new(local_path.into()),
        })
    }

    pub fn artifact_id(&self) -> &str {
        &self.artifact_id
    }

    /// Internal activation boundary accessor. Callers must not surface this
    /// path in UI state or telemetry.
    pub fn local_path(&self) -> &Path {
        self.local_path.as_path()
    }
}

impl fmt::Debug for VerifiedModelArtifact {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VerifiedModelArtifact")
            .field("artifact_id", &self.artifact_id)
            .field("local_path", &"<redacted>")
            .finish()
    }
}

/// Descriptor accepted by activation only after artifact verification has
/// completed successfully.
#[derive(Clone, Debug)]
pub struct VerifiedModelDescriptor {
    pub model_id: String,
    pub revision: String,
    pub artifact: VerifiedModelArtifact,
}

impl VerifiedModelDescriptor {
    pub fn new(
        model_id: impl Into<String>,
        revision: impl Into<String>,
        artifact: VerifiedModelArtifact,
    ) -> Result<Self> {
        let model_id = model_id.into();
        let revision = revision.into();
        if model_id.trim().is_empty() || revision.trim().is_empty() {
            return Err(MukeiError::Invariant(
                "verified model descriptor requires model id and revision".to_string(),
            ));
        }
        Ok(Self {
            model_id,
            revision,
            artifact,
        })
    }

    fn same_model(&self, model_id: &str, revision: &str) -> bool {
        self.model_id == model_id && self.revision == revision
    }
}

/// Authoritative model activation lifecycle.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModelActivationState {
    NoModelSelected,
    ModelMissing {
        model_id: String,
        revision: String,
        generation: u64,
    },
    ModelVerifying {
        model_id: String,
        revision: String,
        generation: u64,
    },
    ModelVerified {
        model_id: String,
        revision: String,
        artifact_id: String,
        generation: u64,
    },
    Activating {
        model_id: String,
        revision: String,
        artifact_id: String,
        operation_id: u64,
    },
    Ready {
        model_id: String,
        revision: String,
        artifact_id: String,
        operation_id: u64,
        backend_kind: BackendKind,
    },
    ActivationFailed {
        model_id: String,
        revision: String,
        artifact_id: String,
        operation_id: u64,
        category: ActivationFailureCategory,
    },
    Deactivating {
        model_id: String,
        revision: String,
        artifact_id: String,
        operation_id: u64,
    },
}

impl ModelActivationState {
    pub const fn is_activation_in_progress(&self) -> bool {
        matches!(
            self,
            Self::ModelVerifying { .. } | Self::Activating { .. } | Self::Deactivating { .. }
        )
    }

    pub const fn is_verified(&self) -> bool {
        matches!(
            self,
            Self::ModelVerified { .. }
                | Self::Activating { .. }
                | Self::Ready { .. }
                | Self::Deactivating { .. }
        )
    }
}

/// Truthful readiness projection for UI/bridge capability snapshots.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InferenceReadinessSnapshot {
    pub inference_interface_exists: bool,
    pub real_backend_implementation_available: bool,
    pub selected_model_exists: bool,
    pub selected_model_verified: bool,
    pub activation_in_progress: bool,
    pub active_backend_ready: bool,
    pub development_mock_active: bool,
    pub activation_failed: bool,
    pub product_ready: bool,
    pub state: ModelActivationState,
}

/// Authoritative identity of the backend currently serving inference turns.
/// Candidate verification state is intentionally excluded so a failed or
/// in-progress replacement cannot masquerade as the active model.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActiveModelSnapshot {
    pub model_id: String,
    pub revision: String,
    pub artifact_id: String,
    pub backend: BackendIdentity,
}

#[async_trait::async_trait]
pub trait InferenceBackendFactory: Send + Sync {
    /// Construct a backend for the exact verified descriptor. Implementations
    /// may perform expensive loading here. The activation service never holds
    /// its state lock across this await.
    async fn activate(
        &self,
        descriptor: &VerifiedModelDescriptor,
    ) -> Result<Arc<dyn InferenceBackend>>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActivationCommit {
    Ready,
    Failed(ActivationFailureCategory),
    StaleIgnored,
}

#[derive(Clone)]
struct ActiveBackend {
    model_id: String,
    revision: String,
    artifact_id: String,
    backend: Arc<dyn InferenceBackend>,
}

fn drop_retired_backend_safely(retired: Option<ActiveBackend>) {
    if std::panic::catch_unwind(AssertUnwindSafe(|| drop(retired))).is_err() {
        tracing::error!(
            panic_category = "backend_dispose",
            "retired inference backend panicked during disposal; panic contained"
        );
    }
}

struct ActivationInner {
    selected: Option<(String, String)>,
    verified: Option<VerifiedModelDescriptor>,
    state: ModelActivationState,
    active: Option<ActiveBackend>,
}

impl Default for ActivationInner {
    fn default() -> Self {
        Self {
            selected: None,
            verified: None,
            state: ModelActivationState::NoModelSelected,
            active: None,
        }
    }
}

/// Stable backend dependency injected into `AgentLoop`.
///
/// The service itself implements [`InferenceBackend`], so the loop owns one
/// stable dependency while activation can atomically replace the underlying
/// model backend. Candidate verification/activation may coexist with a healthy
/// active backend; only explicit deactivation removes it before replacement.
pub struct ModelActivationService {
    inner: RwLock<ActivationInner>,
    generation: AtomicU64,
    real_backend_implementation_available: AtomicBool,
    observability: Option<ObservabilityRecorder>,
}

impl ModelActivationService {
    pub fn new(real_backend_implementation_available: bool) -> Arc<Self> {
        Arc::new(Self {
            inner: RwLock::new(ActivationInner::default()),
            generation: AtomicU64::new(0),
            real_backend_implementation_available: AtomicBool::new(
                real_backend_implementation_available,
            ),
            observability: None,
        })
    }

    pub fn new_with_observability(
        real_backend_implementation_available: bool,
        observability: ObservabilityRecorder,
    ) -> Arc<Self> {
        Arc::new(Self {
            inner: RwLock::new(ActivationInner::default()),
            generation: AtomicU64::new(0),
            real_backend_implementation_available: AtomicBool::new(
                real_backend_implementation_available,
            ),
            observability: Some(observability),
        })
    }

    fn next_generation(&self) -> u64 {
        self.generation
            .fetch_add(1, Ordering::AcqRel)
            .saturating_add(1)
    }

    fn is_current_generation(&self, generation: u64) -> bool {
        self.generation.load(Ordering::Acquire) == generation
    }

    /// Update whether the process currently has a real backend factory registered.
    /// This is runtime truth, not a compile-time feature guess: a production build
    /// remains non-product-ready until the executable composition root installs a
    /// factory that can construct real backends.
    pub fn set_real_backend_implementation_available(&self, available: bool) {
        self.real_backend_implementation_available
            .store(available, Ordering::Release);
    }

    pub fn state(&self) -> ModelActivationState {
        self.inner.read().state.clone()
    }

    pub fn active_model_snapshot(&self) -> Option<ActiveModelSnapshot> {
        self.inner
            .read()
            .active
            .as_ref()
            .map(|active| ActiveModelSnapshot {
                model_id: active.model_id.clone(),
                revision: active.revision.clone(),
                artifact_id: active.artifact_id.clone(),
                backend: active.backend.identity(),
            })
    }

    /// Snapshot the currently selected candidate identity. This is distinct from
    /// the active backend: a replacement may be verifying while the previous
    /// backend continues serving turns.
    pub fn selected_model_snapshot(&self) -> Option<(String, String)> {
        self.inner.read().selected.clone()
    }

    pub fn mark_model_missing(
        &self,
        model_id: impl Into<String>,
        revision: impl Into<String>,
    ) -> u64 {
        let model_id = model_id.into();
        let revision = revision.into();
        let mut inner = self.inner.write();
        let generation = self.next_generation();
        inner.selected = Some((model_id.clone(), revision.clone()));
        inner.verified = None;
        inner.state = ModelActivationState::ModelMissing {
            model_id,
            revision,
            generation,
        };
        // A missing replacement candidate must not evict the currently active
        // backend. Active-backend lifetime changes only on successful swap or
        // explicit deactivation.
        generation
    }

    pub fn begin_verification(
        &self,
        model_id: impl Into<String>,
        revision: impl Into<String>,
    ) -> u64 {
        let model_id = model_id.into();
        let revision = revision.into();
        let mut inner = self.inner.write();
        let generation = self.next_generation();
        inner.selected = Some((model_id.clone(), revision.clone()));
        inner.verified = None;
        inner.state = ModelActivationState::ModelVerifying {
            model_id,
            revision,
            generation,
        };
        // Verification is a candidate-side transition. The active backend keeps
        // serving existing and new turns until a replacement commits successfully.
        generation
    }

    /// Commit a candidate-side verification failure only if it still belongs
    /// to the current verification generation and selected model. A failed
    /// replacement never retires the previously active backend.
    pub fn mark_verification_failed(
        &self,
        generation: u64,
        model_id: &str,
        revision: &str,
        artifact_id: &str,
        category: ActivationFailureCategory,
    ) -> bool {
        if !self.is_current_generation(generation) {
            return false;
        }
        let mut inner = self.inner.write();
        if self.generation.load(Ordering::Acquire) != generation {
            return false;
        }
        let selected_matches = inner
            .selected
            .as_ref()
            .is_some_and(|(id, selected_revision)| id == model_id && selected_revision == revision);
        let verifying_matches = matches!(
            &inner.state,
            ModelActivationState::ModelVerifying {
                model_id: state_model_id,
                revision: state_revision,
                generation: state_generation,
            } if state_model_id == model_id
                && state_revision == revision
                && *state_generation == generation
        );
        if !selected_matches || !verifying_matches {
            return false;
        }
        inner.verified = None;
        inner.state = ModelActivationState::ActivationFailed {
            model_id: model_id.to_string(),
            revision: revision.to_string(),
            artifact_id: artifact_id.to_string(),
            operation_id: generation,
            category,
        };
        // Candidate verification failure is isolated from the serving backend.
        true
    }

    /// Commit a verified descriptor only if it still belongs to the current
    /// verification generation and selected model.
    pub fn mark_verified(&self, generation: u64, descriptor: VerifiedModelDescriptor) -> bool {
        if !self.is_current_generation(generation) {
            return false;
        }
        let mut inner = self.inner.write();
        if self.generation.load(Ordering::Acquire) != generation {
            return false;
        }
        let selected_matches = inner
            .selected
            .as_ref()
            .is_some_and(|(id, revision)| descriptor.same_model(id, revision));
        if !selected_matches {
            return false;
        }
        inner.verified = Some(descriptor.clone());
        inner.state = ModelActivationState::ModelVerified {
            model_id: descriptor.model_id,
            revision: descriptor.revision,
            artifact_id: descriptor.artifact.artifact_id().to_string(),
            generation,
        };
        true
    }

    /// Activate the currently verified descriptor using an injected backend
    /// factory. Backend construction is outside the lock; final replacement
    /// and the `Ready` transition occur in one write-side critical section.
    pub async fn activate_verified(
        &self,
        factory: &dyn InferenceBackendFactory,
    ) -> ActivationCommit {
        let (descriptor, operation_id) = {
            let mut inner = self.inner.write();
            let Some(descriptor) = inner.verified.clone() else {
                return ActivationCommit::Failed(ActivationFailureCategory::VerificationMismatch);
            };
            let selected_matches = inner
                .selected
                .as_ref()
                .is_some_and(|(id, revision)| descriptor.same_model(id, revision));
            let verified_matches = inner.verified.as_ref().is_some_and(|verified| {
                verified.same_model(&descriptor.model_id, &descriptor.revision)
                    && verified.artifact.artifact_id() == descriptor.artifact.artifact_id()
            });
            if !selected_matches || !verified_matches {
                return ActivationCommit::StaleIgnored;
            }
            let operation_id = self.next_generation();
            inner.state = ModelActivationState::Activating {
                model_id: descriptor.model_id.clone(),
                revision: descriptor.revision.clone(),
                artifact_id: descriptor.artifact.artifact_id().to_string(),
                operation_id,
            };
            (descriptor, operation_id)
        };
        self.record_activation_event(
            "model.activation.start",
            OutcomeClass::Success,
            operation_id,
            None,
            None,
        );

        let activation_result = AssertUnwindSafe(factory.activate(&descriptor))
            .catch_unwind()
            .await;
        let backend = match activation_result {
            Ok(Ok(backend)) => backend,
            Ok(Err(error)) => {
                let category = classify_activation_error(&error);
                if !self.commit_activation_failure(operation_id, &descriptor, category) {
                    return ActivationCommit::StaleIgnored;
                }
                self.record_activation_event(
                    "model.activation.failure",
                    OutcomeClass::Unavailable,
                    operation_id,
                    Some(category),
                    None,
                );
                return ActivationCommit::Failed(category);
            }
            Err(_) => {
                let category = ActivationFailureCategory::Internal;
                if !self.commit_activation_failure(operation_id, &descriptor, category) {
                    return ActivationCommit::StaleIgnored;
                }
                self.record_activation_event(
                    "model.activation.failure",
                    OutcomeClass::InternalFailure,
                    operation_id,
                    Some(category),
                    None,
                );
                tracing::error!(
                    operation_id,
                    panic_category = "backend_activation",
                    "backend activation panic converted to explicit failure"
                );
                return ActivationCommit::Failed(category);
            }
        };

        let identity = backend.identity();
        if identity.kind == BackendKind::Unavailable {
            let category = ActivationFailureCategory::BackendUnavailable;
            if !self.commit_activation_failure(operation_id, &descriptor, category) {
                return ActivationCommit::StaleIgnored;
            }
            self.record_activation_event(
                "model.activation.failure",
                OutcomeClass::Unavailable,
                operation_id,
                Some(category),
                Some(identity.kind),
            );
            return ActivationCommit::Failed(category);
        }

        let retired = {
            let mut inner = self.inner.write();
            if self.generation.load(Ordering::Acquire) != operation_id {
                return ActivationCommit::StaleIgnored;
            }
            let still_selected = inner
                .selected
                .as_ref()
                .is_some_and(|(id, revision)| descriptor.same_model(id, revision));
            let still_verified = inner.verified.as_ref().is_some_and(|verified| {
                verified.same_model(&descriptor.model_id, &descriptor.revision)
                    && verified.artifact.artifact_id() == descriptor.artifact.artifact_id()
            });
            if !still_selected || !still_verified {
                return ActivationCommit::StaleIgnored;
            }

            let next = ActiveBackend {
                model_id: descriptor.model_id.clone(),
                revision: descriptor.revision.clone(),
                artifact_id: descriptor.artifact.artifact_id().to_string(),
                backend,
            };
            let retired = inner.active.replace(next);
            inner.state = ModelActivationState::Ready {
                model_id: descriptor.model_id.clone(),
                revision: descriptor.revision.clone(),
                artifact_id: descriptor.artifact.artifact_id().to_string(),
                operation_id,
                backend_kind: identity.kind,
            };
            retired
        };
        drop_retired_backend_safely(retired);

        self.record_activation_event(
            "model.activation.success",
            OutcomeClass::Success,
            operation_id,
            None,
            Some(identity.kind),
        );
        ActivationCommit::Ready
    }

    fn commit_activation_failure(
        &self,
        operation_id: u64,
        descriptor: &VerifiedModelDescriptor,
        category: ActivationFailureCategory,
    ) -> bool {
        let mut inner = self.inner.write();
        if self.generation.load(Ordering::Acquire) != operation_id {
            return false;
        }
        let still_selected = inner
            .selected
            .as_ref()
            .is_some_and(|(id, revision)| descriptor.same_model(id, revision));
        if !still_selected {
            return false;
        }
        inner.state = ModelActivationState::ActivationFailed {
            model_id: descriptor.model_id.clone(),
            revision: descriptor.revision.clone(),
            artifact_id: descriptor.artifact.artifact_id().to_string(),
            operation_id,
            category,
        };
        // Failure is candidate-local. Preserve the prior active backend so a bad
        // model switch cannot take down an otherwise healthy inference session.
        true
    }

    /// Deactivate the active backend. The transition first publishes
    /// `Deactivating`, then removes the backend, then returns to the verified
    /// state for the selected descriptor (or `NoModelSelected`).
    pub fn deactivate(&self) -> u64 {
        let (operation_id, selected, verified, retired) = {
            let mut inner = self.inner.write();
            let operation_id = self.next_generation();
            let selected = inner.selected.clone();
            let verified = inner.verified.clone();
            let active_identity = inner.active.as_ref().map(|active| {
                (
                    active.model_id.clone(),
                    active.revision.clone(),
                    active.artifact_id.clone(),
                )
            });
            inner.state = match active_identity {
                Some((model_id, revision, artifact_id)) => ModelActivationState::Deactivating {
                    model_id,
                    revision,
                    artifact_id,
                    operation_id,
                },
                None => ModelActivationState::NoModelSelected,
            };
            let retired = inner.active.take();
            (operation_id, selected, verified, retired)
        };
        drop_retired_backend_safely(retired);

        let mut inner = self.inner.write();
        if self.generation.load(Ordering::Acquire) != operation_id {
            return operation_id;
        }
        inner.state = match (selected, verified) {
            (Some((model_id, revision)), Some(descriptor))
                if descriptor.same_model(&model_id, &revision) =>
            {
                ModelActivationState::ModelVerified {
                    model_id,
                    revision,
                    artifact_id: descriptor.artifact.artifact_id().to_string(),
                    generation: operation_id,
                }
            }
            (Some((model_id, revision)), None) => ModelActivationState::ModelMissing {
                model_id,
                revision,
                generation: operation_id,
            },
            _ => ModelActivationState::NoModelSelected,
        };
        operation_id
    }

    pub fn readiness_snapshot(&self) -> InferenceReadinessSnapshot {
        let inner = self.inner.read();
        let active_identity = inner
            .active
            .as_ref()
            .map(|active| active.backend.identity());
        let active_backend_ready = active_identity
            .as_ref()
            .is_some_and(|identity| identity.kind != BackendKind::Unavailable);
        let development_mock_active = active_backend_ready
            && active_identity
                .as_ref()
                .is_some_and(|identity| identity.kind == BackendKind::DevelopmentMock);
        let product_ready = self
            .real_backend_implementation_available
            .load(Ordering::Acquire)
            && active_backend_ready
            && active_identity
                .as_ref()
                .is_some_and(|identity| identity.kind == BackendKind::Production);

        InferenceReadinessSnapshot {
            inference_interface_exists: true,
            real_backend_implementation_available: self
                .real_backend_implementation_available
                .load(Ordering::Acquire),
            selected_model_exists: inner.selected.is_some()
                && !matches!(&inner.state, ModelActivationState::ModelMissing { .. }),
            selected_model_verified: inner.state.is_verified(),
            activation_in_progress: inner.state.is_activation_in_progress(),
            active_backend_ready,
            development_mock_active,
            activation_failed: matches!(
                &inner.state,
                ModelActivationState::ActivationFailed { .. }
            ),
            product_ready,
            state: inner.state.clone(),
        }
    }

    fn record_activation_event(
        &self,
        name: &str,
        outcome: OutcomeClass,
        operation_id: u64,
        failure: Option<ActivationFailureCategory>,
        backend_kind: Option<BackendKind>,
    ) {
        match failure {
            Some(category) => tracing::warn!(
                operation_id,
                failure_category = category.as_tag(),
                backend_kind = backend_kind.map(|kind| kind.as_tag()).unwrap_or("unknown"),
                event = name,
                "model activation lifecycle event"
            ),
            None => tracing::info!(
                operation_id,
                backend_kind = backend_kind.map(|kind| kind.as_tag()).unwrap_or("unknown"),
                event = name,
                "model activation lifecycle event"
            ),
        }

        let Some(recorder) = self.observability.as_ref() else {
            return;
        };
        let Ok(mut event) = OperationalEvent::new(
            name,
            "engine.activation",
            if outcome.is_success_like() {
                EventSeverity::Info
            } else {
                EventSeverity::Warn
            },
            outcome,
            EventScope::Essential,
        ) else {
            return;
        };
        let _ = event.push_attribute(
            "operation_id",
            AttributeValue::Unsigned(operation_id),
            FieldSensitivity::OperationalSafe,
        );
        if let Some(category) = failure {
            let _ = event.push_attribute(
                "failure_category",
                AttributeValue::Stable(category.as_tag().to_string()),
                FieldSensitivity::OperationalSafe,
            );
        }
        if let Some(kind) = backend_kind {
            let _ = event.push_attribute(
                "backend_kind",
                AttributeValue::Stable(kind.as_tag().to_string()),
                FieldSensitivity::OperationalSafe,
            );
        }
        let _ = recorder.record_event(event);
    }
}

#[async_trait::async_trait]
impl InferenceBackend for ModelActivationService {
    fn identity(&self) -> BackendIdentity {
        let inner = self.inner.read();
        if let Some(active) = inner.active.as_ref() {
            return active.backend.identity();
        }
        match &inner.state {
            ModelActivationState::NoModelSelected => BackendIdentity::unavailable_with_reason(
                "activation_service",
                BackendUnavailableReason::NoModelSelected,
            ),
            ModelActivationState::ModelMissing { .. } => BackendIdentity::unavailable_with_reason(
                "activation_service",
                BackendUnavailableReason::ModelMissing,
            ),
            ModelActivationState::ModelVerifying { .. } => {
                BackendIdentity::unavailable_with_reason(
                    "activation_service",
                    BackendUnavailableReason::ModelVerifying,
                )
            }
            ModelActivationState::ModelVerified { .. } => BackendIdentity::unavailable_with_reason(
                "activation_service",
                BackendUnavailableReason::ModelVerifiedNotActivated,
            ),
            ModelActivationState::Activating { .. } => BackendIdentity::unavailable_with_reason(
                "activation_service",
                BackendUnavailableReason::ActivationInProgress,
            ),
            ModelActivationState::ActivationFailed { .. } => {
                BackendIdentity::unavailable_with_reason(
                    "activation_service",
                    BackendUnavailableReason::ActivationFailed,
                )
            }
            ModelActivationState::Deactivating { .. } => BackendIdentity::unavailable_with_reason(
                "activation_service",
                BackendUnavailableReason::Deactivating,
            ),
            ModelActivationState::Ready { .. } => BackendIdentity::unavailable_with_reason(
                "activation_service",
                BackendUnavailableReason::Unspecified,
            ),
        }
    }

    async fn run(
        &self,
        prompt: &str,
        cancel: CancellationToken,
        token_sender: mpsc::Sender<String>,
    ) -> Result<InferenceOutcome> {
        let backend = self
            .inner
            .read()
            .active
            .as_ref()
            .map(|active| active.backend.clone())
            .ok_or_else(|| {
                MukeiError::ModelLoadFailed("active inference backend is not ready".to_string())
            })?;
        // Clone the active Arc before awaiting. A concurrent successful model
        // switch can publish a new backend without changing this in-flight turn.
        backend.run(prompt, cancel, token_sender).await
    }

    async fn run_bounded(
        &self,
        prompt: &str,
        cancel: CancellationToken,
        token_sender: mpsc::Sender<String>,
        max_tokens: u64,
    ) -> Result<InferenceOutcome> {
        let backend = self
            .inner
            .read()
            .active
            .as_ref()
            .map(|active| active.backend.clone())
            .ok_or_else(|| {
                MukeiError::ModelLoadFailed("active inference backend is not ready".to_string())
            })?;
        backend
            .run_bounded(prompt, cancel, token_sender, max_tokens)
            .await
    }
}

fn classify_activation_error(error: &MukeiError) -> ActivationFailureCategory {
    match error {
        MukeiError::ModelCorrupted => ActivationFailureCategory::VerificationMismatch,
        MukeiError::ModelLoadFailed(_)
        | MukeiError::ContextCreationFailed(_)
        | MukeiError::MemoryPreflightRejected(_)
        | MukeiError::OOM => ActivationFailureCategory::ModelLoad,
        _ => ActivationFailureCategory::Internal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{MockInferenceBackend, StopReason};
    use std::time::Duration;

    struct StaticFactory {
        backend: Arc<dyn InferenceBackend>,
        delay: Duration,
    }

    #[async_trait::async_trait]
    impl InferenceBackendFactory for StaticFactory {
        async fn activate(
            &self,
            _descriptor: &VerifiedModelDescriptor,
        ) -> Result<Arc<dyn InferenceBackend>> {
            if !self.delay.is_zero() {
                tokio::time::sleep(self.delay).await;
            }
            Ok(self.backend.clone())
        }
    }

    struct ProductionBackend {
        inner: MockInferenceBackend,
    }

    #[async_trait::async_trait]
    impl InferenceBackend for ProductionBackend {
        fn identity(&self) -> BackendIdentity {
            BackendIdentity::production("production_test_backend")
        }

        async fn run(
            &self,
            prompt: &str,
            cancel: CancellationToken,
            token_sender: mpsc::Sender<String>,
        ) -> Result<InferenceOutcome> {
            self.inner.run(prompt, cancel, token_sender).await
        }

        async fn run_bounded(
            &self,
            prompt: &str,
            cancel: CancellationToken,
            token_sender: mpsc::Sender<String>,
            max_tokens: u64,
        ) -> Result<InferenceOutcome> {
            self.inner
                .run_bounded(prompt, cancel, token_sender, max_tokens)
                .await
        }
    }

    struct FailingFactory;

    #[async_trait::async_trait]
    impl InferenceBackendFactory for FailingFactory {
        async fn activate(
            &self,
            _descriptor: &VerifiedModelDescriptor,
        ) -> Result<Arc<dyn InferenceBackend>> {
            Err(MukeiError::ModelLoadFailed(
                "synthetic activation failure".to_string(),
            ))
        }
    }

    struct PanicFactory;

    #[async_trait::async_trait]
    impl InferenceBackendFactory for PanicFactory {
        async fn activate(
            &self,
            _descriptor: &VerifiedModelDescriptor,
        ) -> Result<Arc<dyn InferenceBackend>> {
            panic!("synthetic activation panic")
        }
    }

    fn descriptor(id: &str) -> VerifiedModelDescriptor {
        VerifiedModelDescriptor::new(
            id,
            "r1",
            VerifiedModelArtifact::new(format!("artifact-{id}"), format!("/{id}.gguf")).unwrap(),
        )
        .unwrap()
    }

    fn explicit_mock() -> Arc<dyn InferenceBackend> {
        Arc::new(MockInferenceBackend {
            chunk_bytes: 32,
            per_chunk_ms: 0,
            template: "ok".to_string(),
        })
    }

    fn production_backend() -> Arc<dyn InferenceBackend> {
        Arc::new(ProductionBackend {
            inner: MockInferenceBackend {
                chunk_bytes: 32,
                per_chunk_ms: 0,
                template: "ok".to_string(),
            },
        })
    }

    #[test]
    fn downloaded_or_missing_model_is_not_ready() {
        let service = ModelActivationService::new(true);
        service.mark_model_missing("model-a", "r1");
        let snapshot = service.readiness_snapshot();
        assert!(!snapshot.selected_model_exists);
        assert!(!snapshot.active_backend_ready);
        assert!(!snapshot.product_ready);
    }

    #[test]
    fn downloaded_model_under_verification_is_not_ready() {
        let service = ModelActivationService::new(true);
        let generation = service.begin_verification("model-a", "r1");
        let snapshot = service.readiness_snapshot();
        assert!(generation > 0);
        assert!(snapshot.selected_model_exists);
        assert!(!snapshot.selected_model_verified);
        assert!(!snapshot.active_backend_ready);
        assert!(!snapshot.product_ready);
    }

    #[test]
    fn verified_model_is_not_ready() {
        let service = ModelActivationService::new(true);
        let generation = service.begin_verification("model-a", "r1");
        assert!(service.mark_verified(generation, descriptor("model-a")));
        let snapshot = service.readiness_snapshot();
        assert!(snapshot.selected_model_verified);
        assert!(!snapshot.active_backend_ready);
        assert!(!snapshot.product_ready);
    }

    #[tokio::test]
    async fn successful_explicit_mock_activation_is_ready_but_not_product_ready() {
        let service = ModelActivationService::new(true);
        let generation = service.begin_verification("model-a", "r1");
        assert!(service.mark_verified(generation, descriptor("model-a")));
        let factory = StaticFactory {
            backend: explicit_mock(),
            delay: Duration::ZERO,
        };
        assert_eq!(
            service.activate_verified(&factory).await,
            ActivationCommit::Ready
        );
        let snapshot = service.readiness_snapshot();
        assert!(snapshot.active_backend_ready);
        assert!(snapshot.development_mock_active);
        assert!(!snapshot.product_ready);
    }

    #[tokio::test]
    async fn successful_production_activation_becomes_product_ready() {
        let service = ModelActivationService::new(true);
        let generation = service.begin_verification("model-a", "r1");
        assert!(service.mark_verified(generation, descriptor("model-a")));
        let factory = StaticFactory {
            backend: production_backend(),
            delay: Duration::ZERO,
        };
        assert_eq!(
            service.activate_verified(&factory).await,
            ActivationCommit::Ready
        );
        let snapshot = service.readiness_snapshot();
        assert!(snapshot.active_backend_ready);
        assert!(!snapshot.development_mock_active);
        assert!(snapshot.product_ready);
        assert_eq!(service.identity().kind, BackendKind::Production);
    }

    #[tokio::test]
    async fn production_identity_cannot_claim_product_ready_when_implementation_is_unavailable() {
        let service = ModelActivationService::new(false);
        let generation = service.begin_verification("model-a", "r1");
        assert!(service.mark_verified(generation, descriptor("model-a")));
        let factory = StaticFactory {
            backend: production_backend(),
            delay: Duration::ZERO,
        };
        assert_eq!(
            service.activate_verified(&factory).await,
            ActivationCommit::Ready
        );
        let snapshot = service.readiness_snapshot();
        assert!(snapshot.active_backend_ready);
        assert!(!snapshot.real_backend_implementation_available);
        assert!(!snapshot.product_ready);
    }

    #[tokio::test]
    async fn candidate_verification_preserves_current_active_backend() {
        let service = ModelActivationService::new(true);
        let generation = service.begin_verification("model-a", "r1");
        assert!(service.mark_verified(generation, descriptor("model-a")));
        let factory = StaticFactory {
            backend: production_backend(),
            delay: Duration::ZERO,
        };
        assert_eq!(
            service.activate_verified(&factory).await,
            ActivationCommit::Ready
        );

        let generation_b = service.begin_verification("model-b", "r1");
        assert!(generation_b > generation);
        assert_eq!(service.identity().kind, BackendKind::Production);
        assert!(service.readiness_snapshot().active_backend_ready);

        let (tx, _rx) = mpsc::channel(8);
        let outcome = service
            .run("still routed", CancellationToken::new(), tx)
            .await
            .unwrap();
        assert_eq!(outcome.stop_reason, StopReason::Completed);
    }

    #[tokio::test]
    async fn failed_replacement_activation_preserves_previous_backend() {
        let service = ModelActivationService::new(true);
        let generation = service.begin_verification("model-a", "r1");
        assert!(service.mark_verified(generation, descriptor("model-a")));
        let factory = StaticFactory {
            backend: production_backend(),
            delay: Duration::ZERO,
        };
        assert_eq!(
            service.activate_verified(&factory).await,
            ActivationCommit::Ready
        );

        let generation_b = service.begin_verification("model-b", "r1");
        assert!(service.mark_verified(generation_b, descriptor("model-b")));
        assert_eq!(
            service.activate_verified(&FailingFactory).await,
            ActivationCommit::Failed(ActivationFailureCategory::ModelLoad)
        );
        assert!(service.readiness_snapshot().activation_failed);
        assert!(service.readiness_snapshot().active_backend_ready);
        assert_eq!(service.identity().kind, BackendKind::Production);
    }

    #[test]
    fn backend_factory_availability_is_dynamic_and_truthful() {
        let service = ModelActivationService::new(false);
        assert!(
            !service
                .readiness_snapshot()
                .real_backend_implementation_available
        );
        service.set_real_backend_implementation_available(true);
        assert!(
            service
                .readiness_snapshot()
                .real_backend_implementation_available
        );
        service.set_real_backend_implementation_available(false);
        assert!(
            !service
                .readiness_snapshot()
                .real_backend_implementation_available
        );
    }

    #[tokio::test]
    async fn failed_activation_is_explicit_and_never_falls_back_to_mock() {
        let service = ModelActivationService::new(true);
        let generation = service.begin_verification("model-a", "r1");
        assert!(service.mark_verified(generation, descriptor("model-a")));
        assert_eq!(
            service.activate_verified(&FailingFactory).await,
            ActivationCommit::Failed(ActivationFailureCategory::ModelLoad)
        );
        let snapshot = service.readiness_snapshot();
        assert!(snapshot.activation_failed);
        assert!(!snapshot.active_backend_ready);
        assert!(!snapshot.development_mock_active);
        assert!(!snapshot.product_ready);
        assert_eq!(
            service.identity().unavailable_reason,
            Some(BackendUnavailableReason::ActivationFailed)
        );
    }

    #[tokio::test]
    async fn activation_factory_panic_becomes_explicit_internal_failure() {
        let service = ModelActivationService::new(true);
        let generation = service.begin_verification("model-a", "r1");
        assert!(service.mark_verified(generation, descriptor("model-a")));
        assert_eq!(
            service.activate_verified(&PanicFactory).await,
            ActivationCommit::Failed(ActivationFailureCategory::Internal)
        );
        assert!(matches!(
            service.state(),
            ModelActivationState::ActivationFailed {
                category: ActivationFailureCategory::Internal,
                ..
            }
        ));
        assert!(!service.readiness_snapshot().product_ready);
    }

    #[tokio::test]
    async fn stale_activation_a_cannot_overwrite_newer_model_b() {
        let service = ModelActivationService::new(true);
        let generation_a = service.begin_verification("model-a", "r1");
        assert!(service.mark_verified(generation_a, descriptor("model-a")));

        let slow_factory = StaticFactory {
            backend: explicit_mock(),
            delay: Duration::from_millis(40),
        };
        let service_for_a = service.clone();
        let a = tokio::spawn(async move { service_for_a.activate_verified(&slow_factory).await });

        tokio::time::sleep(Duration::from_millis(5)).await;
        let generation_b = service.begin_verification("model-b", "r1");
        assert!(service.mark_verified(generation_b, descriptor("model-b")));
        let fast_factory = StaticFactory {
            backend: explicit_mock(),
            delay: Duration::ZERO,
        };
        assert_eq!(
            service.activate_verified(&fast_factory).await,
            ActivationCommit::Ready
        );
        assert_eq!(a.await.unwrap(), ActivationCommit::StaleIgnored);

        match service.state() {
            ModelActivationState::Ready { model_id, .. } => assert_eq!(model_id, "model-b"),
            state => panic!("expected model-b ready, got {state:?}"),
        }
    }

    #[tokio::test]
    async fn ready_backend_dispatches_inference_only_after_activation() {
        let service = ModelActivationService::new(true);
        let (tx, _rx) = mpsc::channel(8);
        let unavailable = service.run("hello", CancellationToken::new(), tx).await;
        assert!(unavailable.is_err());

        let generation = service.begin_verification("model-a", "r1");
        assert!(service.mark_verified(generation, descriptor("model-a")));
        let factory = StaticFactory {
            backend: explicit_mock(),
            delay: Duration::ZERO,
        };
        assert_eq!(
            service.activate_verified(&factory).await,
            ActivationCommit::Ready
        );

        let (tx, _rx) = mpsc::channel(8);
        let outcome = service
            .run("hello", CancellationToken::new(), tx)
            .await
            .unwrap();
        assert_eq!(outcome.stop_reason, StopReason::Completed);
    }
}
