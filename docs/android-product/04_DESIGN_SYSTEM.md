# 04 — Design System

Status: **Draft v0.1**

This document turns the UI/UX Blueprint visual direction into a Compose-oriented design contract.

The source blueprint defines a warm, soft-paper, local-first workspace aesthetic. This specification preserves that intent while separating **semantic roles** from raw color values so the implementation can support accessibility, dark theme, dynamic type, and future brand refinement.

## Design principles

1. **Warm, not decorative.** Warmth comes from surfaces, spacing, typography rhythm, and humane copy — not gradients or visual gimmicks.
2. **Quiet by default.** Conversation and reading surfaces should remain low-chroma and low-noise.
3. **Tangible work.** Workspace, activity, and artifact components may increase information density when work becomes structured.
4. **State over decoration.** Fill, icon style, surface emphasis, and motion should communicate real state.
5. **Accessible first.** Contrast, dynamic type, touch targets, reduced motion, and non-color status cues are part of the design system.
6. **Semantic tokens over hard-coded values.** Feature code MUST consume semantic tokens/components rather than raw hex values, arbitrary dp, or one-off radii.

---

# 1. Surface model

The visual hierarchy is:

```text
App background
  ↓
Primary paper surface
  ↓
Muted/secondary surface
  ↓
Elevated/transient surface
  ↓
Interactive/selected surface
```

Hierarchy SHOULD be established mostly through subtle tonal differences and spacing, not heavy shadows.

## Semantic surface roles

- `background` — root app canvas.
- `surface` — main paper-like content/composer/card base.
- `surfaceMuted` — secondary controls/chips/group backgrounds.
- `surfaceElevated` — menus, sheets, transient elevated content.
- `surfaceSelected` — active navigation/chip/selection state.

The implementation MAY derive `surfaceSelected` from accent-muted roles rather than add a separate fixed color.

---

# 2. Draft color tokens

The following values are imported from Blueprint v0.1 as **draft light-theme seeds**, not immutable brand constants.

| Role | Seed |
|---|---|
| Background | `#F8F2EA` |
| Surface | `#FFF9F1` |
| Surface muted | `#F2E8DC` |
| Surface elevated | `#FFFDF8` |
| Text primary | `#2B211A` |
| Text secondary | `#6B5A4A` |
| Text tertiary | `#9A8D80` |
| Divider | `#E7DCCF` |
| Accent | `#8A6A4F` |
| Accent muted | `#E8D7C6` |
| Success | `#687C5A` |
| Success muted | `#E4EBDD` |
| Warning | `#A7793F` |
| Warning muted | `#F0E2C8` |
| Error | `#9B5E55` |
| Error muted | `#F0DCD8` |

## Color rules

- Feature code MUST NOT reference these hex values directly.
- Semantic status MUST NOT rely on color alone.
- Text/background pairs MUST be contrast-tested before release.
- `textTertiary` MUST NOT be used for essential information if contrast becomes insufficient at small sizes.
- Error does not automatically mean bright red; severity is communicated through copy, icon/state, and surface treatment.
- Accent guides attention; it is not a branding flood-fill.

## Dark theme

Dark theme values are intentionally **not locked** in v0.1.

Dark theme MUST preserve:

- warm rather than blue-gray neutral bias;
- clear surface hierarchy;
- readable long-form text;
- muted accent behavior;
- semantic success/warning/error distinction;
- equivalent accessible contrast.

An ADR or token revision should lock the dark palette after visual/device testing.

---

# 3. Typography

Typography is primarily a readability system.

## Requirements

- dynamic type/font scaling MUST be supported;
- long-form text must remain comfortable at large scales;
- weights and spacing should carry hierarchy before extra color;
- body copy should not become an edge-to-edge wall;
- code uses a legible monospaced face/container.

## Semantic type roles

Exact font family remains implementation/design-review dependent.

Suggested Compose roles:

- `displaySmall` — rare onboarding/empty-state emphasis only.
- `headlineLarge` — major screen concept when needed.
- `headlineMedium` — section/screen title.
- `titleLarge` — workspace/project/artifact title.
- `titleMedium` — card heading.
- `bodyLarge` — primary Mukei long-form response.
- `bodyMedium` — standard UI/body content.
- `labelLarge` — primary controls/buttons.
- `labelMedium` — metadata/status/chips, subject to accessibility.
- `codeBody` — monospaced semantic role outside Material defaults if required.

## Conversation reading rhythm

- Target long-form line length: approximately **60–75 characters** when layout permits.
- Phone horizontal reading padding: minimum **16dp**, preferred **20dp**, up to **24dp** on larger phones.
- Tablet/large screens MUST constrain reading width rather than stretch paragraphs indefinitely.

## Copy casing

- Use sentence case for labels and actions.
- Avoid all-caps status language.
- Human-readable activity words are preferred over internal enum/code strings.

---

# 4. Spacing scale

Feature code SHOULD use a stable semantic spacing scale.

| Token | dp | Intended use |
|---|---:|---|
| `micro` | 4 | icon/text micro-gap, tiny separators |
| `xs` | 8 | compact internal gap |
| `sm` | 12 | compact control/card spacing |
| `md` | 16 | base screen/component spacing |
| `comfortable` | 20 | reading/composer comfort |
| `lg` | 24 | spacious card/screen separation |
| `section` | 32 | section separation |
| `largeSection` | 40 | large visual break |
| `major` | 56 | major empty-state separation |
| `openingBreath` | 72 | opening-screen breathing area where viewport permits |

## Rules

- Do not introduce arbitrary spacing values for routine layout when an existing token is adequate.
- Optical adjustments MAY use intermediate values inside reusable components, but should not become feature-local constants.
- Large text layouts MAY need increased vertical spacing.

---

# 5. Shape scale

| Token | dp | Intended use |
|---|---:|---|
| `small` | 8 | small controls |
| `chip` | 12 | chips / compact elements |
| `card` | 16 | standard cards |
| `largeCard` | 20 | prominent cards/composer internals |
| `sheet` | 24 | sheets/drawer corners |
| `composer` | 28 | large composer |

## Rules

- Avoid random per-screen radii.
- Rounded does not mean bubbly; avoid excessive capsule shapes for every container.
- Chip/button geometry must preserve accessible hit targets.

---

# 6. Elevation and shadows

Prefer tonal elevation.

## Allowed

- subtle shadow for modal drawer/sheets where needed for separation;
- minimal card shadow only if tonal separation is insufficient;
- gentle elevation interpolation during interaction.

## Avoid

- dark drop shadows;
- glossy floating cards;
- glassmorphism as a primary visual language;
- glowing borders;
- stacked/nested shadows.

---

# 7. Density contexts

Mukei uses three product-density modes.

## Quiet

Used for:

- Home;
- Conversation;
- reading/long-form result surfaces.

Characteristics:

- generous whitespace;
- low control density;
- restrained chroma;
- document rhythm.

## Active

Used for:

- Workspace;
- Activity;
- Storage/search;
- import/export.

Characteristics:

- more visible state/progress;
- more metadata;
- still warm and structured.

## Technical

Used for:

- Models;
- diagnostics;
- advanced settings.

Characteristics:

- denser information;
- explicit numbers/compatibility/config;
- no developer-console aesthetic unless user intentionally opens diagnostics.

Feature screens SHOULD declare which density context they are designed for.

---

# 8. Iconography

Primary icon library: **Phosphor Icons**, subject to Android licensing/package feasibility review.

Blueprint target distribution:

- ~87% Thin — default/action/navigation;
- ~8% Fill — active/selected/current state;
- ~5% Duotone — rare emotional/onboarding/milestone emphasis.

The percentages are directional, not telemetry requirements.

## Locked semantic rule

```text
Default = thin
Active/current = fill
Celebratory/expressive = duotone (rare)
```

## Sizes

Suggested visual sizes:

- toolbar: 22–24dp;
- chips: 18–20dp;
- cards: 20–24dp;
- meaningful empty-state icon: 40–56dp;
- hit target: minimum 48×48dp.

## State rules

Fill MUST communicate state, e.g.:

- selected drawer item;
- active capability chip;
- pinned chat;
- active model;
- selected toggle-like state.

Duotone MUST NOT become routine navigation styling.

## Candidate mappings

- Menu → `List`
- New chat → `ChatCircleText` / `NotePencil`
- Options → `DotsThreeVertical`
- Storage → `Archive` / `Files`
- Projects → `Folders`
- Models → `Cpu` / `Cube`
- Chats → `ChatsCircle`
- Settings → `GearSix`
- Deep Research → `Microscope` / `MagnifyingGlass`
- Build App → `Code` / `BracketsCurly`
- Read Files → `FileText` / `FolderOpen`
- Write → `PencilSimple`
- Workspace → `Desk` / `Folder`

Final mapping should be centralized; feature code must not independently choose inconsistent icon families.

---

# 9. Motion

Motion communicates state; it does not decorate idle screens.

## Timing tokens

- fast: **150ms**;
- standard: **180–220ms**;
- complex transition maximum: **250ms** in normal product UI.

Preferred motion curve target from blueprint:

```text
cubic-bezier(0.25, 0.1, 0.25, 1)
```

Compose implementation may approximate this with a reusable easing token.

## Allowed vocabulary

- fade;
- slight translate;
- opacity shift;
- gentle elevation change;
- small reveal;
- expansion/collapse.

Translate range target: **8–12dp**.

## Forbidden default vocabulary

- bounce;
- overshoot;
- spring recoil;
- elastic effects;
- large zoom;
- dramatic rotation;
- persistent pulsing backgrounds.

Spring APIs MAY only be used when configured to avoid visible bounce/overshoot and approved by design review.

## Motion levels

### Level 1 — Invisible

- screen transition;
- drawer;
- menu;
- routine state changes.

### Level 2 — Context

- activity expansion;
- workspace card reveal;
- contextual controls;
- meaningful loading transitions.

### Level 3 — Emotion

Rare:

- first setup;
- meaningful export completion;
- milestone/empty-state warmth.

## Reduced motion

When reduced motion is enabled:

- remove nonessential translation;
- prefer opacity/state replacement;
- shorten durations;
- disable expressive motion.

---

# 10. Loading/progress language

Preferred hierarchy:

1. progressive real content;
2. skeleton where it meaningfully represents stable layout;
3. human activity line;
4. spinner;
5. blocking dialog only as last resort.

## Progress rules

- Never display fake percentages.
- Prefer real byte/count progress for downloads/imports.
- For indeterminate work, show phase/current operation.

Examples:

- `Searching 4 sources…`
- `Reading 8 files…`
- `Writing project files…`
- `Packaging ZIP…`

---

# 11. Core component contracts

Components define visible semantics; their internal architecture belongs to Android implementation docs.

## TopBar

Contains contextually:

- MenuIconButton;
- NewChatIconButton;
- OptionsIconButton.

Rules:

- quiet;
- consistent icon positions;
- no Home title duplication;
- options are context-aware.

## NavigationDrawer

Sections:

- Mukei;
- Storage;
- Projects;
- Models;
- Chats;
- Settings.

States include closed/open/selected/long-chat-list/empty chats.

## GreetingBlock

- time-aware/neutral greeting;
- optional locally-known name;
- warm prompt;
- must remain secondary to composer interaction.

## Composer

Supports at minimum:

- text;
- attachments;
- send;
- multiline expansion;
- contextual placeholder.

Design:

- large rounded paper-like container;
- generous internal padding;
- secondary attachment controls;
- send becomes primary when sendable.

The composer MUST NOT encode capability selection as a required mode switch.

## CapabilityChipRow

- horizontal scroll on phone;
- short label + thin icon;
- selected uses fill + stronger selected surface;
- disabled/loading states are semantic.

Long-press explanation is a blueprint affordance but should only be implemented if discoverable/accessibility behavior is satisfactory.

## UserPromptBlock

- clearly distinguishes user input;
- not excessively bubbly;
- supports long text and attachments.

## MukeiResponseBlock

Document-like rich body supporting:

- headings;
- paragraphs;
- lists;
- code;
- references;
- inline status/work cards.

## ActivityCard

Collapsed:

- high-level status;
- current meaningful phase;
- Details.

Expanded:

- grouped operations;
- per-step state;
- provider/file/tool details where appropriate;
- contextual control.

## WorkspaceCard

Minimum:

- label `Workspace`;
- title;
- file/change summary;
- View Workspace;
- Export when valid.

## ArtifactCard

Minimum:

- result state;
- filename/title;
- size/type/count when known;
- 1–2 primary actions.

## ErrorRecoveryCard

Must answer:

- what failed;
- what is preserved;
- what can be done next;
- Details.

## FileRow

- name;
- type icon;
- human state label where meaningful;
- selected/accessibility state;
- context action without hidden destructive shortcut.

## ModelCard

- model name;
- size;
- status;
- local/remote;
- compatibility;
- active state;
- install/activate action.

---

# 12. Buttons and actions

## Hierarchy

Use semantic action hierarchy rather than color proliferation:

- Primary — one dominant next action per local context.
- Secondary — common alternative.
- Tertiary/text — lower-priority inspection/navigation.
- Destructive — explicit destructive action with contextual confirmation.

Cards SHOULD normally expose no more than 1–2 primary visible actions; overflow can hold secondary actions.

## Labels

Use verbs and concrete objects:

- `View workspace`
- `Export ZIP`
- `Retry build`
- `Install model`

Avoid vague:

- `Proceed`
- `OK` for meaningful actions;
- internal terms like `Execute`.

---

# 13. Dividers and grouping

Prefer:

1. whitespace;
2. headings/labels;
3. subtle surface grouping;
4. divider only when structural clarity still requires it.

Do not place separators between every list row merely by default.

---

# 14. Accessibility tokens and invariants

## Touch

- minimum hit target: **48×48dp**.
- visual icon can remain 20–24dp.

## Dynamic type

- layouts must wrap/reflow;
- fixed-height text containers are prohibited for essential content;
- drawer/sheets must scroll under large text.

## Color

State uses combinations of:

- icon/style;
- label;
- surface/shape;
- color.

Color alone is insufficient.

## Screen readers

Reusable components MUST expose semantic labels/state.

Activity components should summarize progress rather than announce every low-level operation.

## Focus

Modal drawers/dialogs/sheets must manage focus predictably and restore focus on close.

---

# 15. Privacy/trust presentation

Trust is embedded contextually rather than shown as permanent warning banners.

Approved contextual phrases include:

- `Local workspace`
- `Stored on this device`
- `No account required`
- `Uses your configured provider`
- `This search sends your query to your selected provider`

Use these at trust-sensitive moments:

- first setup;
- Storage;
- provider setup;
- file import;
- web search;
- model install;
- export/share.

Do not repeat the same privacy label on every row/card.

---

# 16. Compose implementation contract

The design system SHOULD live in `:core:designsystem` and expose semantic primitives rather than feature-local styling.

Candidate package concepts:

```text
MukeiTheme
MukeiColors / semantic ColorScheme adapter
MukeiSpacing
MukeiShapes
MukeiMotion
MukeiIcons
MukeiTypography
```

Reusable visible components MAY be split between core design system and feature-owned components depending on domain coupling.

### Core design-system candidates

- icon button wrappers;
- button styles;
- chip styles;
- paper/card surfaces;
- typography/theme;
- dialog/sheet primitives;
- loading/progress primitives;
- status labels.

### Feature-owned candidates

- ActivityCard;
- WorkspaceCard;
- ArtifactCard;
- ModelCard;
- Conversation response renderer.

Feature-owned components MUST still consume core tokens.

## Prohibited implementation pattern

```kotlin
Color(0xFFF8F2EA)
RoundedCornerShape(17.dp)
padding(19.dp)
```

inside random feature composables.

Equivalent values must come from the shared system unless a documented exceptional layout requirement exists.

---

# 17. Design review checklist

A screen passes visual/system review when:

- primary action is understandable quickly;
- warmth exists without decorative clutter;
- long-form text is comfortable;
- surfaces use semantic hierarchy;
- controls use shared shapes/spacing;
- state is not conveyed by color alone;
- motion is restrained;
- reduced-motion behavior exists;
- touch targets meet minimum size;
- large text does not clip primary actions;
- local/remote/privacy cues appear where context requires them;
- no feature has invented its own icon/style language.

## Token governance

Token changes that affect multiple screens should be made centrally and reviewed as design-system changes, not silently adjusted feature-by-feature.

Major irreversible visual-system changes MAY require an ADR when they alter implementation architecture, dependency choice, or accessibility behavior.
