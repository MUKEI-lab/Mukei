# ADR-003 — Project Ownership vs Aggregation

Status: **Proposed**  
Priority: **Critical before Projects implementation**

## Context

Projects organize long-running work across chats, workspaces, files, and artifacts.

If Project becomes the physical owner of all referenced data, adding/removing items can cause duplicate files and dangerous cascade semantics.

## Proposed decision

**Project is an aggregation/context layer, not a byte-owning storage scope by default.**

A Project stores its own metadata and explicit references to:

```text
Chats
Workspaces
Artifacts
Storage nodes/items where product policy allows
Project-specific settings/context
```

Underlying durable files continue to belong to their Universal/Workspace storage scopes.

## Alternatives considered

### A. Project owns a dedicated storage scope

Pros:

- intuitive folder-container model.

Cons:

- duplicates or moves data when adding to project;
- complex chat/workspace ownership migration;
- dangerous deletion cascades.

### B. Project aggregates references — proposed

Pros:

- no unnecessary byte duplication;
- one artifact can appear in Storage and Project;
- safe chat/project deletion separation;
- supports multiple chats/workspaces.

Cons:

- reference/lifecycle UI must be explicit;
- permissions/context resolution required.

## Consequences

- `project_id` is a contextual/organizational identity.
- Project deletion removes project metadata/references first, not underlying storage by default.
- Explicit `Delete project and selected owned/generated work` may be offered only with clear scope and domain support.
- Mutating operations must still target explicit workspace/storage identities; `project_id` alone is not a file mutation target.

## Migration / compatibility impact

Project schema should use relation tables rather than embedding project ownership into every storage node unless a later accepted ADR changes the model.

## Security / privacy impact

Project membership must not grant implicit cross-workspace mutation permission. Authorization remains based on explicit target scope and allowed relations.

## Product / UX impact

Project detail can present a unified view of chats/files/artifacts without physically relocating them.

`Add to Project` means create an organizational/context reference unless UI explicitly says Copy/Move.
