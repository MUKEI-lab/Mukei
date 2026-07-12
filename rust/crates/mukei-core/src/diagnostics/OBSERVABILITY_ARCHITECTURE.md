# Mukei Diagnostics Observability Foundation

## Scope and ownership

This diagnostics-owned subsystem provides local structured operational events, bounded metrics, explicit health state, SLO aggregation, privacy policy enforcement, crash diagnostics, recent-event buffering, diagnostic snapshots, and pluggable sink boundaries. It does not implement product analytics, billing metering, inference lifecycle, Protocol V2, storage migrations, bridge dispatch, or a remote telemetry backend.

## Privacy boundary and epoch model

Structured data is sanitized before it can enter any recorder-owned queue. Event identities and metric dimensions accept only bounded stable machine identifiers. Text attributes are passed through canonical redaction, and high-risk field names such as prompts, model output, authorization/token material, key blobs, document content, filenames, private paths, and device/customer/tenant identifiers are redacted even when a caller accidentally marks them operational-safe. Field-name classification is applied before value-type handling, so `Stable`, numeric, or boolean values cannot bypass a sensitive-key rule. The general telemetry text sanitizer hard-caps both scan work and retained output. Fields explicitly classified `Sensitive` or `Secret` are rejected.

Correlation identifiers are bounded one-way fingerprints. They are not copied into metric dimensions automatically.

`TelemetryPolicy` separates two decisions:

1. local diagnostic coverage (`Disabled`, `Essential`, `Extended`); and
2. whether sink export is allowed.

The default is local `Essential` diagnostics with export disabled. Export requires explicit opt-in.

Every policy change increments a privacy epoch. Sink envelopes carry the epoch and scope under which they were created. A sink worker revalidates the epoch immediately before callback delivery; stale queued work is discarded and counted. Narrowing from `Extended` to `Essential` removes extended recent events and clears registries that do not retain per-series scope. `Disabled` clears locally retained recorder state. A recorder-level policy update gate prevents an old-policy producer from writing after a narrowing clear has completed.

## Memory bounds and overflow policy

The recent-event queue is bounded by all of:

- envelope count;
- total approximate encoded/in-memory bytes;
- maximum single-event bytes.

Each event computes a conservative size estimate before insertion. Oversized events are rejected. Non-critical events cannot consume the configured critical reserve. When pressure requires eviction, critical `Warn`/`Error` events preferentially evict the oldest non-critical diagnostic. Non-critical events never evict retained critical events merely to gain capacity. All evictions and rejections are counted.

Metric, health, and SLO registries use fixed cardinality budgets. Metric dimensions are a closed five-field schema with bounded values and an ASCII stable-identifier policy; arbitrary label maps and arbitrary JSON payloads are intentionally absent. Series overflow drops the new series and increments an overflow counter.

Distributions retain fixed buckets rather than raw observations. Metric and SLO histories retain only current window, previous window, and fixed-size lifetime aggregate state.

## Coalescing semantics

- Counters aggregate deltas in place.
- Gauges retain the latest value for a stable metric series.
- Equivalent health signals within the bounded coalescing window refresh one stored state instead of creating history growth; state transitions remain explicit through previous-state and transition-count metadata.
- Pending metric snapshots in a sink queue are replaceable point-in-time views, so a newer snapshot replaces an older pending snapshot.
- Semantically distinct operational events, especially critical failures, are never coalesced.

## Sink fan-out and slow-sink isolation

Recorder fan-out shares immutable event/snapshot payloads through `Arc`; it does not clone a full payload per sink.

Every sink owns an independent queue with:

- maximum queued count;
- maximum logical queued bytes;
- maximum single-envelope bytes;
- non-blocking producer insertion;
- metric-snapshot coalescing;
- queue-pressure drop accounting;
- a bounded consecutive-drop disconnect policy;
- degraded/disconnected health statistics;
- slow callback accounting.

A blocked sink can hold only its one in-flight callback plus its bounded queue. It cannot create unbounded producer pressure and cannot prevent another sink worker from continuing. The recorder exposes the aggregate worst sink health (`Healthy`, `Degraded`, or `Disconnected`) together with bounded queue pressure counters for local diagnostics. The synchronous `DiagnosticSink` trait is not forcibly interrupted mid-callback; persistent producer-side pressure disconnects further delivery instead of spawning unbounded timeout threads.

## Monotonic time

`ObservabilityClock` provides two timelines:

- monotonic elapsed time for window rotation, health expiry, sink callback age, and other duration decisions;
- UTC wall time for human-readable snapshot/export timestamps.

Production uses `SystemClock` (`Instant` + UTC). Tests inject deterministic clocks. Metrics, SLO windows, and health expiry never derive elapsed duration from wall-clock subtraction, so wall-clock rollback cannot freeze expiry, create negative elapsed duration, or prevent window rotation.

## SLO semantics

SLO summaries have an explicit measurement interval and retain current, previous, and lifetime aggregate state. Operation ratios use explicit numerator/denominator fields. Division by zero and low sample counts return `None`; `SloSampleState::InsufficientData` remains distinct from sufficient data and is never treated as healthy by implication.

The default minimum operation sample count is `MIN_SLO_OPERATION_SAMPLES`. Raw success ratio remains available separately for diagnostic inspection, while policy-facing ratios require sufficient samples.

## Health semantics

Health uses the controlled states `Unknown`, `Healthy`, `Degraded`, and `Unhealthy`. Expiry is monotonic and produces `Unknown`. Explicit publishes drive recovery; a transient error does not create an append-only permanent failure record. Equivalent repeats coalesce, while state changes record transition metadata.

Aggregation rules remain deterministic: critical unhealthy dominates; degraded or non-critical unhealthy yields degraded; critical unknown prevents an otherwise healthy aggregate from being reported healthy; empty state is unknown.

## Crash and panic diagnostics

Panic-hook installation is idempotent for the normal install path and recursion guarded. Arbitrary panic payloads are treated as potentially sensitive: only bounded stable reason codes are retained, otherwise the human-readable reason is redacted. Sink callback panics are caught to avoid recursive failure loops.

Crash records have bounded fields and a hard serialized-size limit. Public crash-record values are re-sanitized immediately before serialization, reads are bounded, writes use a temporary-file replacement pattern, failures are best-effort/local-only, and retained crash files are capped. Retention pruning keeps only a bounded in-memory candidate set while scanning legacy directories, so even cleanup work remains memory-bounded. Private paths and secret-shaped values pass through diagnostics redaction before persistence.

## UI store role

`DiagnosticsStore.qml` exposes derived presentation state for local enablement, export permission, privacy epoch, queue degradation, and dropped/coalesced counters. It never becomes authoritative for privacy policy; Rust remains the source of truth and the QML store does not expose raw diagnostic payloads.
