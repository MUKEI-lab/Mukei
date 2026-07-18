# 01 — Information Architecture

Status: **Draft v0.1**

This document defines where product concepts live, how users navigate between them, and the provisional relationships between chats, workspaces, storage, projects, artifacts, models, and settings.

Data ownership details that remain unresolved MUST be finalized through ADRs before storage/project implementation is considered stable.

## 1. Top-level navigation

The primary Android shell is drawer-based.

```text
Mukei / Home
Storage
Projects
Models
Chats
Settings
```

### 1.1 Mukei / Home

Purpose: begin or resume intent naturally.

Home is an opening state, not a dashboard.

It contains:

- global menu access;
- new-chat action;
- contextual options;
- greeting/prompt;
- composer;
- optional capability chips.

Home MUST NOT become a persistent recent-items dashboard.

### 1.2 Storage

Purpose: user-visible access to durable files stored/managed by Mukei.

Storage includes or references:

- imported files;
- generated files;
- documents;
- images;
- exports;
- artifacts that are represented as durable files.

Storage SHOULD communicate whether content is local and how it is scoped.

### 1.3 Projects

Purpose: organize larger bodies of work over time.

A project may aggregate conversations, workspaces, files, and artifacts. Exact ownership semantics remain an ADR decision.

Projects MUST NOT be treated as a cosmetic folder layer until ownership/lifecycle semantics are explicit.

### 1.4 Models

Purpose: inspect, install, activate, configure, and manage inference models/capabilities.

Model UI SHOULD distinguish:

- installed vs available;
- local vs remote/provider-backed;
- active vs inactive;
- compatible vs unavailable;
- required artifacts missing vs ready.

### 1.5 Chats

Purpose: access conversation history without turning Home into a history list.

Chats MAY be shown as a recent subset in the drawer and SHOULD have a dedicated full-history/search surface when history grows.

### 1.6 Settings

Purpose: durable user controls.

Expected sections:

- Personalization
- Memory
- Appearance
- General
- Privacy
- Storage
- Providers
- Advanced
- About

Settings SHOULD sit at the bottom of the navigation hierarchy and MUST NOT compete with primary creation flows.

## 2. Core user-facing entities

### 2.1 Chat

A chat is the persistent conversation context visible to the user.

A chat contains:

- user prompts;
- Mukei responses;
- operation/activity references;
- workspace references where applicable;
- artifact references where applicable.

A chat MUST remain understandable after process restart; transient runtime events alone are not sufficient history.

### 2.2 Workspace

A workspace is a structured working area for a task/conversation involving files, folders, generated outputs, or multi-step project state.

A workspace is NOT synonymous with Storage.

A workspace SHOULD expose:

- file/folder tree or logical list;
- created/modified/read state where useful;
- artifacts/exports;
- task-level activity summary;
- export/inspect controls.

Provisional relationship:

```text
Chat ── contextualizes ──> Workspace
```

Whether this is exactly one workspace per chat is unresolved and requires ADR.

### 2.3 Universal Storage

Universal Storage is the app-wide durable file surface.

It is conceptually independent from any one chat or workspace.

```text
Universal Storage
├── Imported files
├── Generated files
├── Documents
├── Images
└── Exports / durable artifacts
```

Workspace-specific files MAY appear in Storage through references/views, but scope and ownership MUST remain explicit.

### 2.4 Project

A project is a user-facing organizational unit for longer-lived work.

Provisional model:

```text
Project
├── Chats
├── Workspace(s)
├── Files/references
└── Artifacts
```

Open question: whether Project owns these entities or aggregates/references them.

### 2.5 Activity / Operation

Activity represents visible execution progress.

It is not a destination in the top-level navigation.

Activity appears contextually inside conversation/workspace flows and may expand into detail.

Examples:

- searching;
- reading;
- creating/editing files;
- building;
- exporting;
- model downloading;
- indexing/ingestion.

Activity MUST have stable user-facing states rather than being inferred directly from raw log lines.

### 2.6 Artifact

An artifact is a user-relevant output that is ready to open, download, share, inspect, or save/associate with a project.

Examples:

- ZIP package;
- report;
- generated document;
- image;
- code bundle;
- exported dataset.

An artifact may be backed by a durable file, but the product concept includes metadata and user actions beyond “file exists.”

Exact artifact lifecycle/data model requires ADR.

### 2.7 Model

A model represents an inference capability/configuration available to Mukei.

Model state MAY include:

- available;
- downloading;
- installed;
- verifying;
- active;
- incompatible;
- failed;
- artifact-required.

## 3. Concept boundaries

The following distinctions are mandatory:

```text
Universal Storage ≠ Workspace
Workspace ≠ Project
Project ≠ Chat
Artifact ≠ ordinary file
Activity ≠ conversation message
Model availability ≠ backend readiness
```

### 3.1 Backend ready vs intelligence ready

The app MUST distinguish native runtime readiness from model/capability readiness.

Example:

```text
Backend ready
+ encrypted storage ready
+ model artifacts missing
= app shell/storage/settings may be usable
  but inference-dependent actions are unavailable or gated
```

The UI MUST NOT collapse these into one generic “backend unavailable” state.

## 4. Navigation relationships

Recommended conceptual navigation:

```text
Home
  └─ start/resume → Chat
                    ├─ inspect → Activity
                    ├─ open → Workspace
                    ├─ open/download → Artifact
                    └─ associate → Project

Drawer
  ├─ Storage → file surfaces
  ├─ Projects → project surfaces
  ├─ Models → model management
  ├─ Chats → history/search
  └─ Settings → controls
```

Activity is contextual and SHOULD generally open as an inline expansion, sheet, or detail destination rather than a permanent drawer item.

## 5. Home versus history

Home MUST remain low-friction.

Recent chats/projects SHOULD be reachable through:

- drawer;
- search;
- dedicated Chats/Projects surfaces;
- optional subtle contextual affordances.

A persistent large recent-work grid on Home is out of scope for v0.1.

## 6. Workspace appearance rules

Workspace SHOULD appear when at least one of the following becomes true:

- the task creates persistent files;
- the task modifies files;
- the user explicitly requests a workspace/project;
- the task has a structured multi-file output;
- an artifact requires inspectable source/state.

Workspace SHOULD NOT be created merely because a conversation exists.

## 7. Artifact visibility rules

When a user-relevant output is produced, the product SHOULD surface an ArtifactCard in conversation and make the durable output discoverable from the appropriate Workspace/Storage/Project context.

The user should be able to answer:

1. What was created?
2. Where is it stored?
3. Can I open/download/share/export it?
4. Will it still exist after I leave this chat?

## 8. Provisional drawer structure

```text
Mukei

Storage
Projects
Models

Chats
  Recent chat A
  Recent chat B
  Recent chat C

Settings
```

Rules:

- Mukei navigates to the opening state.
- Settings remains bottom-anchored where layout allows.
- Recent chats MUST NOT dominate the drawer.
- Active destination must be visually clear.
- Navigation state must survive configuration changes.

## 9. Open architecture decisions

Create ADRs for:

- one workspace per chat vs many;
- workspace without chat;
- project ownership vs aggregation;
- artifact identity/lifecycle;
- file reference semantics between workspace and Universal Storage;
- deletion/trash behavior across scopes;
- canonical search scope across Chats/Projects/Storage.
