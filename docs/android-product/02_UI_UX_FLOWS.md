# 02 — UI/UX Flows

Status: **Draft v0.1**

This document defines end-to-end user journeys. It is intentionally behavioral rather than pixel-specific.

Each flow should later map to:

- screen specifications;
- interaction state machines;
- Protocol V2/backend contracts;
- real-device acceptance tests.

## Flow notation

- **Entry**: how the user reaches the flow.
- **Preconditions**: system requirements before the flow can proceed.
- **Main flow**: expected happy path.
- **Branches**: cancellation, unavailable capability, error, or recovery paths.
- **Exit**: durable state after completion.

---

# F01 — App launch and readiness

## Goal

Open Mukei into a usable product surface without exposing backend implementation details as the primary experience.

## Entry

User launches the app.

## Main flow

```text
Process start
  ↓
Application initializes secure runtime
  ↓
UI shows bounded startup state if initialization is not immediate
  ↓
Runtime capabilities are resolved
  ↓
Home opens
```

## Readiness model

The UI MUST distinguish at least:

1. app shell ready;
2. secure backend/runtime ready;
3. encrypted storage ready;
4. inference/model capability ready;
5. network/provider capability ready where applicable.

Example valid state:

```text
Backend ready
Encrypted storage ready
Model artifacts missing
```

This MUST NOT be represented as a total app failure.

## Branch — startup failure

If secure runtime bootstrap fails:

- show a human-readable recovery state;
- retain a stable diagnostic code in expandable details;
- offer Retry when retry is safe;
- offer diagnostic/export guidance where supported.

Raw internal error strings MUST NOT be the only user-facing content.

## Exit

Home or a recoverable startup error surface.

---

# F02 — First launch / start naturally

## Goal

The user understands that they can begin freely without selecting a mode.

## Main flow

```text
Home
  ↓
Greeting + “What’s on your mind?”
  ↓
Primary composer visible
  ↓
Optional capability chips visible
  ↓
User types natural request
  ↓
Chat is created/resumed when request is submitted
```

## Requirements

- Home MUST NOT be a feature dashboard.
- User MUST NOT choose a capability before typing.
- Capability chips MAY alter context/placeholder but MUST remain optional.
- Recent projects/workspaces MUST NOT dominate the opening state.

## Exit

Conversation flow.

---

# F03 — Casual conversation

## Goal

Support ordinary conversation without forcing task/workspace machinery into the UI.

## Main flow

```text
Home/Chat
  ↓
User sends conversational prompt
  ↓
Mukei responds naturally
  ↓
Response renders in document-like conversation layout
```

## Workspace rule

No workspace is created or shown unless the conversation produces/uses structured persistent work.

## Branch — inference unavailable

If no usable model/provider is configured:

- keep the chat shell usable;
- explain that a model/capability is required;
- provide a direct route to Models or relevant provider configuration;
- MUST NOT label the entire backend as unavailable if secure runtime is healthy.

---

# F04 — Send, stream, stop, retry

## Goal

Give the user predictable control over an active response/operation.

## Main flow

```text
Idle
  ↓ Send
Submitting
  ↓ Accepted
Running
  ↓ Hybrid response chunks/events
Completed
```

Response streaming SHOULD arrive in composed chunks:

- complete short paragraphs;
- complete headings;
- complete list items;
- stable code blocks where practical.

It SHOULD NOT visually simulate mechanical token-by-token typing when chunked output is available.

## Stop branch

```text
Running
  ↓ Stop
Cancelling
  ↓ backend acknowledgement/final state
Cancelled
```

The UI MUST prevent duplicate Stop actions while cancellation is pending.

## Failure branch

```text
Running → Failed
```

Show:

- concise failure explanation;
- what remains saved;
- Retry where safe;
- Details for diagnostics.

## Scroll behavior

- Do not yank scroll when the user is reading older content.
- Auto-follow only when the user is near the bottom.
- Show a “new response below” affordance when needed.

---

# F05 — Deep research / external information task

## Goal

Make research feel transparent without turning the conversation into logs.

## Preconditions

Required network/provider/tool capability is available or can be requested/configured.

## Main flow

```text
User asks research question
  ↓
Mukei states high-level plan
  ↓
ActivityCard appears
  ↓
Searching / Reading grouped progress
  ↓
User may expand details
  ↓
Structured answer appears
  ↓
Sources/output may be saved to Project/Storage where supported
```

## Activity collapsed example

```text
Working on it
Searching reliable sources…
```

## Activity expanded example

```text
Searching
✓ Source A
✓ Source B

Reading
✓ Document 1
• Document 2
```

## Trust requirements

- Provider/tool use must be disclosed at an appropriate level.
- Source transparency must be available.
- No fake progress percentages.

---

# F06 — Build/create structured files

## Goal

Turn a natural request into tangible workspace files and a usable output.

## Example

“Create a React dashboard.”

## Main flow

```text
User request
  ↓
Mukei confirms approach briefly
  ↓
Workspace becomes relevant
  ↓
WorkspaceCard appears
  ↓
Activity groups file creation/edit/build work
  ↓
User may open Workspace while work continues if safe
  ↓
Build/package completes
  ↓
ArtifactCard appears
```

## WorkspaceCard minimum content

- workspace/project title;
- meaningful file/change summary;
- View Workspace;
- export action when export is available.

## Completion

Example:

```text
Done. Your project is ready.

React Dashboard.zip
[Download] [Open workspace]
```

## Failure branch

If build/package fails after partial work:

- preserve successfully committed workspace files;
- show what is saved;
- offer Retry, Inspect, and Export Current where valid;
- MUST NOT imply rollback if partial files remain.

---

# F07 — Import/read multiple files

## Goal

Let the user bring files into Mukei, understand storage/provider implications, and ask questions over them.

## Main flow

```text
Composer
  ↓ Attach
System file picker
  ↓ Select file(s)
Import/staging validation
  ↓
Attachment chips/cards appear
  ↓
User submits request
  ↓
Activity groups file reads/ingestion
  ↓
Mukei responds with file-aware result
  ↓
Durable imported files are discoverable in Storage/workspace as defined by scope
```

## Trust requirements

The UI MUST make it possible to understand:

- whether a local copy is stored;
- what scope it belongs to;
- whether content must leave the device for a configured provider/tool;
- how to remove/revoke access where supported.

## Failure branches

- unsupported file;
- oversized file;
- permission revoked;
- import cancelled;
- file changed during import;
- ingestion/indexing failed after storage succeeded.

Storage success and indexing success MUST be represented as separate states when they differ.

---

# F08 — Open and inspect workspace

## Goal

Allow the user to understand tangible work without presenting a full IDE by default.

## Entry

- WorkspaceCard in conversation;
- Project context;
- Storage/project deep link where applicable.

## Main flow

```text
Workspace summary
  ↓ View workspace
Workspace screen
  ├─ Files
  ├─ Artifacts
  ├─ Linked chats (if supported)
  ├─ Activity history
  └─ Export actions
```

## File presentation

Default SHOULD be calm and structured, with folder grouping and meaningful state labels such as:

- Created
- Edited
- Imported
- Exported
- Needs review
- Local only

A dense IDE-style tree is not the default requirement.

---

# F09 — Export/download artifact

## Goal

The user clearly understands what was produced, where it remains stored, and what exporting does.

## Main flow

```text
Operation completes
  ↓
ArtifactCard appears
  ↓
User selects Download / Share / Save / Export
  ↓
Destination/action completes
  ↓
Confirmation shown
```

## Requirements

When known, show:

- output name;
- type;
- file count for bundles;
- size;
- storage state/destination.

Export MUST NOT silently remove the workspace copy.

The user should be able to retrieve the durable output later from the appropriate Storage/Workspace/Project surface.

---

# F10 — Continue an existing project

## Goal

Resume larger work with visible context.

## Main flow

```text
Drawer
  ↓ Projects
Project list
  ↓ Select project
Project screen
  ├─ Current/relevant workspace
  ├─ Files
  ├─ Chats
  └─ Artifacts
  ↓
User starts or resumes conversation
  ↓
Project context is visibly active
```

## Requirement

The user MUST be able to tell what context Mukei is using.

Project context MUST NOT be silently inferred in a way that risks modifying the wrong workspace/files.

---

# F11 — Install and activate local model

## Goal

Make technical model management understandable and trustworthy.

## Main flow

```text
Drawer
  ↓ Models
Installed + Available sections
  ↓ Select model
Model details
  ├─ size/storage cost
  ├─ compatibility
  ├─ context/configuration
  └─ local/remote status
  ↓ Install
Downloading
  ↓ Verify
Installed
  ↓ Activate
Active
```

## Requirements

- Download progress must be real, not fake.
- Cancel/pause/resume only appear when supported.
- Storage impact should be visible before download.
- Compatibility failures should be explained before or during installation.
- “Backend ready” and “model active” remain separate states.

---

# F12 — Error and recovery

## Goal

A failure should not destroy user confidence or obscure saved work.

## Generic structure

```text
Operation fails
  ↓
Explain failure in human language
  ↓
State what is preserved
  ↓
Offer valid recovery actions
  ↓
Details available separately
```

## Required questions answered

1. What failed?
2. What is still saved/safe?
3. What can I do next?
4. Is any manual cleanup required?

## Example

```text
I couldn’t finish the build.

The files created so far are still saved in your workspace.

[Retry] [Inspect] [Export current] [Details]
```

Actions MUST be shown only when semantically valid.

---

# F13 — Process death / app restart recovery

## Goal

Restore a truthful UI after Android process death, crash, or device restart.

## Main flow

```text
App/process restarts
  ↓
Secure runtime reopens
  ↓
Durable chat/workspace/storage state loads
  ↓
In-flight operation state is reconciled
  ↓
UI renders one of:
     completed
     failed
     cancelled
     interrupted/recoverable
```

## Requirements

- UI MUST NOT fabricate a still-running operation from stale Compose state.
- UI MUST NOT duplicate messages/events after replay/reconnection.
- Partial durable work must remain discoverable.
- Recoverable operations may offer Resume/Retry only if backend semantics support it.

This flow is a release-critical acceptance path.

---

# F14 — Chat management

## Entry

Chat options menu.

Expected actions:

- Pin
- Add to Project
- Find in chat
- Rename
- Export
- Delete

## Delete flow

```text
Delete chat?

This removes the conversation from Mukei.
Files saved in projects or storage are not deleted unless selected separately.

[Cancel] [Delete]
```

The exact copy MUST reflect final ownership semantics from the storage/project ADRs.

---

# F15 — Settings and personalization

## Goal

Change durable preferences without destabilizing the product’s visual behavior.

## Main flow

```text
Drawer
  ↓ Settings
Settings section
  ↓ Change preference
Validate/save
  ↓ Immediate or clearly-labelled deferred effect
```

Personality/tone changes MAY alter language behavior but MUST NOT make fundamental visual interaction patterns unpredictable.

Privacy/provider/storage changes MUST clearly describe consequences before destructive or access-expanding changes.

---

# Cross-flow invariants

All primary flows MUST satisfy these rules:

1. **Natural intent first** — mode selection is never mandatory before typing.
2. **Visible work** — meaningful long-running work has activity/progress representation.
3. **User control** — cancellable work exposes Stop/Cancel when appropriate.
4. **Truthful capability state** — backend, model, network, provider, storage, and artifact readiness are distinct.
5. **Durable outputs** — users can find created work after leaving the conversation.
6. **Progressive disclosure** — details exist without turning default UX into logs.
7. **Recovery** — failures explain preserved state and valid next actions.
8. **Privacy clarity** — local versus external processing is not misleading.
9. **No silent destructive coupling** — deleting chat/project/workspace/file must follow explicit ownership semantics.
10. **Real-device validation** — cold start, process restart, file import, native loading, signing/installability, and core flows require runtime acceptance evidence.

# Next derivations

The next documents should derive directly from these flows:

- `03_SCREEN_SPECIFICATIONS.md`: screen contracts and entry/exit points;
- `05_INTERACTION_STATE_MODEL.md`: F01/F04/F06/F07/F11/F13 state machines;
- `06_UI_BACKEND_CONTRACT.md`: command/event/snapshot mapping;
- `10_TEST_ACCEPTANCE_PLAN.md`: one acceptance scenario per critical flow and branch.
