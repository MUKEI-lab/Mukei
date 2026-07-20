# Android Product ADR Decision Review — v0.1

Status: **Review recommendation; does not change ADR status**

This document consolidates ADR-001 through ADR-007 into one decision gate before implementation reaches expensive-to-reverse storage/workspace/project/protocol choices.

All ADRs remain **Proposed** until explicitly accepted or modified.

## Executive recommendation

| ADR | Recommended disposition | Blocking milestone | Core rationale |
|---|---|---|---|
| ADR-001 | Accept with modification | M5 Workspace | Do not hard-lock one-workspace-per-chat; model workspace as stable independent entity with explicit chat relationship |
| ADR-002 | Accept direction | M4 Storage | Universal Storage and Workspace are distinct scopes; cross-scope reuse must be explicit reference/copy semantics |
| ADR-003 | Accept direction | M8 Projects | Project should aggregate/contextualize durable entities rather than own duplicate file bytes |
| ADR-004 | Accept direction | M6 Artifacts | Artifact should be semantic identity over durable storage versions/bundles, not a second byte-storage system |
| ADR-005 | Accept direction | M2 Conversation persistence | Durable Rust/DB projection is authoritative; runtime transient state and Kotlin UI state reconcile around it |
| ADR-006 | Accept direction | M1B Product Shell | Single-Activity Compose navigation with typed top-level/detail routes fits current Android architecture and minimizes shell rewrite |
| ADR-007 | Accept direction | M2A query contract / M4 protocol | Keep Protocol V2 additive; add bounded typed query/snapshot contracts rather than using events as the only read model |

---

# ADR-001 — Workspace cardinality and chat relationship

## Recommended decision

A `Workspace` SHOULD be a stable durable entity with its own identity.

A chat MAY have:

- no workspace;
- one primary/current workspace relationship for simple UX;
- additional explicitly attached/referenced workspaces if future product flows require them.

The database MUST NOT encode an irreversible global invariant of exactly one workspace per chat unless product evidence later proves that invariant is required.

## Why

The blueprint treats Workspace as structured persistent work, while Projects can span chats/files/artifacts. Hard-coding one workspace per chat would make later project/context composition and multi-stage work expensive to migrate.

The useful temp storage branch currently enforces one active workspace per chat at domain/schema level. That constraint should be removed or generalized during selective port rather than copied unchanged.

## Near-term UX simplification

M5 MAY still present a simple rule:

> A conversation has at most one **primary visible workspace** by default.

That is a UX policy, not necessarily the durable data cardinality constraint.

---

# ADR-002 — Universal Storage vs Workspace ownership/reference model

## Recommended decision

Maintain two explicit storage-scope classes:

```text
Universal Storage
Workspace scope
```

Logical file nodes belong to exactly one scope. Encrypted immutable object bytes MAY be deduplicated/referenced internally when security and lifecycle rules permit, but user-visible ownership, deletion, authorization, and discoverability are scope-specific.

Cross-scope operations MUST be explicit:

- copy into scope;
- move where semantically valid;
- attach/reference without changing ownership where supported.

They MUST NOT silently re-parent nodes across scope boundaries.

## Security consequence

Same-scope parent/import guards from the temp branch are valuable and should be preserved/generalized.

---

# ADR-003 — Project ownership vs aggregation

## Recommended decision

A `Project` is primarily an aggregation/context entity.

It groups or references:

- chats;
- workspaces;
- files/storage nodes;
- artifacts;
- optional project metadata/instructions.

Project deletion MUST NOT automatically destroy referenced durable content unless the user explicitly selects a destructive cascade and the ownership model permits it.

## Why

Making Project a byte-owning storage root would duplicate/conflict with Universal Storage and Workspace scope semantics and complicate moving work between projects.

---

# ADR-004 — Artifact identity and lifecycle

## Recommended decision

An `Artifact` is a semantic deliverable identity backed by one or more durable storage versions/nodes.

Examples:

- a generated report PDF;
- a ZIP bundle;
- a final code export;
- a research document intended as a user-facing deliverable.

Artifact metadata should include stable identity, display name/type, backing storage identities, creation operation/context, and export state/history where useful.

Exporting/sharing MUST NOT silently delete the internal durable artifact.

## Why

This avoids a second file-byte system while allowing UI/product semantics that ordinary generated files do not have.

---

# ADR-005 — Authoritative state and process recovery

## Recommended decision

Use layered authority:

```text
Durable database/Rust projection
        ↓ authoritative persistent truth
Live Rust runtime snapshot/events
        ↓ authoritative transient operation truth while session is alive
Kotlin repositories
        ↓ reconciled typed projection/cache
ViewModel / Compose state
        ↓ presentation + drafts only
```

After process death, the previous Compose rendering is never authoritative.

Active operations restored from durable state must be reconciled against the new runtime session. Interrupted operations become truthful recovered terminal/interrupt states unless a backend-supported resume/regenerate path exists.

## Consequence

M2 requires bounded authoritative conversation/query projections; event replay alone is insufficient as the product read model.

---

# ADR-006 — Android navigation architecture

## Recommended decision

Use a single Android `ComponentActivity` with Compose navigation.

Top-level destinations:

- Home / new conversation entry;
- Chats;
- Storage;
- Projects;
- Models;
- Settings.

Typed detail routes should carry stable entity IDs rather than entire mutable objects:

- conversation ID;
- workspace ID;
- project ID;
- storage node ID;
- model ID;
- artifact ID.

Navigation state is UI state; durable entity state remains repository-owned.

## Back behavior

Deterministic priority:

1. dismiss transient dialog/sheet/menu;
2. close drawer;
3. pop detail route;
4. leave app only from top-level root according to Android conventions.

---

# ADR-007 — Protocol V2 evolution and query contract

## Recommended decision

Keep Protocol major version `2` while making additive compatible changes when older consumers can safely ignore unknown capabilities/fields/event types.

Introduce typed bounded read/query contracts in addition to mutations/events.

Minimum early query domains:

```text
app/readiness
chat index
conversation detail
operations
model readiness/inventory subset
```

Later:

```text
storage scopes/list/detail
workspace detail
project detail
artifact detail
```

## Rules

- queries/snapshots are bounded;
- pagination/selectors are explicit;
- projection schema versions are independent from transport protocol version where practical;
- capability negotiation advertises available commands/query domains;
- event streams provide incremental updates, not the only durable read source;
- command/query payloads are typed/validated at the Kotlin↔Rust boundary;
- unknown required capability fails closed with stable machine error.

---

# Implementation gate result

## Safe to start now

The following work does not depend on final ADR-001..004 acceptance and may proceed:

- M1A typed runtime readiness;
- typed Protocol V2 command/ack/event codec;
- runtime session/sequence tracking;
- application composition root/runtime coordinator;
- M1B shell/navigation scaffolding following ADR-006 direction, provided route contracts stay typed and storage/workspace details remain deferred.

## Must wait for explicit ADR decision

- database migration that fixes workspace cardinality;
- storage schema selective port that encodes ownership/cascade semantics;
- Project durable schema;
- Artifact durable schema;
- broad workspace API surface.

## Recommended review outcome

Accept ADR-002 through ADR-007 substantially as proposed, and accept ADR-001 only after replacing **exactly one workspace per chat** with a more flexible durable relationship model while retaining a simple primary-workspace UX for v0.1.
