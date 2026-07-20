# Android Product ADRs

Architecture Decision Records capture durable decisions that affect product behavior, data ownership, protocol contracts, or implementation structure.

## Status values

- Proposed
- Accepted
- Superseded
- Rejected

## ADR format

Each ADR should contain:

1. Context
2. Decision
3. Alternatives considered
4. Consequences
5. Migration/compatibility impact
6. Security/privacy impact
7. Product/UX impact
8. Status and date/review state where applicable

## Decision review gate

Read [DECISION_REVIEW_v0.1.md](DECISION_REVIEW_v0.1.md) before accepting or modifying individual ADRs.

The review matrix:

- consolidates dependencies and recommended dispositions;
- distinguishes safe non-blocked implementation work from schema/protocol choices that must wait;
- recommends modifying ADR-001 so the durable model does not hard-lock exactly one workspace per chat;
- does **not** change any ADR from `Proposed` by itself.

## Decision queue

| ADR | Decision | Status | Priority |
|---|---|---|---|
| [ADR-001](ADR-001-workspace-cardinality-and-chat-relationship.md) | Workspace cardinality and chat relationship | Proposed | Critical before Workspace implementation |
| [ADR-002](ADR-002-universal-storage-vs-workspace-ownership.md) | Universal Storage vs Workspace ownership/reference model | Proposed | Critical before Storage port |
| [ADR-003](ADR-003-project-ownership-vs-aggregation.md) | Project ownership vs aggregation | Proposed | Critical before Projects |
| [ADR-004](ADR-004-artifact-identity-and-lifecycle.md) | Artifact identity and lifecycle | Proposed | High before Artifact/export implementation |
| [ADR-005](ADR-005-authoritative-state-and-process-recovery.md) | Authoritative state/recovery after process death | Proposed | Critical before Conversation persistence |
| [ADR-006](ADR-006-android-navigation-architecture.md) | Android navigation architecture | Proposed | High before Product Shell implementation |
| [ADR-007](ADR-007-protocol-v2-evolution-and-query-contract.md) | Protocol V2 evolution and bounded query/snapshot contract | Proposed | High before product-domain protocol expansion |
| [ADR-008](ADR-008-temporary-chat-ephemeral-isolation.md) | Temporary Chat ephemeral persistence/RAG isolation | Proposed | Critical before enabling Temporary Chat |

## Recommended review order

```text
ADR-005 State authority
  ↓
ADR-007 Protocol/query evolution
  ↓
ADR-008 Temporary Chat isolation
  ↓
ADR-006 Navigation
  ↓
ADR-001 Workspace relationship
  ↓
ADR-002 Storage/workspace scope semantics
  ↓
ADR-004 Artifact identity
  ↓
ADR-003 Project aggregation
```

The order follows implementation dependencies rather than numerical order.

## Rule

An ADR should be used when changing the decision later would require data migration, protocol migration, major UI restructuring, security review, or substantial code rewrite.

Routine implementation details do not require ADRs.

A `Proposed` ADR is not permission to silently hard-code the recommendation. It becomes normative only after explicit review/acceptance or when superseded by an accepted alternative.
