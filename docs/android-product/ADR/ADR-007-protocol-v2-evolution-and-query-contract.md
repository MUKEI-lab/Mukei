# ADR-007 — Protocol V2 Evolution and Query/Snapshot Contract

Status: **Proposed**  
Priority: **High before adding Storage/Workspace product APIs**

## Context

Protocol V2 already provides versioned command envelopes, acknowledgements, ordered events, capability negotiation, bounded event drains, idempotency, and snapshots.

The current fixed snapshot domains are limited to application/settings/protocol/operations, while the product requires bounded authoritative projections for conversations, models, storage, workspaces, artifacts, and projects.

Simply adding large `all_*` snapshot payloads risks unbounded JSON, weak pagination, and difficult compatibility evolution.

## Proposed decision

Keep **Protocol major version 2** for backward-compatible additive evolution and introduce a **versioned bounded query/snapshot contract** for product-domain reads.

### Compatibility rules

A Protocol major bump is required for incompatible changes to:

- envelope structure/meaning;
- required correlation/identity semantics;
- existing command/event payload semantics that cannot be consumed safely by older compatible clients;
- removal/redefinition of required capabilities.

Additive compatible changes MAY remain Protocol 2 with minor/capability evolution:

- new commands;
- new optional event types;
- new optional fields with defined defaults;
- new query domains/selectors;
- new capability strings;
- new projection schema versions.

Capability negotiation is authoritative. Version number alone does not imply a feature exists.

## Query/snapshot model

Introduce a bounded request/response concept similar to:

```text
QueryRequestV2
- protocol_version
- request_id
- domain
- selector?
- cursor?
- limit?
- projection_schema_version?

QueryResponseV2
- protocol_version
- runtime_session_id
- domain
- schema_version
- generated_at
- items/projection
- next_cursor?
- has_more
```

Exact wire names remain implementation design work.

Domains may include:

```text
chat_index
conversation
models
storage
workspace
artifact
project
```

Selectors use stable opaque IDs and explicit scope.

## Why not only expand SnapshotDomainV2?

Expanding fixed domains is acceptable for small singleton projections, but domains such as all chats/files can exceed bounded transport limits and need pagination/selectors.

A hybrid model is allowed:

- fixed singleton snapshots for application/settings/protocol/operations;
- bounded query projections for collections/entity detail.

## Event evolution rules

- event types are stable machine identifiers;
- existing event semantic meaning must not be silently changed;
- payload schema changes are versioned/additive;
- unknown optional event types may be ignored only when capability/schema rules permit safe forward compatibility;
- required unknown events or sequence gaps trigger reconciliation/failure, not guessing.

## Command evolution rules

- mutation commands use explicit IDs/scopes;
- user-visible duplicate effects require idempotency keys;
- commands advertise capabilities only when genuinely usable end-to-end;
- deprecated commands remain supported for a documented compatibility window or require a major protocol migration.

## Scope evolution

Current scope contains conversation/branch/turn/model/document IDs.

Storage/workspace/project/artifact mutations require explicit additional identities or a versioned successor scope structure.

Adding optional scope fields is compatible only when old peers can safely ignore them. Commands whose security depends on new scope fields must be capability-gated and rejected by peers that do not understand them.

## Projection schema versioning

Each domain projection has its own schema version independent from Protocol major/minor.

Rules:

- repositories validate schema version;
- additive optional fields can preserve schema where safe;
- incompatible projection changes increment domain schema version;
- migrations/adapters may support multiple recent projection schemas where required.

## Error contract

Protocol should preserve stable machine codes and safe structured context:

```text
code
domain
retryability?
preserved_work?
details/reference?
```

Human copy remains Kotlin/product-layer responsibility.

Do not flatten distinct native failures irretrievably into one generic transport string.

## Alternatives considered

### A. Put all product state into event replay

Rejected: process death/event gaps make recovery expensive and fragile.

### B. Add one giant snapshot per domain

Simple but does not scale for chats/files/projects and conflicts with bounded transport.

### C. Bounded query/snapshot + incremental events — proposed

Provides authoritative recovery, pagination, entity detail, and incremental responsiveness.

## Consequences

- `:core:protocol` gains typed query models/codecs;
- native gateway gains bounded query call(s) or equivalent snapshot selector API;
- repositories use queries for initial load/recovery and events for incremental updates;
- protocol size limits remain enforceable;
- feature capability strings can represent complete vertical contracts.

## Migration / compatibility impact

Existing command/event/snapshot APIs continue during migration.

Conversation MVP should implement the query mechanism first for `chat_index`/`conversation` and model readiness, then Storage/Workspace reuse the same bounded infrastructure.

## Security / privacy impact

Queries validate selector/scope authorization and return only the requested bounded projection.

Do not expose internal filesystem paths, keys, secrets, or unrestricted whole-database dumps through generic query APIs.

## Product / UX impact

Screens can restore truthful durable state after process death without waiting for event replay or loading unbounded collections.

Pagination should remain invisible to the user except normal progressive list loading.
