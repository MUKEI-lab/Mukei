use std::path::PathBuf;
use std::sync::Arc;

use mukei_core::engine::{
    ActivationCommit, ActivationFailureCategory, InferenceBackend, InferenceBackendFactory,
    MockInferenceBackend, ModelActivationService, VerifiedModelArtifact, VerifiedModelDescriptor,
};

struct MockFactory;

#[async_trait::async_trait]
impl InferenceBackendFactory for MockFactory {
    async fn activate(
        &self,
        _descriptor: &VerifiedModelDescriptor,
    ) -> mukei_core::error::Result<Arc<dyn InferenceBackend>> {
        Ok(Arc::new(MockInferenceBackend::default()))
    }
}

fn verified(model_id: &str, revision: &str, artifact_id: &str) -> VerifiedModelDescriptor {
    let artifact =
        VerifiedModelArtifact::new(artifact_id, PathBuf::from("/tmp/mukei-test-model.gguf"))
            .expect("valid test artifact");
    VerifiedModelDescriptor::new(model_id, revision, artifact).expect("valid test descriptor")
}

#[test]
fn verification_is_reported_as_activation_in_progress() {
    let service = ModelActivationService::new(true);
    let _generation = service.begin_verification("model-a", "rev-a");
    assert!(service.readiness_snapshot().activation_in_progress);
}

#[tokio::test]
async fn verification_failure_preserves_previous_active_backend() {
    let service = ModelActivationService::new(true);
    let first_generation = service.begin_verification("model-a", "rev-a");
    assert!(service.mark_verified(first_generation, verified("model-a", "rev-a", "artifact-a")));
    assert_eq!(
        service.activate_verified(&MockFactory).await,
        ActivationCommit::Ready
    );
    let serving_before = service.active_model_snapshot().expect("active backend");

    let replacement_generation = service.begin_verification("model-b", "rev-b");
    assert!(service.mark_verification_failed(
        replacement_generation,
        "model-b",
        "rev-b",
        "artifact-b",
        ActivationFailureCategory::VerificationMismatch,
    ));

    let serving_after = service
        .active_model_snapshot()
        .expect("previous backend preserved");
    assert_eq!(serving_after.model_id, serving_before.model_id);
    assert_eq!(serving_after.artifact_id, serving_before.artifact_id);
    assert!(service.readiness_snapshot().active_backend_ready);
    assert!(service.readiness_snapshot().activation_failed);
}

#[test]
fn stale_verification_failure_cannot_overwrite_newer_selection() {
    let service = ModelActivationService::new(true);
    let stale_generation = service.begin_verification("model-a", "rev-a");
    let current_generation = service.begin_verification("model-b", "rev-b");

    assert!(!service.mark_verification_failed(
        stale_generation,
        "model-a",
        "rev-a",
        "artifact-a",
        ActivationFailureCategory::VerificationMismatch,
    ));
    assert_eq!(
        service.selected_model_snapshot(),
        Some(("model-b".to_string(), "rev-b".to_string()))
    );
    assert!(service.readiness_snapshot().activation_in_progress);
    let _ = current_generation;
}
