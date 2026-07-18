# 09 — Implementation Roadmap

Status: **Seed v0.1**

This roadmap sequences Android work as **vertical product slices**. Each milestone must produce a coherent, testable user capability rather than only isolated infrastructure.

## Guiding rules

1. The `Kotlin` branch remains the integration base.
2. Product changes should land behind explicit contracts from this docs set.
3. Each milestone MUST have real-device/emulator acceptance criteria.
4. Backend and UI work should land together where practical.
5. Experimental branch code should be selectively ported, not wholesale merged, when histories have materially diverged.
6. A milestone is not complete merely because CI compiles.

---

# M0 — Secure Android runtime bootstrap

Status: **Baseline achieved; continue hardening**

Current capability:

- app launches;
- secure native runtime can initialize;
- SQLCipher/security status is exposed;
- JNI/native packaging is functional;
- Protocol V2 foundation exists;
- backend/model readiness can be distinguished conceptually.

Remaining hardening:

- replace diagnostic bootstrap UI with product readiness states;
- preserve specific stable startup error codes through JNI/Kotlin/UI;
- add device boot smoke test to CI/release acceptance;
- ensure signed APK installability is verified through official Android tooling.

Exit criterion:

> Cold install → launch → backend/storage readiness resolved on supported device/emulator without manual debugging.

---

# M1 — Product shell and design-system foundation

## User value

Mukei opens as the intended product instead of a backend status screen.

## Scope

- Home/opening screen;
- navigation drawer;
- top bar actions;
- base navigation graph;
- design tokens from UI/UX Blueprint v0.1;
- typography/spacing/shapes/color primitives;
- core icon conventions;
- startup/readiness presentation;
- reduced-motion and basic accessibility foundations.

## Components

- `MukeiScaffold`
- `MukeiTopBar`
- `NavigationDrawer`
- `GreetingBlock`
- `PromptComposer` shell
- `CapabilityChipRow`
- readiness/error surface

## Exit criteria

- cold launch reaches Home when shell is usable;
- drawer destinations are navigable;
- backend/model readiness is truthful and non-blocking where possible;
- TalkBack labels exist for primary actions;
- rotation/recomposition does not lose navigation state.

---

# M2 — Conversation MVP

## User value

The user can start and continue a real conversation from Home.

## Scope

- create/resume chat;
- send user prompt;
- render Mukei response;
- hybrid streaming/event updates;
- stop/cancel;
- retry basic failures;
- chat persistence/history projection;
- scroll-follow behavior;
- empty conversation state;
- basic chat options: rename/delete/find can be phased.

## Backend contract work

Map UI to existing Protocol V2 commands/events/snapshots and identify missing APIs.

At minimum validate:

- initialize;
- send message;
- stop generation;
- ordered event consumption;
- authoritative conversation snapshot/recovery.

## Exit criteria

- Home → send → response works end-to-end with a configured capability;
- stop is deterministic;
- process recreation does not duplicate messages;
- process restart restores truthful conversation state;
- inference-unavailable state routes clearly to Models/provider setup rather than generic backend failure.

---

# M3 — Activity and operation visibility

## User value

Long-running work is understandable and controllable.

## Scope

- `ActivityCard` collapsed/expanded;
- grouped operation categories;
- searching/reading/writing/building/packaging states;
- cancellation and failure UI;
- stable operation IDs/state reconciliation;
- approval UI hooks where required.

## Exit criteria

User can answer during a long task:

1. What is happening?
2. Is it still running?
3. Can I stop it?
4. What failed if it stops?

No raw log console is required for normal use.

---

# M4 — Universal Storage foundation

## User value

Imported and generated files have a durable, understandable home.

## Strategy

Selectively port/reconcile useful storage work from `temp/universal-storage-workspace-v0.1` onto current `Kotlin` rather than direct-merging the divergent branch.

## Scope

- Universal Storage domain model;
- encrypted object/file persistence as accepted by architecture review;
- import transaction lifecycle;
- Android document picker/staging integration;
- file metadata/versioning where required;
- trash/recovery semantics;
- Storage screen MVP;
- explicit scope/isolation rules.

## Required prior decision

`07_STORAGE_WORKSPACE_MODEL.md` + ADRs for ownership/deletion/scope.

## Exit criteria

- import file from Android picker;
- survive process death during/after import;
- imported file appears in Storage;
- deletion/trash behavior is deterministic;
- no cross-workspace/scope leakage;
- encryption/status invariants validated.

---

# M5 — Workspace vertical slice

## User value

Structured work becomes tangible inside conversation and a dedicated workspace view.

## Scope

- workspace lifecycle;
- WorkspaceCard in conversation;
- workspace screen;
- file/folder presentation;
- created/edited/imported/read states;
- workspace activity history;
- chat ↔ workspace relationship;
- file preview basics.

## Required prior decision

ADR: one workspace per chat vs multiple/attachable workspaces.

## Exit criteria

A file-producing task can:

```text
Chat request
→ create/open workspace
→ create/edit files
→ show WorkspaceCard
→ inspect files
→ leave app
→ return and recover same durable workspace
```

---

# M6 — Artifacts and export

## User value

Mukei delivers usable outputs, not merely messages.

## Scope

- artifact identity/metadata;
- ArtifactCard;
- ZIP/report/document export;
- open/download/share flows;
- Storage/workspace discoverability;
- export confirmation and failure recovery.

## Exit criteria

User can answer:

- what was created;
- where it is stored;
- how to retrieve it later;
- what export/share changed.

---

# M7 — Models product surface

## User value

The user can make inference capability ready without technical guesswork.

## Scope

- Models screen;
- installed/available states;
- compatibility/storage estimates;
- download/verify/activate flows;
- local vs remote/provider distinction;
- artifact-required state resolution;
- model selection.

## Exit criteria

Fresh user can move from “model artifacts required” to an active usable model through product UI with clear storage/compatibility feedback.

---

# M8 — Projects

## User value

Long-lived work can be organized across chats, workspaces, files, and artifacts.

## Required prior decision

Project ownership/aggregation ADR.

## Scope

- project list/detail;
- create/rename/delete;
- add chat to project;
- link/associate workspace/artifacts/files;
- visible active project context;
- continue-work flow.

## Exit criteria

User can resume a project and clearly understand what context Mukei will use before new work begins.

---

# M9 — Settings, personalization, privacy controls

## Scope

- Personalization;
- Memory;
- Appearance;
- Privacy;
- Storage;
- Providers;
- Advanced;
- About.

Settings implementation should follow actual backend capability rather than creating nonfunctional toggles.

---

# M10 — Release hardening

## Scope

- full accessibility pass;
- performance/startup profiling;
- process-death/restart matrix;
- offline behavior;
- storage corruption/recovery cases;
- signed build/install/update validation;
- ABI/device matrix;
- migration compatibility;
- destructive-action tests;
- privacy/provider disclosure review.

## Release gate

A release candidate MUST include installable signed artifacts verified through official Android tooling and an automated/manual device smoke-test record.

---

# Immediate next work order

Recommended sequence from the current baseline:

```text
1. Finalize Screen Specs for Home + Shell
2. Create Design System doc/tokens mapping
3. Create Interaction State Model for readiness + conversation
4. Create UI ↔ Backend Contract for Conversation MVP
5. Implement M1 Product Shell
6. Implement M2 Conversation MVP
7. Resolve Storage/Workspace ADRs before M4/M5
```

Do not begin broad Projects implementation before the storage/workspace ownership model is settled.
