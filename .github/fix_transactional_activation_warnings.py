from pathlib import Path

path = Path("rust/crates/mukei-core/src/engine/activation.rs")
text = path.read_text(encoding="utf-8")

old = '''/// Stable backend dependency injected into `AgentLoop`.
///
/// The service itself implements [`InferenceBackend`], so the loop owns one
/// stable dependency while activation can atomically replace the underlying
/// model backend. A non-ready lifecycle state always rejects inference rather
/// than falling back to a mock or an older selected model.'''
new = '''/// Stable backend dependency injected into `AgentLoop`.
///
/// The service itself implements [`InferenceBackend`], so the loop owns one
/// stable dependency while activation can atomically replace the underlying
/// model backend. Candidate verification/activation may coexist with a healthy
/// active backend; only explicit deactivation removes it before replacement.'''
if text.count(old) != 1:
    raise SystemExit(f"activation service docs anchor mismatch: {text.count(old)}")
text = text.replace(old, new, 1)

old = '''struct ActiveBackend {
    model_id: String,
    revision: String,
    artifact_id: String,
    operation_id: u64,
    backend: Arc<dyn InferenceBackend>,
}'''
new = '''struct ActiveBackend {
    model_id: String,
    revision: String,
    artifact_id: String,
    backend: Arc<dyn InferenceBackend>,
}'''
if text.count(old) != 1:
    raise SystemExit(f"ActiveBackend field anchor mismatch: {text.count(old)}")
text = text.replace(old, new, 1)

old = '''            let next = ActiveBackend {
                model_id: descriptor.model_id.clone(),
                revision: descriptor.revision.clone(),
                artifact_id: descriptor.artifact.artifact_id().to_string(),
                operation_id,
                backend,
            };'''
new = '''            let next = ActiveBackend {
                model_id: descriptor.model_id.clone(),
                revision: descriptor.revision.clone(),
                artifact_id: descriptor.artifact.artifact_id().to_string(),
                backend,
            };'''
if text.count(old) != 1:
    raise SystemExit(f"ActiveBackend construction anchor mismatch: {text.count(old)}")
text = text.replace(old, new, 1)

path.write_text(text, encoding="utf-8")
for temporary in [
    ".github/fix_transactional_activation_warnings.py",
    ".github/workflows/transactional-activation-warning-runner.yml",
]:
    candidate = Path(temporary)
    if candidate.exists():
        candidate.unlink()
print("transactional activation warning cleanup complete")
