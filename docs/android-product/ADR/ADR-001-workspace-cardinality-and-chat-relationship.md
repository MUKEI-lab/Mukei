# ADR-001 — Workspace Cardinality and Chat Relationship

Status: **Proposed**  
Priority: **Critical before Workspace implementation**

## Context

The UI/UX blueprint treats Conversation and Workspace as distinct surfaces. A conversation can produce structured work, while a workspace contains files, artifacts, and task state.

The experimental `temp/universal-storage-workspace-v0.1` branch currently assumes exactly one workspace per chat and enforces one active workspace scope per `owner_chat_id`.

That model is simple, but it may constrain future use cases:

- one chat producing multiple independent projects/workspaces;
- a Project creating a workspace before a chat exists;
- multiple chats collaborating on one workspace;
- chat branches that should share or fork workspace state;
- deleting a chat while retaining valuable workspace files.

## Proposed decision

**Workspace is an independent first-class entity with a stable `workspace_id`. Chats reference workspaces explicitly.**

For the initial v0.1 UX, the product MAY auto-create and present one **primary workspace** for a chat when structured work begins, preserving the simple experience.

However, persistence/protocol architecture MUST NOT permanently encode `workspace_id = function(chat_id)` or a database uniqueness rule that makes multiple/reference relationships impossible without migration.

Recommended relationship model:

```text
Chat ── references ──> Workspace
Project ── references ──> Workspace

A Chat may have 0..N workspace references.
A Workspace may have 0..N chat references subject to product policy.
```

The first release MAY enforce a product policy of at most one primary workspace per chat while retaining an extensible data model.

## Alternatives considered

### A. Exactly one workspace owned by one chat

Pros:

- simplest implementation;
- straightforward isolation;
- easy default UI.

Cons:

- chat deletion/cascade becomes dangerous;
- difficult multi-chat projects;
- difficult branching semantics;
- later migration likely.

### B. Workspace independent and referenced by chats — proposed

Pros:

- flexible lifecycle;
- explicit identities/scopes;
- safer project aggregation;
- chat deletion can be independent.

Cons:

- relation table/policy needed;
- more explicit authorization checks;
- UI must identify active workspace target.

### C. Workspace owned only by Project

Rejected as universal rule because casual structured work may exist before a Project is created.

## Consequences

- `workspace_id` is always explicit.
- Workspace storage scope ownership is independent from display/chat relationship.
- Protocol mutations targeting workspace must carry explicit workspace/scope identity.
- A relation/projection defines primary/current workspace per chat when needed.
- UI may still show one smart WorkspaceCard by default.

## Migration / compatibility impact

When porting temp branch storage schema:

- do not blindly preserve `storage_one_workspace_per_chat` as irreversible domain constraint;
- either omit/replace the unique constraint or treat it as temporary schema policy with a planned migration path;
- retain `owner_chat_id` only if its semantics are clarified (creator/origin vs exclusive owner).

## Security / privacy impact

Independent references increase authorization complexity.

Every operation must validate:

- caller/chat/project context;
- explicit workspace ID;
- allowed relationship;
- storage scope isolation.

Possession of a workspace ID alone must not bypass policy.

## Product / UX impact

Initial UX remains simple:

```text
Conversation
  → structured work begins
  → primary WorkspaceCard appears
```

Future UX can support multiple workspaces/projects without rewriting the storage identity model.

## Open questions

- Can multiple chats actively mutate one workspace concurrently?
- Does chat branching share workspace by default or fork explicitly?
- What relation survives when a chat is deleted?
- How is `primary workspace` selected when multiple references exist?
