# Mukei v0.4 Sol — Durable Agent History Patch

Base: `Mukei_v0.3_sol`

## Implemented

- Added `AgentEventSink` and a bridge-backed durable sink.
- Persisted every assistant tool-call attempt, validator envelope, tool result,
  repeated-output notice, and supervisor directive before the next inference
  iteration starts.
- Final assistant rows now point at the actual last durable intermediate
  message, preserving the branch graph.
- Final persisted assistant content now comes from the inference result itself,
  not the concatenated stream containing prior hidden tool-protocol iterations.
- Added token-aware finalization metadata through `AgentRunOutcome`.
- Failed/cancelled turns retain recovery snapshots instead of deleting them.
- Added interrupted-turn discovery and atomic recovery-attempt storage
  primitives while keeping the original partial response immutable.

## Manual compile verification required

The build environment used for this patch did not contain `cargo`/`rustc`.
Run from `rust/`:

```bash
cargo fmt --all -- --check
cargo check -p mukei-core --all-features
cargo test -p mukei-core --all-features
cargo check -p mukei-bridge --all-features
```

v0.4 intentionally contains storage/runtime primitives for recovery; the
bridge-executable resume/regenerate flow is completed in v0.5.
