# 00 — Product Vision

Status: **Draft v0.1**

## 1. Product identity

**Mukei is a capable companion inside a private, local-first workspace.**

It is not primarily a chatbot, dashboard, file manager, developer console, or cloud SaaS control panel. Conversation is the natural command surface through which the user can ask, research, write, organize, inspect files, build projects, and receive usable outputs.

The intended feeling is:

> Calm enough to think. Alive enough to stay. Warm enough to return.

## 2. North-star experience

The user should feel:

> “Mukei is here with me, and I can start anything from here.”

The experience MUST reinforce four qualities:

1. **Companionship** — present and useful without pretending to be human.
2. **Possibility** — the user can begin naturally without first classifying a task.
3. **Stability** — progress, errors, and long-running work are predictable and controllable.
4. **Trust** — local storage, provider use, file changes, and exports are understandable.

## 3. Primary interaction principle

**The opening screen MUST be an invitation, not a menu.**

The user MUST be able to type a natural request immediately.

The user MUST NOT be forced to select a mode before entering a request.

Capability chips MAY suggest actions such as Research, Build App, Read Files, Write, Code, or Plan, but they remain optional affordances.

Canonical rule:

> **Menu = where things live. Chips = what Mukei can do now. Input = what the user wants.**

## 4. Primary product surfaces

Mukei has four conceptual surfaces.

### 4.1 Conversation

Purpose: intent, clarification, decisions, explanation, results.

Conversation MUST remain the primary thinking surface. Responses SHOULD read like documents or useful explanations rather than dense chat bubbles.

### 4.2 Workspace

Purpose: files, folders, task/project state, generated outputs, structured work.

Workspace SHOULD appear when a task creates or manipulates tangible work. It MUST NOT be forced into every conversation.

### 4.3 Activity

Purpose: visible progress for searches, reads, edits, builds, exports, downloads, and other multi-step operations.

Activity MUST reduce black-box anxiety without exposing a console by default.

The default view SHOULD summarize high-level progress. Detailed operations SHOULD be available on demand.

### 4.4 Controls

Purpose: stop, pause where supported, retry, approve, undo where supported, inspect, export, and recover.

Controls MUST be available when the user can meaningfully affect an operation, but MUST NOT crowd ordinary reading/conversation states.

## 5. Local-first trust contract

Trust is a product feature, not only a backend property.

The UI SHOULD communicate relevant facts with calm, contextual language such as:

- “Local workspace”
- “Stored on this device”
- “Uses your configured provider”
- “No account required”
- “Export ready”
- “This file stays in your workspace unless you share it”

The product MUST NOT imply that all processing is on-device when a configured remote provider or network tool is involved.

The product MUST make destructive file/data actions explicit and recoverable where the underlying model supports recovery.

The product MUST summarize meaningful file changes rather than silently modifying user work.

## 6. Power visibility

Mukei may perform real work such as:

- searching;
- reading files;
- editing files;
- creating files/projects;
- running structured tasks;
- packaging/exporting outputs;
- installing/activating models;
- invoking future tools/plugins/providers.

The UI SHOULD expose this work progressively.

### Default layer

Human-readable state, for example:

- “I’ll check a few reliable sources first.”
- “I’m creating the project structure.”
- “Your files are ready.”

### Expanded layer

When requested, show structured details such as:

- searches performed;
- files read;
- files created/edited;
- exports produced;
- provider/tool used;
- approvals or failures.

Detailed activity MUST remain structured product UI, not raw log output.

## 7. Home experience contract

The empty/opening state MUST prioritize:

1. quiet top bar;
2. warm greeting/prompt;
3. prominent composer;
4. optional capability chips.

It MUST NOT default to:

- a feature dashboard;
- recent workspace grid;
- marketing/brand billboard;
- project list;
- mandatory onboarding wizard unless truly required for operation.

The composer is the primary action.

## 8. Product language principles

Primary UI SHOULD avoid calling Mukei an “AI assistant.”

Copy SHOULD be:

- direct;
- calm;
- specific;
- non-blaming;
- honest about limitations.

Errors SHOULD answer:

1. What failed?
2. What is still safe/saved?
3. What can the user do next?

Example pattern:

> “I couldn’t finish the build. The files created so far are still saved in your workspace. You can retry, inspect them, or export the current version.”

## 9. Anti-patterns

The Android product MUST avoid:

- mandatory mode selection before typing;
- first-launch feature grids;
- excessive brand repetition;
- decorative “AI” visuals that obscure functionality;
- glowing/neon visual language inconsistent with the blueprint;
- raw console logs as the default activity UI;
- hidden file mutations without summaries;
- technical backend error codes as primary user-facing error copy;
- loading states with no cancellation/control for long-running cancellable tasks;
- presenting “ready” when required model/artifact dependencies are unavailable.

## 10. Definition of product success

Mukei succeeds when a user can begin with natural intent and move smoothly from thought to tangible result:

```text
Intent
  → Conversation
  → Visible work when needed
  → Workspace/files when needed
  → Usable artifact/result
  → Clear persistence/export/recovery
```

The interface SHOULD recede during focus and become more explicit when guidance, progress, reassurance, or control is required.

## 11. Engineering interpretation

The product MUST be implemented as end-to-end vertical slices.

Backend capability without usable UX is incomplete.

UI without durable state, recovery, and backend contracts is incomplete.

CI compilation alone is not sufficient release evidence; critical flows MUST be exercised through real Android runtime acceptance tests.
