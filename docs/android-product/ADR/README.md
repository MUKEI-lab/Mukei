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
8. Status and date

## Initial decision queue

| ADR | Decision | Priority |
|---|---|---|
| ADR-001 | Workspace cardinality and chat relationship | Critical before Workspace implementation |
| ADR-002 | Universal Storage vs Workspace ownership/reference model | Critical before Storage port |
| ADR-003 | Project ownership vs aggregation | Critical before Projects |
| ADR-004 | Artifact identity and lifecycle | High before Artifact/export implementation |
| ADR-005 | Authoritative state/recovery after process death | Critical before Conversation persistence |
| ADR-006 | Android navigation architecture | High before Product Shell implementation |
| ADR-007 | Protocol V2 contract evolution/versioning rules | High before adding workspace/storage commands |

## Rule

An ADR should be used when changing the decision later would require data migration, protocol migration, major UI restructuring, security review, or substantial code rewrite.

Routine implementation details do not require ADRs.
