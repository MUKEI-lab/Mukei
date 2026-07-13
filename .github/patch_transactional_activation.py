from pathlib import Path

path = Path("rust/crates/mukei-core/src/engine/activation.rs")
text = path.read_text(encoding="utf-8")


def replace_once(old: str, new: str, label: str) -> None:
    global text
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{label}: expected one anchor, found {count}")
    text = text.replace(old, new, 1)
    print(f"PASS {label}")

replace_once(
    "use std::sync::atomic::{AtomicU64, Ordering};",
    "use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};",
    "activation atomic availability import",
)
replace_once(
    "    real_backend_implementation_available: bool,",
    "    real_backend_implementation_available: AtomicBool,",
    "activation dynamic availability field",
)
if text.count("            real_backend_implementation_available,") != 2:
    raise SystemExit("activation constructors: expected two availability initializers")
text = text.replace(
    "            real_backend_implementation_available,",
    "            real_backend_implementation_available: AtomicBool::new(\n                real_backend_implementation_available,\n            ),",
)
print("PASS activation constructors use AtomicBool")

replace_once(
    '''    fn is_current_generation(&self, generation: u64) -> bool {
        self.generation.load(Ordering::Acquire) == generation
    }

    pub fn state(&self) -> ModelActivationState {''',
    '''    fn is_current_generation(&self, generation: u64) -> bool {
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

    pub fn state(&self) -> ModelActivationState {''',
    "activation dynamic availability setter",
)

replace_once(
    '''        let (generation, retired) = {
            let mut inner = self.inner.write();
            let generation = self.next_generation();
            inner.selected = Some((model_id.clone(), revision.clone()));
            inner.verified = None;
            inner.state = ModelActivationState::ModelMissing {
                model_id,
                revision,
                generation,
            };
            (generation, inner.active.take())
        };
        drop_retired_backend_safely(retired);
        generation''',
    '''        let mut inner = self.inner.write();
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
        generation''',
    "mark_model_missing preserves active backend",
)
replace_once(
    '''        let (generation, retired) = {
            let mut inner = self.inner.write();
            let generation = self.next_generation();
            inner.selected = Some((model_id.clone(), revision.clone()));
            inner.verified = None;
            inner.state = ModelActivationState::ModelVerifying {
                model_id,
                revision,
                generation,
            };
            (generation, inner.active.take())
        };
        drop_retired_backend_safely(retired);
        generation''',
    '''        let mut inner = self.inner.write();
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
        generation''',
    "begin_verification preserves active backend",
)

replace_once(
    '''        let retired = {
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
            inner.active.take()
        };
        drop_retired_backend_safely(retired);
        true''',
    '''        let mut inner = self.inner.write();
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
        true''',
    "activation failure preserves previous backend",
)

replace_once(
    '''        let ready_state = matches!(&inner.state, ModelActivationState::Ready { .. });
        let active_backend_ready = ready_state && active_identity.is_some();
        let development_mock_active = active_backend_ready
            && active_identity
                .as_ref()
                .is_some_and(|identity| identity.kind == BackendKind::DevelopmentMock);
        let product_ready = self.real_backend_implementation_available
            && active_backend_ready
            && active_identity
                .as_ref()
                .is_some_and(|identity| identity.kind == BackendKind::Production);''',
    '''        let active_backend_ready = active_identity
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
                .is_some_and(|identity| identity.kind == BackendKind::Production);''',
    "readiness follows active backend independently of candidate state",
)
replace_once(
    "            real_backend_implementation_available: self.real_backend_implementation_available,",
    "            real_backend_implementation_available: self\n                .real_backend_implementation_available\n                .load(Ordering::Acquire),",
    "readiness snapshots dynamic factory availability",
)

old_identity = '''    fn identity(&self) -> BackendIdentity {
        let inner = self.inner.read();
        match (&inner.state, &inner.active) {
            (
                ModelActivationState::Ready {
                    operation_id,
                    model_id,
                    revision,
                    artifact_id,
                    ..
                },
                Some(active),
            ) if active.operation_id == *operation_id
                && active.model_id.as_str() == model_id.as_str()
                && active.revision.as_str() == revision.as_str()
                && active.artifact_id.as_str() == artifact_id.as_str() =>
            {
                active.backend.identity()
            }
            (ModelActivationState::NoModelSelected, _) => BackendIdentity::unavailable_with_reason(
                "activation_service",
                BackendUnavailableReason::NoModelSelected,
            ),
            (ModelActivationState::ModelMissing { .. }, _) => {
                BackendIdentity::unavailable_with_reason(
                    "activation_service",
                    BackendUnavailableReason::ModelMissing,
                )
            }
            (ModelActivationState::ModelVerifying { .. }, _) => {
                BackendIdentity::unavailable_with_reason(
                    "activation_service",
                    BackendUnavailableReason::ModelVerifying,
                )
            }
            (ModelActivationState::ModelVerified { .. }, _) => {
                BackendIdentity::unavailable_with_reason(
                    "activation_service",
                    BackendUnavailableReason::ModelVerifiedNotActivated,
                )
            }
            (ModelActivationState::Activating { .. }, _) => {
                BackendIdentity::unavailable_with_reason(
                    "activation_service",
                    BackendUnavailableReason::ActivationInProgress,
                )
            }
            (ModelActivationState::ActivationFailed { .. }, _) => {
                BackendIdentity::unavailable_with_reason(
                    "activation_service",
                    BackendUnavailableReason::ActivationFailed,
                )
            }
            (ModelActivationState::Deactivating { .. }, _) => {
                BackendIdentity::unavailable_with_reason(
                    "activation_service",
                    BackendUnavailableReason::Deactivating,
                )
            }
            (ModelActivationState::Ready { .. }, _) => BackendIdentity::unavailable_with_reason(
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
        let backend = {
            let inner = self.inner.read();
            match (&inner.state, &inner.active) {
                (
                    ModelActivationState::Ready {
                        operation_id,
                        model_id,
                        revision,
                        artifact_id,
                        ..
                    },
                    Some(active),
                ) if active.operation_id == *operation_id
                    && active.model_id.as_str() == model_id.as_str()
                    && active.revision.as_str() == revision.as_str()
                    && active.artifact_id.as_str() == artifact_id.as_str() =>
                {
                    active.backend.clone()
                }
                _ => {
                    return Err(MukeiError::ModelLoadFailed(
                        "active inference backend is not ready".to_string(),
                    ))
                }
            }
        };
        backend.run(prompt, cancel, token_sender).await
    }'''
new_identity = '''    fn identity(&self) -> BackendIdentity {
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
            ModelActivationState::ModelVerifying { .. } => BackendIdentity::unavailable_with_reason(
                "activation_service",
                BackendUnavailableReason::ModelVerifying,
            ),
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
    }'''
replace_once(old_identity, new_identity, "active backend router is independent from candidate state")

insert_anchor = '''    #[tokio::test]
    async fn failed_activation_is_explicit_and_never_falls_back_to_mock() {'''
insert_tests = '''    #[tokio::test]
    async fn candidate_verification_preserves_current_active_backend() {
        let service = ModelActivationService::new(true);
        let generation = service.begin_verification("model-a", "r1");
        assert!(service.mark_verified(generation, descriptor("model-a")));
        let factory = StaticFactory {
            backend: production_backend(),
            delay: Duration::ZERO,
        };
        assert_eq!(service.activate_verified(&factory).await, ActivationCommit::Ready);

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
        assert_eq!(service.activate_verified(&factory).await, ActivationCommit::Ready);

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
        assert!(!service.readiness_snapshot().real_backend_implementation_available);
        service.set_real_backend_implementation_available(true);
        assert!(service.readiness_snapshot().real_backend_implementation_available);
        service.set_real_backend_implementation_available(false);
        assert!(!service.readiness_snapshot().real_backend_implementation_available);
    }

'''
if text.count(insert_anchor) != 1:
    raise SystemExit("activation transactional tests insertion anchor mismatch")
text = text.replace(insert_anchor, insert_tests + insert_anchor, 1)

path.write_text(text, encoding="utf-8")

for temporary in [
    ".github/patch_transactional_activation.py",
    ".github/workflows/transactional-activation-runner.yml",
]:
    candidate = Path(temporary)
    if candidate.exists():
        candidate.unlink()

print("Transactional activation core hardening complete")
