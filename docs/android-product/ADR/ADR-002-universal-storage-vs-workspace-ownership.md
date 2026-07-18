# ADR-002 — Universal Storage vs Workspace Ownership/Reference Model

Status: **Proposed**  
Priority: **Critical before Storage port**

## Context

Mukei needs both:

- app-wide Universal Storage;
- isolated task-oriented Workspaces.

The same user-visible content may need to appear in both contexts without unsafe implicit sharing or unnecessary byte duplication.

The temp storage prototype already separates logical nodes, file versions, and immutable encrypted objects, which makes explicit copy/reference semantics possible.

## Proposed decision

**Universal Storage and each Workspace are separate logical storage scopes. A logical node belongs to exactly one scope. Cross-scope movement/sharing must be an explicit operation.**

Encrypted immutable `StorageObject` content MAY be deduplicated/referenced internally across logical nodes when lifecycle/reference accounting is safe, but user-visible logical nodes and version histories remain scope-specific.

Product actions are distinct:

```text
Move
- changes logical ownership/location
- source node no longer remains in original location after successful commit

Copy
- creates independent logical node/history in target scope
- source remains

Add/Reference
- organizational reference only when product/domain explicitly supports it
- does not imply shared mutable file node
```

## Alternatives considered

### A. One global file tree with workspace folders

Rejected because workspace isolation/security and lifecycle become implicit path conventions rather than enforced scopes.

### B. Separate scopes with explicit operations — proposed

Pros:

- strong isolation;
- clear deletion/trash semantics;
- explicit user intent;
- compatible with encrypted-object deduplication.

Cons:

- copy/move APIs more explicit;
- reference counting/reclamation complexity.

### C. Duplicate physical bytes for every scope

Simple lifecycle, but wastes storage and undermines immutable-object dedup benefits.

## Consequences

- every `StorageNode` has exactly one `scope_id`;
- parent/child relationships never cross scope;
- import targets explicit scope + directory;
- workspace UI lists only its scope unless explicitly surfacing external references;
- Universal Storage is not a hidden parent of workspaces.

## Migration / compatibility impact

Retain temp prototype same-scope database guards and explicit scope IDs.

Cross-scope operations must be added intentionally rather than implemented by directly rewriting `scope_id` without journaling/authorization.

## Security / privacy impact

This creates a clear security boundary for workspace isolation.

Authorization must fail closed for cross-scope IDs unless the requested operation is explicitly a validated Move/Copy/Reference operation.

## Product / UX impact

UI labels must use explicit verbs:

- `Copy to workspace`
- `Move to Storage`
- `Add to project`

Avoid ambiguous `Add` when it is actually copying bytes/logical ownership.
