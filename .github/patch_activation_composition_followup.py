from pathlib import Path


def replace_once(path: Path, old: str, new: str, label: str) -> None:
    text = path.read_text(encoding="utf-8")
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{label} anchor mismatch: {count}")
    path.write_text(text.replace(old, new, 1), encoding="utf-8")


# The root patch originally introduced an optional factory registry before a
# concrete production factory existed. That left dead composition state and
# failed `-D warnings`. Keep the authoritative activation router, but do not
# retain a factory slot that no executable producer can populate yet.
app_runtime = Path("rust/crates/mukei-bridge/src/app_runtime.rs")
replace_once(
    app_runtime,
    "use mukei_core::engine::{InferenceBackendFactory, ModelActivationService};",
    "use mukei_core::engine::ModelActivationService;",
    "remove premature factory import",
)
replace_once(
    app_runtime,
    "    inference_backend_factory: ParkingMutex<Option<Arc<dyn InferenceBackendFactory>>>,\n",
    "",
    "remove premature factory field",
)
replace_once(
    app_runtime,
    "                inference_backend_factory: ParkingMutex::new(None),\n",
    "",
    "remove premature factory initialization",
)
replace_once(
    app_runtime,
    '''    pub(crate) fn inference_backend_factory(&self) -> Option<Arc<dyn InferenceBackendFactory>> {
        self.agent.inference_backend_factory.lock().clone()
    }

    pub(crate) fn set_inference_backend_factory(
        &self,
        factory: Option<Arc<dyn InferenceBackendFactory>>,
    ) {
        let available = factory.is_some();
        *self.agent.inference_backend_factory.lock() = factory;
        self.agent
            .activation_service
            .set_real_backend_implementation_available(available);
    }

''',
    "",
    "remove premature factory accessors",
)
replace_once(
    app_runtime,
    "        assert!(application_runtime().inference_backend_factory().is_none());\n",
    "",
    "remove premature factory singleton assertion",
)

# Complete every existing AgentLoop rebuild callsite so registry refreshes keep
# the same process-owned activation router instead of silently reconstructing
# an unavailable backend.
path = Path("rust/crates/mukei-bridge/src/lib.rs")
text = path.read_text(encoding="utf-8")

old = '''use mukei_core::agent::{AgentEventSink, AgentRunRequest};
use mukei_core::ffi::tags::{TagEvents, TagsStreaming};'''
new = '''use mukei_core::agent::{AgentEventSink, AgentRunRequest};
use mukei_core::engine::InferenceBackend;
use mukei_core::ffi::tags::{TagEvents, TagsStreaming};'''
if text.count(old) != 1:
    raise SystemExit(f"InferenceBackend import anchor mismatch: {text.count(old)}")
text = text.replace(old, new, 1)

old = '''                let loop_handle = agent_runtime::build_agent_loop(
                    &cfg,
                    registry.clone(),
                    pool,
                    runtime_state().audit_log_writer().clone(),
                );'''
new = '''                let loop_handle = agent_runtime::build_agent_loop(
                    &cfg,
                    registry.clone(),
                    pool,
                    runtime_state().audit_log_writer().clone(),
                    runtime_state().model_activation_service(),
                );'''
count = text.count(old)
if count != 2:
    raise SystemExit(f"rusqlite registry-rebuild callsite count mismatch: {count}")
text = text.replace(old, new)

old = '''            let loop_handle = agent_runtime::build_agent_loop(&cfg, registry.clone());'''
new = '''            let loop_handle = agent_runtime::build_agent_loop(
                &cfg,
                registry.clone(),
                runtime_state().model_activation_service(),
            );'''
count = text.count(old)
if count != 2:
    raise SystemExit(f"non-rusqlite registry-rebuild callsite count mismatch: {count}")
text = text.replace(old, new)

path.write_text(text, encoding="utf-8")

# Supplemental transport is temporary and must not survive the production commit.
Path(".github/patch_activation_composition_followup.py").unlink()
print("Activation composition compile follow-up complete")
