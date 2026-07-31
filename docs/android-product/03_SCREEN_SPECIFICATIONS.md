# 03 — Screen Specifications

Status: **Draft v0.1**

This document defines the behavioral contract for the Android product surfaces described by the Mukei UI/UX Blueprint v0.1 and the flows in `02_UI_UX_FLOWS.md`.

It defines what each screen must communicate and do. It does **not** prescribe final pixel-perfect layouts or internal Kotlin class structure.

## Screen contract template

Each screen specification uses the following fields:

- **Purpose** — why the screen exists.
- **Entry points** — how users reach it.
- **Primary content** — the minimum visible hierarchy.
- **Primary actions** — actions that define the screen.
- **States** — empty/loading/ready/error/partial states the UI must represent.
- **Navigation/back** — deterministic navigation behavior.
- **Trust/accessibility** — privacy, local/remote, screen-reader, focus, and control requirements.
- **Data contract** — information the UI needs from repositories/backend; exact protocol mapping is deferred to `06_UI_BACKEND_CONTRACT.md`.
- **Acceptance signals** — observable criteria for product/QA review.

## Global shell rules

These rules apply to all primary destinations unless a screen explicitly overrides them.

1. Primary screens MUST expose a stable way to open the navigation drawer.
2. The shell MUST NOT use the app name as a large repeated title merely for branding.
3. New Chat SHOULD be globally reachable from the primary shell where context permits.
4. Context-specific actions belong in the options menu or inline controls, not in a permanent global toolbar.
5. Destructive actions MUST be separated from routine actions and require confirmation when data loss is possible.
6. A healthy secure backend with unavailable model artifacts MUST NOT be represented as a total app failure.
7. Long-running operations MUST expose user control when cancellation/pause/approval is supported.
8. The shell MUST remain navigable when inference is unavailable.
9. Touch targets MUST be at least 48×48dp even when visual icons are smaller.
10. Large text and screen-reader navigation MUST preserve all primary actions.

---

# S00 — Startup / Readiness / Recovery

Priority: **M1 foundation**  
Related flows: **F01, F12, F13**

## Purpose

Bridge process startup into the real product shell without making backend diagnostics the primary user experience.

## Entry points

- cold app launch;
- process recreation;
- explicit Retry after startup failure.

## Primary content

The default successful path SHOULD transition quickly into Home without a dedicated diagnostic screen.

If startup is not immediate, show a bounded warm startup state containing:

- Mukei mark or restrained identity cue;
- concise status such as `Opening your workspace…`;
- optional retry/recovery only when needed.

## Readiness dimensions

The UI MUST model these independently:

- app shell readiness;
- secure native runtime readiness;
- encrypted database/storage readiness;
- inference/model readiness;
- optional network/provider readiness.

Example valid state:

```text
App shell: ready
Secure runtime: ready
Encrypted storage: ready
Inference: model artifacts required
```

This state MUST allow navigation to Home, Storage, Models, and Settings where their dependencies are satisfied.

## Failure state

A startup failure surface MUST answer:

1. What could not start, in human language?
2. Is local data still safe?
3. Can the user retry safely?
4. Is there a stable diagnostic code available under Details?

Raw exception strings MUST NOT be the only user-facing explanation.

## Navigation/back

- During unrecoverable startup, Back exits according to normal Android behavior.
- After shell readiness, startup MUST leave navigation history in a normal Home state, not trap the user on a splash route.

## Trust/accessibility

- Do not continuously announce low-level boot steps to screen readers.
- Announce only meaningful transitions: ready, recovery required, retry result.
- Never imply that files are lost unless that is known.

## Data contract

Required projection:

```text
AppReadiness
- shellState
- backendState
- storageState
- inferenceState
- providerState
- recoverableFailure?
- diagnosticCode?
```

## Acceptance signals

- Healthy backend + missing model opens usable product shell.
- Failure details are expandable rather than dominant.
- Retry cannot create duplicate native runtimes.
- Process recreation converges on truthful readiness state.

---

# S01 — Home / Opening Screen

Priority: **M1**  
Related flows: **F02, F03, F04**

## Purpose

Provide a calm invitation to begin naturally. Home is not a dashboard and not a feature catalog.

## Entry points

- successful startup;
- tap `Mukei` in drawer;
- New Chat action;
- completion/exit from flows that intentionally return to a clean start.

## Primary content

### Top bar

Left:

- Menu icon.

Right:

- New Chat icon;
- Options icon when meaningful.

Home MUST NOT show a redundant large `Mukei` title.

### Greeting block

Examples:

- `Good evening.`
- `What’s on your mind?`
- `Ready when you are.`

Greeting copy SHOULD feel natural and MUST NOT use generic marketing language such as `Your AI assistant is ready`.

### Composer

The composer is the dominant interactive object.

It MUST support the product path for:

- text;
- attachments/files;
- send;
- multiline growth.

Voice/images/future context controls MAY be added when their capability is implemented.

### Capability chips

Examples:

- Deep Research;
- Build App;
- Read Files;
- Write;
- Code;
- Workspace.

Chips MUST remain optional affordances. The user MUST be able to type and send without selecting one.

## States

### Empty first launch

Show greeting + composer + lightweight capability chips.

Do NOT show:

- large feature grid;
- recent workspace dashboard;
- promotional hero;
- mandatory mode picker.

### Returning user

The opening state SHOULD remain clean. Recent work MAY be exposed via drawer, search, subtle Recent affordance, or Projects — not as the dominant Home content.

### Keyboard open

- composer remains stable and reachable;
- greeting MAY compact/fade upward;
- chips MAY compact;
- layout MUST avoid abrupt jumps.

### User typing

- send becomes primary;
- capability chips become visually quieter;
- attachment controls remain available.

### Inference unavailable

Composer may remain usable for drafting/attachments, but submission requiring inference MUST explain the missing capability and offer direct navigation to Models/provider setup.

## Primary actions

- open drawer;
- start new chat;
- type request;
- attach file;
- select/clear optional capability hint;
- send.

## Navigation/back

- Back with keyboard open dismisses keyboard first.
- Back with drawer open closes drawer first.
- Back from clean Home follows normal root activity behavior.

## Trust/accessibility

- Composer needs a stable accessible label independent of placeholder text.
- Capability chips must expose selected state semantically, not by color alone.
- If a capability would use a remote provider, disclosure belongs at execution/approval time, not as permanent warning noise on Home.

## Data contract

```text
HomeUiModel
- greeting
- draft
- attachments[]
- capabilityHints[]
- selectedHint?
- sendAvailability
- inferenceAvailability
- activeProjectContext?
```

## Acceptance signals

- Primary action is obvious within three seconds.
- User can start without selecting a mode.
- Screen remains usable at large font scale.
- New Chat produces a clean draft without silently deleting existing conversation history.

---

# S02 — Navigation Drawer / App Map

Priority: **M1**  
Related flows: **F02, F08, F10, F11**

## Purpose

Provide stable access to persistent user areas without turning navigation into a capability dashboard.

## Locked hierarchy

```text
Mukei

Storage
Projects
Models

Chats

Settings
```

## Phone behavior

- modal drawer;
- approximately 82–88% width as a visual target, subject to responsive/accessibility constraints;
- warm paper surface;
- Settings visually anchored toward bottom when space permits;
- drawer content MUST remain scrollable when font scale/content exceeds viewport.

## Drawer items

### Mukei

Returns to Home/opening state.

### Storage

User-owned/imported/generated durable files and exports.

### Projects

Long-running organized work and context.

### Models

Local/remote model capability management.

### Chats

Pinned/recent/archived/project-linked conversations. Recent chat list MUST NOT dominate the entire drawer by default.

### Settings

Personalization, Memory, Appearance, General, Privacy, Storage, Providers, Advanced, About.

## States

- closed;
- opening;
- open;
- selected destination;
- chat list empty;
- long chat list;
- offline.

## Interaction rules

- inactive icons use thin style;
- active destination uses fill icon + selected surface;
- color MUST NOT be the only active-state cue;
- drawer close/open MUST not bounce or overshoot.

## Navigation/back

- Back closes drawer before navigating away from destination.
- Selecting current destination closes drawer without duplicating navigation stack entries.
- Selecting another primary destination SHOULD behave as top-level navigation, preserving expected state according to navigation ADR.

## Trust/accessibility

- Focus enters drawer at first meaningful item when opened via accessibility navigation.
- Focus MUST not escape behind modal drawer.
- Closing drawer returns focus logically to the invoking control.

## Data contract

```text
DrawerUiModel
- currentDestination
- recentChats[]
- pinnedChats[]
- unread/recovery indicators where valid
```

## Acceptance signals

- All persistent areas remain reachable with inference offline.
- Long chat lists do not displace Settings permanently.
- Active destination is perceivable without color.

---

# S03 — Conversation

Priority: **M2**  
Related flows: **F03, F04, F05, F06, F07, F12, F13**

## Purpose

Serve as the primary thinking, clarification, decision, progress, and result surface.

Conversation is document-like, not primarily a stack of tiny alternating chat bubbles.

## Entry points

- submit from Home;
- open existing chat;
- continue chat from Project;
- deep link from recovery/recent state.

## Primary content

### Top bar

- Menu;
- New Chat;
- Options.

Conversation options MAY include:

- Pin;
- Add to Project;
- Find in chat;
- Rename;
- Export;
- Delete.

### User prompt block

Clearly identifies user intent without excessive bubble styling.

### Mukei response block

Supports document-like rendering:

- paragraphs;
- headings;
- lists;
- code blocks;
- references/source affordances;
- inline ActivityCard;
- inline WorkspaceCard;
- inline ArtifactCard;
- ErrorRecoveryCard.

### Composer

Anchored/reliably reachable at bottom of interaction surface.

## Streaming behavior

Use hybrid composed chunks where possible:

- complete short paragraphs;
- complete headings;
- complete list items;
- stable code blocks.

The UI SHOULD NOT simulate mechanical token-by-token typing when the backend supplies meaningful chunks/events.

## Operation states

Conversation MUST visibly distinguish:

- idle;
- submitting;
- running;
- cancelling;
- completed;
- cancelled;
- failed;
- recovery/reconnecting where applicable.

Stop MUST NOT be tappable repeatedly while cancellation is pending.

## Scroll behavior

- auto-follow only if user is near the bottom;
- never yank the user away from older content they are reading;
- show `new response below` affordance when streaming continues offscreen;
- restoring process/screen state SHOULD restore a sensible reading position without pretending transient streaming position is durable.

## Activity integration

Default inline activity is concise:

```text
Working on it
Searching reliable sources…
[Details]
```

Low-level tool operations MUST NOT flood the main conversation by default.

## Workspace integration

When structured persistent work becomes relevant, show a WorkspaceCard with:

- title;
- file/change summary;
- View Workspace;
- Export action when available.

Workspace SHOULD NOT appear for ordinary casual conversation.

## Completion integration

Clear and calm:

```text
Done. Your project is ready.
```

Avoid celebratory noise, generic `Task completed successfully`, or excessive animation.

## Error/recovery

Failure presentation MUST state:

- what failed;
- what remains saved;
- valid next actions;
- optional Details.

Recovery actions MUST be semantically valid. Do not show `Retry` when replay is unsafe or could duplicate destructive work.

## Find in chat

When implemented:

- search field;
- highlighted matches;
- result count;
- next/previous navigation;
- reading position preservation.

## Delete confirmation

Must clarify that deleting a chat does not automatically delete files already stored in Project/Storage unless the data model explicitly couples them.

## Navigation/back

- Back closes transient sheets/search before leaving chat.
- Leaving an actively running operation MUST NOT silently cancel it unless product policy explicitly says so.
- If work continues in background/process scope, show truthful status when user returns.

## Trust/accessibility

Screen reader announcements SHOULD summarize major task phases, not every token or low-level operation.

Examples:

- `Searching reliable sources.`
- `Project files created.`
- `Build failed. Files are saved.`

## Data contract

```text
ConversationUiModel
- chatId
- title
- projectContext?
- items[]
- composer
- activeOperation?
- scrollContinuationState
- capabilities

ConversationItem
- UserPrompt
- MukeiResponse
- Activity
- WorkspaceSummary
- Artifact
- ErrorRecovery
- SystemNotice
```

## Acceptance signals

At any time, the user can answer:

1. What did I ask?
2. What is Mukei doing?
3. What changed?
4. What can I do next?

---

# S04 — Activity Details

Priority: **M2/M4**  
Related flows: **F05, F06, F07, F12**

## Purpose

Expose real work through progressive disclosure and reduce black-box anxiety without becoming a developer console.

## Entry points

- tap Details on ActivityCard;
- open activity history from Workspace/Project where supported.

## Primary content

Grouped human-readable categories:

- Searching;
- Reading;
- Writing;
- Editing;
- Building;
- Testing;
- Packaging;
- Waiting;
- Needs approval;
- Done;
- Couldn’t finish.

Parallel operations SHOULD be grouped.

Example:

```text
Reading 6 files…
```

Expanded:

```text
Reading
✓ package.json
✓ README.md
• src/App.tsx
```

## Progress rules

- Never invent percentages.
- Use counts/phase/current meaningful operation when available.
- Provider/tool identity MAY be shown in expanded details where useful for trust.

## Controls

Contextual only:

- Stop;
- Pause/Continue;
- Approve/Reject;
- Retry failed step;
- Hide details.

## Accessibility

Do not create continuous live-region chatter. Announce only meaningful phase transitions and approval requirements.

## Acceptance signals

- Default conversation remains calm.
- Expanded details make external/provider/file activity inspectable.
- Failure identifies failed phase without exposing secrets/raw credentials.

---

# S05 — Workspace

Priority: **M4**  
Related flows: **F06, F08, F09, F10, F12**

## Purpose

Make persistent structured work tangible: files, changes, outputs, linked context, and exports.

Workspace is a calm project table, not a full IDE by default.

## Entry points

- WorkspaceCard from conversation;
- Project screen;
- relevant Storage/deep link;
- recovery path after partial work.

## Primary content

1. Workspace/project title.
2. Local/trust status where relevant.
3. Files.
4. Artifacts.
5. Linked chats when supported.
6. Activity history when supported.
7. Export actions.

## File presentation

Supports:

- folder grouping;
- file type icon;
- generated vs imported/user-provided distinction;
- state labels;
- search/filter;
- preview;
- export selection where supported.

Recommended human labels:

- Created;
- Edited;
- Read;
- Imported;
- Exported;
- Needs review;
- Failed;
- Locked;
- Local only.

A dense IDE tree MUST NOT be the default on phone.

## Workspace states

- loading projection;
- ready/idle;
- active mutation;
- partial result;
- export in progress;
- recoverable failure;
- unavailable/deleted.

Files already durably committed MUST remain visible if a later build/package step fails.

## Primary actions

- open/preview file;
- inspect changes where supported;
- export ZIP/artifact;
- search/filter;
- navigate to linked chat/project context.

## Destructive actions

Deletion confirmation MUST explain scope:

```text
Delete this project?
This removes the project files from Mukei’s workspace. Exported copies outside Mukei will not be touched.
```

Exact wording depends on ownership model resolved in `07_STORAGE_WORKSPACE_MODEL.md`/ADR.

## Trust/accessibility

Use contextual labels such as:

- `Local workspace`;
- `Stored on this device`;
- `Provider access required for this step`.

Do not repeat privacy banners on every row.

## Data contract

```text
WorkspaceUiModel
- workspaceId
- title
- scope/owner
- state
- trustLabel?
- files[]
- artifacts[]
- linkedChats[]
- activitySummary
- exportCapabilities
```

## Acceptance signals

- User can identify changed/created files.
- User can retrieve generated outputs after conversation ends.
- Partial work remains truthful after failure/restart.

---

# S06 — Storage

Priority: **M3**  
Related flows: **F07, F09**

## Purpose

Expose durable local files Mukei can access or has produced without resembling a raw Android filesystem browser.

## Entry points

- drawer → Storage;
- saved/export confirmation;
- artifact/workspace deep link.

## Primary content

- title `Storage`;
- contextual trust label such as `Stored on this device`;
- recent items;
- category/filter affordances;
- import action;
- list/grid of durable items.

Candidate categories:

- Files;
- Images;
- Docs;
- Exports.

Categories MUST be derived from real metadata, not duplicate ownership concepts.

## States

- empty;
- loading;
- ready;
- filtering/searching;
- importing;
- import partially succeeded but indexing failed;
- storage error;
- permission/revocation issue.

Storage success and indexing/readiness MUST be separate when they differ.

## Primary actions

- open item;
- import;
- sort/filter/search;
- share/export where valid;
- delete/remove according to ownership scope.

## Trust requirements

For imported files, user must be able to understand:

- whether Mukei stores a copy;
- owning scope;
- original/source relationship;
- whether provider access may send content off-device;
- how to delete/revoke where supported.

## Data contract

Deferred in detail to `07_STORAGE_WORKSPACE_MODEL.md`, but UI requires durable item identity, type, ownership/scope, size, timestamps, state, source, and available actions.

## Acceptance signals

- Generated exports remain discoverable.
- Import errors do not create ghost UI entries.
- User can distinguish local storage from external/share destination.

---

# S07 — Projects

Priority: **M7**  
Related flows: **F10**

## Purpose

Provide an aggregation/context layer for longer-running work.

The exact ownership model is pending ADR and `07_STORAGE_WORKSPACE_MODEL.md`.

## Project list

Show concise project rows/cards with meaningful signals:

- title;
- recent activity/update;
- workspace/file/artifact summary where useful;
- optional pinned state.

Do not turn Projects into a generic dashboard of analytics.

## Project detail

Expected sections:

- current/relevant workspace(s);
- files/artifacts according to ownership model;
- linked chats;
- activity history;
- project context controls.

## Context safety

The user MUST be able to tell when a Project context is active.

Mukei MUST NOT silently mutate the wrong workspace because of hidden project inference.

## Primary actions

- create project;
- open/resume;
- start/attach chat;
- add files;
- export;
- rename/settings;
- delete with scope clarification.

## Acceptance signals

- Resuming project clearly restores context.
- Project deletion semantics are explicit.
- Chats/workspaces/files are not duplicated merely to appear in a Project UI.

---

# S08 — Models

Priority: **M6**  
Related flows: **F03, F11**

## Purpose

Make model capability, installation, activation, compatibility, and local/remote behavior understandable.

## Primary content

- title `Models`;
- clear local/privacy context;
- Installed section;
- Available section;
- active model indication;
- model details/configuration.

## Model row/card minimum

- model name;
- size;
- install/availability state;
- local/remote label;
- compatibility;
- context/config summary where relevant.

## Lifecycle states

- unavailable/catalog only;
- available;
- queued/downloading;
- paused;
- verifying;
- installed;
- activating;
- active;
- incompatible;
- failed;
- deleting.

Only show Pause/Resume if actually supported.

## Primary actions

- install;
- cancel/pause/resume where supported;
- activate/select;
- configure;
- delete;
- import model where supported.

## Trust/accessibility

Before installation, show storage impact and local/remote behavior.

`Backend ready` and `Model active` MUST remain separate concepts.

## Acceptance signals

- Missing model routes user here without implying backend failure.
- Real download progress only.
- Active state uses label/icon/surface, not color alone.

---

# S09 — Settings

Priority: **M8**  
Related flows: cross-cutting

## Purpose

Expose durable controls without contaminating primary conversation surfaces with configuration complexity.

## Sections

- Personalization;
- Memory;
- Appearance;
- General;
- Privacy;
- Storage;
- Providers / API Keys;
- Advanced;
- About / open-source notices.

No profile/auth card is required while the product has no account-centric identity model.

## Rules

- Security/privacy settings MUST explain consequences before destructive actions.
- Secrets/API keys MUST never be rendered in full after storage unless explicit secure reveal policy allows it.
- Appearance includes theme/accent/text/reduced-motion controls when implemented.
- Memory settings MUST explain scope, storage, deletion, and effect on responses.

## Navigation/back

Settings is a top-level drawer destination; subsections may use nested navigation with deterministic Up behavior.

## Acceptance signals

- Privacy/storage/provider concepts are understandable without technical logs.
- Destructive reset/delete actions require confirmation and scope explanation.

---

# S10 — Artifact / Export Interaction

Priority: **M5**  
Related flows: **F06, F09, F12**

## Purpose

Present usable outputs as durable objects with clear retrieval/export semantics.

## ArtifactCard minimum

- human result label (`Your files are ready`);
- output name;
- type;
- size/file count when known;
- storage state where useful;
- 1–2 primary actions.

Example:

```text
Your files are ready
React Dashboard.zip
34 files · 1.8 MB
[Download] [Open workspace]
```

## Primary actions

Depending on type/capability:

- Download/Save;
- Open;
- Share;
- Save to Project;
- Open Workspace.

## Export rules

- Export MUST NOT silently remove the durable workspace/storage copy.
- Destination/action success must be confirmed truthfully.
- Failed external export MUST NOT mark the internal artifact as lost.

## Completion tone

Satisfying but quiet:

- optional restrained success icon;
- subtle haptic;
- brief fade/translate;
- no confetti/bounce.

## Acceptance signals

- User knows what was produced and where it remains.
- Re-export is possible when the durable artifact still exists.

---

# S11 — Modal sheets, dialogs, pickers, and menus

Priority: cross-cutting

## Rules

Transient surfaces MUST have one clear purpose.

Examples:

- options menu;
- project picker;
- delete confirmation;
- import/file picker handoff;
- activity details sheet;
- export destination/share sheet.

### Dialog requirements

Destructive confirmations state:

- object being affected;
- scope of deletion/change;
- what will remain untouched;
- clear Cancel and destructive action labels.

Avoid generic `Are you sure?` without scope.

### Bottom sheet requirements

- support large text without clipping;
- scroll when required;
- preserve system back behavior;
- avoid nested sheets where a dedicated screen is clearer.

---

# Screen traceability matrix

| Screen | Primary flows | First milestone |
|---|---|---|
| S00 Startup/Readiness | F01, F12, F13 | M1 |
| S01 Home | F02–F04 | M1 |
| S02 Drawer | F02, F08, F10, F11 | M1 |
| S03 Conversation | F03–F07, F12, F13 | M2 |
| S04 Activity Details | F05–F07, F12 | M2/M4 |
| S05 Workspace | F06, F08–F10, F12 | M4 |
| S06 Storage | F07, F09 | M3 |
| S07 Projects | F10 | M7 |
| S08 Models | F03, F11 | M6 |
| S09 Settings | Cross-cutting | M8 |
| S10 Artifact/Export | F06, F09, F12 | M5 |

## Implementation rule

No screen should be implemented as an isolated mock disconnected from flow and state semantics.

For every screen shipped, implementation must be traceable through:

```text
Screen spec
  → interaction state model
  → UI/backend contract
  → repository/use-case implementation
  → real-device acceptance test
```
