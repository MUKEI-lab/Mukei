# ADR-006 — Android Navigation Architecture

Status: **Proposed**  
Priority: **High before Product Shell implementation**

## Context

The product blueprint defines a stable drawer hierarchy for persistent areas while also requiring contextual detail screens such as Conversation, Workspace, Project, Model details, Settings subsections, and transient sheets/dialogs.

Navigation must remain predictable under:

- process/Activity recreation;
- deep links;
- long-running operations that continue while user navigates elsewhere;
- modal drawer/sheets/dialogs;
- top-level destination switching;
- typed entity IDs and recovery.

## Proposed decision

Use a **single-Activity Compose Navigation architecture** with:

1. a stable set of top-level destinations exposed by the drawer;
2. typed nested/detail routes for entity screens;
3. transient UI surfaces modeled separately from destination identity;
4. navigation state independent from backend operation lifecycle.

Recommended top-level destinations:

```text
Home
Storage
Projects
Models
Settings
```

Chats are reachable through conversation routes/recent chat navigation and may remain represented in the drawer as a section/list rather than requiring a standalone dashboard destination in v0.1.

Typed detail routes include:

```text
Conversation(chatId, branchId?)
Workspace(workspaceId)
Project(projectId)
ModelDetails(modelId)
SettingsSection(sectionId)
```

## Drawer behavior

- Drawer is navigation UI, not a separate destination.
- Selecting the currently active top-level destination closes the drawer without duplicating back-stack entries.
- Selecting another top-level destination uses top-level navigation semantics and SHOULD restore meaningful previous state where Compose Navigation save/restore supports it safely.
- Long chat lists must not become the canonical navigation stack.

## Back behavior

Order of precedence:

```text
Dialog/sheet/search mode open
  → close transient surface
Drawer open
  → close drawer
Detail route
  → navigate to logical parent/back stack
Top-level root
  → normal Android system back behavior
```

Navigating Back MUST NOT implicitly cancel a running model/chat/storage operation unless that operation contract explicitly defines navigation-triggered cancellation.

## Alternatives considered

### A. Multiple Activities per feature

Rejected for v0.1 because it complicates shared runtime/repository state, transitions, and product-shell consistency without clear benefit.

### B. One flat navigation graph with string routes only

Simple initially but increases argument/type errors and makes entity/context restoration fragile.

### C. Single Activity + typed top-level/detail route structure — proposed

Balances Android conventions, Compose architecture, process recovery, and product hierarchy.

## Consequences

- `:app` owns the root NavHost/composition root.
- Feature modules expose route registration/navigation contracts rather than directly controlling global navigation.
- Route arguments carry stable IDs, not whole mutable objects.
- ViewModels load authoritative data by ID from repositories.
- Modal UI state does not become durable domain/navigation truth.

## Migration / compatibility impact

Current `MainActivity` bootstrap surface becomes the root product shell/NavHost after runtime readiness integration.

Existing backend runtime singleton/coordinator remains process-scoped and does not follow Activity destination lifecycle.

## Security / privacy impact

Deep links/route arguments containing entity IDs still require repository/backend authorization. Knowing a workspace/project ID does not authorize access.

Sensitive data must not be embedded directly in route strings/saved state when stable opaque IDs are sufficient.

## Product / UX impact

- Drawer remains a calm app map.
- Conversation/Workspace/Project navigation feels contextual rather than like switching between disconnected mini-apps.
- Back behavior is deterministic.
- Running work can continue while the user inspects another screen when backend semantics allow it.

## Open questions

- Exact top-level back-stack save/restore policy per destination.
- Whether Chats gets a dedicated top-level index destination in v0.1 or remains a drawer section + search/recent route.
- Deep-link policy before external/public links exist.
