# Mukei Android Product Specification

Status: **Draft v0.1**  
Target branch: **Kotlin**  
Primary visual source: **Mukei UI/UX Blueprint v0.1**

This directory converts the UI/UX blueprint into implementation-grade product contracts for the Android application.

The blueprint remains the visual and experiential reference. These documents define behavior, ownership, state, integration boundaries, delivery order, and acceptance criteria.

## Normative language

- **MUST**: required for the product contract.
- **MUST NOT**: prohibited behavior.
- **SHOULD**: expected unless a documented reason requires deviation.
- **MAY**: optional or implementation-dependent.

## Document map

| Document | Purpose | Status |
|---|---|---|
| `00_PRODUCT_VISION.md` | Product identity, principles, non-goals, trust contract | Drafted |
| `01_INFORMATION_ARCHITECTURE.md` | Top-level surfaces and domain relationships visible to users | Drafted |
| `02_UI_UX_FLOWS.md` | End-to-end user journeys, including failure/cancellation/recovery | Drafted |
| `03_SCREEN_SPECIFICATIONS.md` | Per-screen behavioral contracts and acceptance signals | Drafted |
| `04_DESIGN_SYSTEM.md` | Compose-oriented semantic tokens, primitives, component and motion rules | Drafted |
| `05_INTERACTION_STATE_MODEL.md` | Explicit UI/domain/runtime state machines and recovery rules | Drafted |
| `06_UI_BACKEND_CONTRACT.md` | Compose action ↔ Protocol V2/Rust mapping and missing APIs | Next |
| `07_STORAGE_WORKSPACE_MODEL.md` | Universal Storage, workspace, project, artifact ownership/lifecycle | Next |
| `08_ANDROID_ARCHITECTURE.md` | Kotlin modules, state ownership, repositories, navigation | Planned |
| `09_IMPLEMENTATION_ROADMAP.md` | Vertical slices and milestone exit criteria | Seeded |
| `10_TEST_ACCEPTANCE_PLAN.md` | Device-level acceptance and regression matrix | Planned |
| `ADR/` | Architecture Decision Records for durable decisions | Seeded |

## Source hierarchy

When requirements disagree, use this order until an ADR changes it:

1. Security/privacy invariants and data-integrity requirements.
2. Accepted ADRs.
3. This Android product specification.
4. Mukei UI/UX Blueprint v0.1.
5. Existing implementation behavior.

Existing behavior is **not** automatically the desired product contract.

## Product architecture rule

> UX defines the product contract; architecture must satisfy that contract without compromising security, correctness, privacy, recoverability, or maintainability.

The UI must not dictate unsafe persistence or lifecycle behavior. Conversely, backend constraints should not leak into the user experience as avoidable technical complexity.

## Current implementation baseline

The Kotlin Android line currently has a functioning secure native bootstrap, Protocol V2 foundation, SQLCipher-backed runtime initialization, JNI gateway, and Jetpack Compose shell. The current visible app is still primarily a backend/bootstrap status surface rather than the intended product UI.

The next implementation phase is therefore the **product shell and conversation vertical slice**, not another standalone infrastructure phase.

## Working method

Each feature should be implemented through this traceability chain:

```text
Product principle
    ↓
Information architecture
    ↓
User flow
    ↓
Screen + interaction state
    ↓
UI/backend contract
    ↓
Android architecture
    ↓
Acceptance tests
```

A feature is not complete merely because a screen renders or a backend API exists. It is complete when the end-to-end user flow meets its acceptance criteria on a real device/emulator.

## Current specification dependency chain

```text
00 Product Vision
  ↓
01 Information Architecture
  ↓
02 UI/UX Flows
  ↓
03 Screen Specifications ─┐
04 Design System          ├─→ 06 UI/Backend Contract
05 Interaction State     ┘          ↓
                         07 Storage/Workspace Model
                                  ↓
                         08 Android Architecture
                                  ↓
                         09 Implementation Roadmap
                                  ↓
                         10 Test Acceptance Plan
```

`06` and `07` are the next critical documents because they determine which parts of the current Protocol V2/runtime are already usable and which domain APIs must be added before the blueprint can be implemented truthfully.

## Initial decisions to resolve via ADR

1. Is there exactly one workspace per chat, or can a chat attach multiple workspaces?
2. Can a workspace exist without a chat?
3. Is a project an aggregation layer over chats/workspaces/files, or an owning container?
4. What distinguishes an artifact from an ordinary generated file at the data-model level?
5. Which state is authoritative after process death: Rust snapshot, Kotlin persistence, or a reconciled projection?
6. What is the canonical navigation model: drawer + destinations only, or nested navigation graphs per feature?
7. Which Protocol V2 commands/events are required before the first real conversation MVP can ship?
