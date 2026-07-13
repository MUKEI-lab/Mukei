from pathlib import Path

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
