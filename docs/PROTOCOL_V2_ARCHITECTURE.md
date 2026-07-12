# Mukei Command/Event Protocol V2

## Purpose

Protocol V2 is the local UI boundary between QML and the existing Rust bridge. It is transport-neutral but currently uses only the in-process CXX-Qt bridge. It does not add network or cloud transport and does not move domain business logic into protocol types.

## Version and capability invariants

- Protocol versioning is independent of the app/crate version.
- Current protocol version is `2.0`; an unknown major version fails closed.
- Future minor versions may add optional fields without breaking older V2 consumers.
- Capability strings are stable machine identifiers and are advertised only for behavior implemented by that bridge mode.
- The production bridge advertises V2 commands, acknowledgements, event identity, per-stream sequencing, bounded idempotent replay protection, operation lifecycle projection, and the explicit legacy-event transition capability.
- The desktop stub advertises V2 command acknowledgement but honestly remains in isolated legacy-V1 event mode.

## Command lifecycle

QML creates one `CommandEnvelopeV2` with opaque `command_id`, `request_id`, `correlation_id`, optional `operation_id`, optional structured scope, typed payload, and an idempotency key.

The bridge performs this acceptance sequence before domain dispatch:

1. parse the bounded envelope;
2. reject unsupported protocol majors;
3. validate bounded identities, command registry membership, scope, and typed payload;
4. reject conflicting idempotency-key reuse or return the original accepted operation identity for a valid replay;
5. apply bridge-local availability/policy/busy preflight only to a new command;
6. allocate/fix the operation identity and correlation context;
7. create exactly one accepted acknowledgement;
8. queue dispatch on the existing Qt/runtime owner so completion events cannot race ahead of the returned acknowledgement;
9. adapt into the existing backend method without moving domain logic into the protocol layer.

`accepted` means only that the command was validated and accepted for processing. It never means a long-running operation has completed.

Rejection happens before execution and uses stable machine reasons such as `unsupported_protocol`, `unknown_command`, `invalid_payload`, `capability_unavailable`, `busy_conflict`, `stale_scope`, `backend_unavailable`, `duplicate_replay_conflict`, and `policy_denied`.

## Central QML operation projection

`OperationStore` is the single command/operation lifecycle projection. Screens continue to emit intents through `IntentDispatcher`; screens do not parse protocol JSON.

A command can move through:

`created` → `awaiting_acknowledgement` → `accepted` → `running` → `completed | failed | cancelled`

A bridge rejection moves the local provisional record to `rejected`. `rejected` is intentionally distinct from a failed running operation.

The projection is idempotent: the same acknowledgement does not create a second operation, terminal state is retained, and a late non-terminal event cannot regress a terminal operation.

## Event envelope and ordering

The production bridge wraps existing typed `BridgeEvent` payloads at the emission boundary in `EventEnvelopeV2`. Domain producers therefore remain unchanged.

Every V2 event carries:

- protocol version;
- stable event identity for that emitted event;
- deterministic `stream_id`;
- monotonic `sequence` within that stream;
- event type and emitted timestamp;
- correlation, operation, request, and command identities when applicable;
- the existing structured event payload.

Stream families are separated so unrelated work does not create false gaps:

- `application:lifecycle`;
- `conversation:<conversation>:branch:<branch>` (with a temporary `chat:active` fallback until scope is known);
- `download:model:<model>`;
- `operation:<operation>`;
- `application:errors`.

The old bridge-global V1 sequence, when present in the payload, is retained only as `legacy_sequence`; V2 ordering never compares it across streams.

## QML event acceptance

`EventDispatcher` is the only raw-event parser.

For V2 it:

- fails closed on unknown protocol majors or malformed V2 envelopes;
- rejects duplicate `event_id` values with bounded memory;
- compares sequence only inside one `stream_id`;
- rejects stale sequence values;
- rejects the gap event, signals controlled feature resynchronization, and resumes from the snapshot baseline;
- keeps stream tracking bounded;
- ignores unknown optional envelope fields;
- validates known payload shapes before store projection.

A second bounded logical-event fingerprint prevents a V1 and V2 copy of the same legacy payload from being applied twice during the transition window.

## Legacy transition

V2 is preferred. Legacy V1 events remain recognized only by the isolated compatibility parser. Contract negotiation reports the active event mode as `protocol_v2` or `legacy_v1`; the modes are not presented as equivalent reliability guarantees.

The production Rust bridge emits V2 envelopes. The desktop stub remains a deliberate legacy-event peer while still supporting the V2 command acknowledgement boundary.

## Recovery acknowledgement invariant

Recovery state is authoritative until the backend has actually claimed the interrupted turn.

The flow is:

1. QML submits `recovery.resume` or `recovery.regenerate` with the interrupted conversation/branch scope;
2. bridge preflight compares the submitted conversation/branch scope with the current interrupted turn; a stale scope is rejected before execution;
3. rejection leaves `RecoveryStore` untouched;
4. accepted acknowledgement creates the operation but does not clear recovery state;
5. the first correlated `chat_state=submitting` event is the claim transition after the existing backend `begin_attempt` succeeds;
6. only then does QML clear the interrupted recovery record and navigate to that conversation.

A correlated error before the claim transition fails the operation and preserves recoverable UI state.

## Ownership boundary preserved

Protocol V2 itself owns only the UI protocol contract, bridge-local protocol adapter/state, QML dispatch/event/operation projection, recovery acknowledgement semantics, and the desktop compatibility mode. Protocol types do not own inference, storage, RAG, SaaS, diagnostics, packaging, or business semantics.

The **current merged archive** contains additional hardening packages in those other domains (for example model activation, observability, scoped RAG, V013 SaaS persistence, generic SaaS transport, async bridge coordination, secure bootstrap, and provenance). Those additions remain separate ownership boundaries; their presence does not move their business logic into Protocol V2.
