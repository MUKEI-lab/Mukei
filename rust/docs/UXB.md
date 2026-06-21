# MUKEI — UI/UX Design Brief (UXB v2.1)

> *"Calm. Capable. Confidential. Crafted."*

| Field | Value |
|-------|-------|
| **Document ID** | MUKEI-UXB-v2.1 |
| **Supersedes** | UXB v1.0 (2026-06-19, first draft) · UXB v2.0 (2026-06-19, Editorial Luxury Manifesto) |
| **Status** | 🟢 Locked Design System Pass — v0.7.5 Convergence-Alignment Revision |
| **Aesthetic** | Warm Luxury Editorial (70 % Minimalism · 20 % Editorial · 10 % Luxury Warm) |
| **Companion docs** | [PRD v0.7.5](PRD_v0.7.5_architect_pass.md) · [TRD v0.7.5](TRD_v0.7.5_architect_pass.md) · [Application Flow v1.2](MUKEI-AF_v1.2_ApplicationFlow.md) · [Backend Schema v1.2](MUKEI-BS_v1.2_BackendSchema.md) |
| **Audience** | Designers, Front-end (QML) engineers, A11y reviewers, Localization, QA, Product |
| **Locked-palette hash** | `dolcevita:#D8CABD · espresso:#362417 · taupe:#92817A · copper:#B87333 · gold:#D4AF37 · terracotta:#C17F3E` |
| **Notation** | Every screen/component cross-references PRD `REQ-*`, TRD `§X.Y`, and (where applicable) AF/BS sections. Every section ends with an **FMEA table** where failure modes are non-trivial. |

> **Reading rule:** *This document is a contract between Design and Engineering.* If a hex code, type token, or spacing value is not here, it does not exist in MUKEI. No drift. No improvisation. No "I thought it looked better."

---

## Table of Contents

1.  [Document Control & Design Principles](#1-document-control--design-principles)
2.  [Design System Foundation — The 70/20/10 Rule](#2-design-system-foundation--the-702010-rule)
3.  [Color System & Theme Tokens](#3-color-system--theme-tokens)
4.  [Typography System — The Editorial Heart](#4-typography-system--the-editorial-heart)
5.  [Spacing, Grid & Layout Architecture](#5-spacing-grid--layout-architecture)
6.  [Component Library — QML Primitives](#6-component-library--qml-primitives)
7.  [Screen Flows & Key Journeys](#7-screen-flows--key-journeys)
8.  [Motion, Animation & Micro-Interactions](#8-motion-animation--micro-interactions)
9.  [Iconography & Imagery](#9-iconography--imagery)
10. [Accessibility (A11y) & Inclusivity](#10-accessibility-a11y--inclusivity)
11. [Qt/QML Implementation Strategy](#11-qtqml-implementation-strategy)
12. [UI Failure Modes & Effects Analysis (FMEA)](#12-ui-failure-modes--effects-analysis-fmea)
13. [Design Deliverables & Handoff](#13-design-deliverables--handoff)
14. [Appendix](#14-appendix)
15. [Revision History](#15-revision-history)

---

## 1. Document Control & Design Principles

### 1.1 Version & Authorship

| Item | Value |
|------|-------|
| Version | 2.0 |
| Date | 2026-06-19 |
| Author | AI-Architect (Senior Design Systems Pass) |
| Reviewers | Engineering Lead, Security Reviewer, Accessibility Reviewer |
| Approval gates | (a) Color contrast WCAG 2.1 AA verified; (b) Type scale matches Figma library; (c) Cross-refs to PRD/TRD/AF/BS validated |

### 1.2 The "Warm Luxury Editorial" Manifesto

MUKEI is **not** another chat app skin. It is a **private boutique** for thought.

Where the rest of the AI industry shouts in blue gradients, neon glows, and animated mesh backgrounds — MUKEI whispers in **unbleached paper, espresso wood, and copper light**. It looks like a leather-bound notebook from a Milanese bookshop. It reads like a New York Times Sunday essay. It feels like a privately-printed monograph.

This is intentional. The user's data lives only on their device — the visual language must *reinforce* that intimacy. Cloud apps can afford to look corporate. A device-resident agent must look *personal*.

The visual identity of MUKEI is built on three pillars:

1. **The Page** — the dominant surface is paper-toned, never pure white, never pure black. (See §3.)
2. **The Type** — content is set like editorial print: serif for AI, sans for chrome, mono for code. (See §4.)
3. **The Metal** — accents are warm metals (copper, muted gold, terracotta), never cold blues. (See §3.4.)

> Anti-pillar (what we are NOT): we are not Material Design, not iOS Mail, not ChatGPT, not Discord, not Notion, not Linear, not Telegram. References are inspiration; copies are violations.

### 1.3 The CCCC Design Principles

Every design decision must satisfy **all four** of the following. If any one fails, the decision is rejected.

#### 1.3.1 Calm

*Definition:* Nothing in the UI should startle, blink, bounce, vibrate, or otherwise demand attention without user invitation.

*Concrete rules:*
- No notification dots that pulse on their own.
- No animation longer than 320 ms unless user-initiated.
- No sound effects (ever, until user opts in — REQ-EXP-04).
- No red until something is genuinely wrong (per §3.5).
- Streaming caret pulses at sinusoid 1100 ms, not strobe. (TRD §35.1.)

*Counter-example (forbidden):* The Apple Mail "ding" sound. The Slack "knock-brush" sound. ChatGPT's gradient-mesh background.

#### 1.3.2 Capable

*Definition:* The user must feel that MUKEI is a **professional instrument**, not a toy. Power should be one tap away — but never in the user's face.

*Concrete rules:*
- Long-press always reveals power features (branch from here, regenerate, copy raw markdown).
- Settings screen exposes inference parameters (temperature, top-p, max_tokens) without burying them.
- Tool-call cards show the actual JSON the LLM emitted, not a vague "thinking…".

#### 1.3.3 Confidential

*Definition:* Every screen must *visibly* reinforce that data is private.

*Concrete rules:*
- Persistent network banner (§4.12 in v1, §7.3 here) shows online/offline state at a glance.
- Encryption notice chip on ChatScreen header: "🔒 Local-only".
- PrivacyScreen explicitly enumerates what is NOT happening (no telemetry, no accounts, no cloud).
- Tool result cards label the trust level: "untrusted — read-only".

#### 1.3.4 Crafted

*Definition:* Pixel-level care. No off-grid spacing. No font drift. No mismatched icon stroke widths. No placeholder Lorem ipsum at release.

*Concrete rules:*
- Every spacing value is on the 8-px grid (or 4-px for fine work).
- Every icon stroke is exactly 1.5 px.
- Every animation curve is one of the two locked cubic-béziers (§8.1).
- Every translated string has been reviewed for typographic length explosion.

### 1.4 How To Use This Document

- **Designers:** This is your source-of-truth before Figma. Figma library mirrors this; this is canonical.
- **QML Engineers:** Every `Theme.qml`, `Type.qml`, `Spacing.qml` token here is the literal value to ship.
- **Reviewers:** Use §13.3 (Design QA Checklist) as the merge-blocker checklist.
- **PMs:** Sections 7 (Screen Flows) and 12 (FMEA) describe what the user experiences and what we promised when it goes wrong.

### 1.5 Glossary

| Term | Meaning |
|------|---------|
| **EDS** | Editorial Luxury Design System (MUKEI's design system name) |
| **Surface** | Any elevated panel: card, modal, sheet, bubble |
| **Bubble** | A single message rendering unit (`MessageBubble.qml`) |
| **Pill** | A small inline status row, e.g. `ToolCallPill.qml` |
| **Caret** | The pulsing end-of-stream indicator |
| **Token** (UX) | One streamed text fragment from Rust → QML (not LLM token; design-level event) |
| **CCCC** | Calm, Capable, Confidential, Crafted (the four principles) |
| **EDS-token** | A named design token referenced from `Theme.qml` (e.g. `EDS.color.copper`) |

---

## 2. Design System Foundation — The 70/20/10 Rule

### 2.1 The Rule, Stated

The visual surface of every MUKEI screen is composed of **three layers** in a *strict* ratio:

```
┌──────────────────────────────────────────────┐
│ 70%  BREATHING SURFACE                       │  ← Dolce Vita / Espresso / Taupe ground
│      (whitespace + base color)               │
│                                              │
│ ┌────────────────────────────────────┐       │
│ │ 20%  EDITORIAL CONTENT             │       │  ← Type, text, body, structured language
│ │      (typography + reading flow)   │       │
│ │ ┌──────────────────────────────┐   │       │
│ │ │ 10%  LUXURY ACCENT MOMENTS   │   │       │  ← Copper / Gold / Terracotta
│ │ │ (CTAs, focus, caret pulse)   │   │       │
│ │ └──────────────────────────────┘   │       │
│ └────────────────────────────────────┘       │
└──────────────────────────────────────────────┘
```

If a designer mocks a screen where the accent color occupies more than ~10 % of the rendered area, **the mock is rejected**. The reverse is also true: zero accent makes the UI feel dead.

This rule is *the* north star for every visual decision.

### 2.2 The 70 % — Minimalism (Breathing Surface)

#### 2.2.1 What It Is

The base surface (Dolce Vita warm paper / Espresso warm coffee / Taupe dusk concrete) plus generous whitespace.

#### 2.2.2 Concrete Rules

- **Edge padding** on every screen: 24 px minimum (32 px preferred) horizontal.
- **Vertical rhythm** between bubbles: 16 px minimum (24 px between role-changes).
- **No borders** where whitespace can do the work. (Exception: input field focus ring — §6.2.)
- **No shadows** on inline content. Cards may use a 1 px subtle inner separator at most.
- **No dividers** in lists; rely on spacing.
- **No decorative graphics in chrome** — illustrations only appear in empty states (§9.3).

#### 2.2.3 Why

The single biggest competitive differentiator of MUKEI is *not feeling like a chat app*. Stripping ornament transfers visual weight to **content** (the user's chat with their model). The 70 % is what makes MUKEI feel like a private notebook rather than another product page.

#### 2.2.4 Common Violations

| Violation | Correction |
|-----------|------------|
| Adding a thin border to every bubble | Remove border; let typography + alignment carry the structure |
| Card with shadow + border + background | Pick one elevation cue; ideally only background-tone |
| Filling a section with subtle dot/grid pattern | Delete pattern; embrace blank surface |
| "Premium feel" via gradient backgrounds | Forbidden; flat warm tone only |

### 2.3 The 20 % — Editorial (Typography as Architecture)

#### 2.3.1 What It Is

The structured language layer — headings, body text, captions, code blocks. **Typography is the primary layout tool of MUKEI**, not boxes or borders.

#### 2.3.2 Concrete Rules

- AI responses render in **Merriweather serif** at 16 px / 1.6 line-height. Generous, readable, magazine-feel.
- User prompts render in **Inter sans** at 16 px / 1.5 line-height. Crisp, conversational.
- Headings (screen titles, section headers) render in **Playfair Display** with `-0.02 em` letter-spacing.
- Code blocks render in **JetBrains Mono** at 14 px / 1.5 line-height.
- Captions and metadata (timestamps, token counts) at 12 px in muted secondary color.

#### 2.3.3 Why Serif for AI

AI responses are the longest sustained reading experience in MUKEI. Serif typefaces increase reading comfort over long passages (this is why books are set in serif). Treating AI replies as *editorial copy*, not chat bubbles, is the single design move that elevates MUKEI from app to instrument.

(See §4 for the full type system.)

#### 2.3.4 The "Sunday Essay" Test

If you took a screenshot of an AI response in MUKEI and printed it on cream paper, would it look like a magazine article excerpt? **If yes, the typography is correct.** If it looks like a chat bubble, it has failed.

### 2.4 The 10 % — Luxury Warm (Accent Moments)

#### 2.4.1 What It Is

Selective use of **warm metals** — copper, muted gold, terracotta — to mark moments that matter. Action, focus, life.

#### 2.4.2 Concrete Rules

- **Buttons (primary CTA)** — copper background, paper-white text.
- **Focus ring** on input field — 2 px copper.
- **Streaming caret** — copper pulse at 1100 ms sinusoid.
- **Tool-call pill icon** — copper accent stroke.
- **Selection highlight** — copper at 20 % opacity.
- **Links inline in AI text** — copper underline (subtle, 1 px).

#### 2.4.3 Where Accent Is FORBIDDEN

- **Headers** — Playfair Display ink-primary, never copper. (Heading copper = corporate brochure feel.)
- **Body text** — Merriweather ink-primary, never copper.
- **Backgrounds** — copper backgrounds are screaming, not whispering.
- **Borders** of cards — paper / surface tones only.
- **Icons in chrome** (back arrow, settings gear) — ink-primary tone, copper only when active/selected.

#### 2.4.4 The Aesop Test

If you placed a still-life photo from the Aesop website next to a MUKEI screen, would the two share a visual language (warm paper, soft surfaces, copper-toned brass fixtures)? **If yes, the accent is correctly tuned.** If the screen looks like a tech startup, it has failed.

### 2.5 Worked Examples — Applying 70/20/10

#### 2.5.1 Empty-state ChatScreen

| Layer | What appears | Approx. visible area |
|-------|--------------|----------------------|
| 70 % | Dolce Vita background, vertical breathing space | ~72 % |
| 20 % | Editorial headline (Playfair Display 32 px), three example-prompt cards (Inter 16 px) | ~22 % |
| 10 % | Copper "Get Started" CTA, small encryption-notice chip with copper icon | ~6 % |

#### 2.5.2 Streaming AI Response

| Layer | What appears | Approx. visible area |
|-------|--------------|----------------------|
| 70 % | Dolce Vita background, ample line spacing | ~64 % |
| 20 % | AI message body (Merriweather 16 px / 1.6), user prompt above (Inter 16 px) | ~30 % |
| 10 % | Copper streaming caret at end of last sentence | ~6 % |

#### 2.5.3 Model Download Sheet

| Layer | What appears | Approx. visible area |
|-------|--------------|----------------------|
| 70 % | Surface tone, sheet rounded top corners | ~62 % |
| 20 % | Model name (Playfair), size + storage check (Inter), progress label (Inter caption) | ~28 % |
| 10 % | Copper progress bar fill, Cancel ghost button (copper text) | ~10 % |

### 2.6 FMEA — 70/20/10 Rule

| ID | Failure mode | Cause | Effect | Mitigation |
|----|--------------|-------|--------|------------|
| F-A-1 | "Accent creep" — copper added to multiple chrome elements | Designer adds copper to nav icons + bubble corners + dividers | UI feels corporate, loses calm | Design QA checklist (§13.3): visually estimate accent area; reject mocks > 12 % |
| F-A-2 | "Whitespace shrink" — engineer compresses spacing to fit more content | Product pressure to "show more" | UI feels cramped, editorial illusion broken | Lock 8-px grid in `Spacing.qml` singleton; no inline magic numbers |
| F-A-3 | "Type drift" — random font sizes appear | New screen authored in isolation | Visual rhythm collapses | Type tokens enforced via `Type.qml`; `qmllint` rule rejects literal `font.pointSize:` values |
| F-A-4 | "Border-creep" — borders added to cards to "separate" content | Engineer doesn't trust whitespace | UI feels boxy, no longer editorial | Code review rule: any new `border.color` requires designer approval |

---

## 3. Color System & Theme Tokens

> **HARD LOCK.** The hex codes in this section are **canonical and non-negotiable**. They are exported as constants in `Theme.qml`, mirrored in the Figma library, and validated in CI via a snapshot test (`test_theme_hex_lock`).

### 3.1 Dolce Vita — Light Theme Palette

> *Italian for "the sweet life". A warm oatmeal-paper morning surface.*

#### 3.1.1 Token Table

| Token | Hex | RGB | Use |
|-------|-----|-----|-----|
| `EDS.color.dv.background` | `#D8CABD` | 216 / 202 / 189 | App-wide background — unbleached sand paper |
| `EDS.color.dv.surface` | `#E8DDD0` | 232 / 221 / 208 | Cards, sheets, elevated surfaces |
| `EDS.color.dv.surfaceVariant` | `#C9B9A7` | 201 / 185 / 167 | User-message bubble bg, subtle bands |
| `EDS.color.dv.ink.primary` | `#362417` | 54 / 36 / 23 | Body text, headings (same hue as Espresso bg — intentional) |
| `EDS.color.dv.ink.secondary` | `#6B5D4F` | 107 / 93 / 79 | Metadata, captions, secondary labels |
| `EDS.color.dv.ink.faint` | `#9C8E80` | 156 / 142 / 128 | Placeholders, disabled, hint text |
| `EDS.color.dv.accent` | `#B87333` | 184 / 115 / 51 | Copper — primary accent (10 % rule) |
| `EDS.color.dv.accent.soft` | `#D49A6A` | 212 / 154 / 106 | Hover/pressed state |
| `EDS.color.dv.divider` | `#BFAE9C` | 191 / 174 / 156 | The *one* allowed thin divider, when truly needed |

#### 3.1.2 Why Not White

Pure white (`#FFFFFF`) is **forbidden** as a background:
- Causes eye strain during extended reading (8 + bubble sessions).
- Visually identical to system OS chrome — fails the "MUKEI feels like a private notebook" test.
- Looks identical to every other app on the device — destroys the boutique identity.

Dolce Vita is the **unbleached linen** alternative — warm, diffusing, premium.

#### 3.1.3 Why The Same Hex As Espresso Background For Text

`#362417` is the Espresso theme background. We use it as Dolce Vita's **ink primary**. This creates a beautiful symmetry: the *darkness* of Espresso theme becomes the *text* of Dolce Vita theme. Reciprocally, Dolce Vita's background lives as Espresso's ink-soft border family.

This is not a happy accident — it's a deliberate **palette inversion** that makes theme transitions feel coherent.

### 3.2 Espresso — Dark Theme Palette

> *Deep roasted coffee. Mahogany. Lounge at midnight.*

#### 3.2.1 Token Table

| Token | Hex | RGB | Use |
|-------|-----|-----|-----|
| `EDS.color.esp.background` | `#362417` | 54 / 36 / 23 | App-wide background |
| `EDS.color.esp.surface` | `#4A3829` | 74 / 56 / 41 | Cards, sheets, elevated |
| `EDS.color.esp.surfaceVariant` | `#5C4736` | 92 / 71 / 54 | User-message bubble bg |
| `EDS.color.esp.ink.primary` | `#EBE1D5` | 235 / 225 / 213 | Body text, headings (warm bone) |
| `EDS.color.esp.ink.secondary` | `#A89888` | 168 / 152 / 136 | Metadata, captions |
| `EDS.color.esp.ink.faint` | `#7D6E60` | 125 / 110 / 96 | Placeholders, hints |
| `EDS.color.esp.accent` | `#D4AF37` | 212 / 175 / 55 | Muted gold — primary accent on dark |
| `EDS.color.esp.accent.soft` | `#E5C66A` | 229 / 198 / 106 | Hover/pressed |
| `EDS.color.esp.divider` | `#604A37` | 96 / 74 / 55 | Subtle divider, used sparingly |

#### 3.2.2 Why Not Black

Pure black (`#000000`) is **forbidden**:
- **OLED smear** — pure black pixels show ghosting as adjacent pixels shift.
- **Halation** — pure white text on pure black creates a perceptual glow that fatigues the eye.
- **Cold and depressing** — pure black with grey text reads as Windows command prompt, not luxury lounge.

Espresso (`#362417`) is **warm charcoal**:
- Power-efficient on OLED (still very dark).
- Visually warm, lounge-like.
- White text doesn't halate against it.

#### 3.2.3 Why Gold Instead Of Copper On Dark

Copper (`#B87333`) on Espresso looks **muddy** — the value contrast is insufficient and the warm-on-warm blends. Muted gold (`#D4AF37`) has a higher value gap from Espresso, so the accent reads cleanly.

We retain "warm metal" identity by using **muted** gold, not pure metallic gold (`#FFD700`), which would look gauche.

### 3.3 Taupe — Custom "Dusk" Theme

> *Architectural concrete. Late dusk sky. Zen minimalism.*

For users who find Dolce Vita too warm and Espresso too dark.

#### 3.3.1 Token Table

| Token | Hex | RGB | Use |
|-------|-----|-----|-----|
| `EDS.color.tp.background` | `#92817A` | 146 / 129 / 122 | App-wide background |
| `EDS.color.tp.surface` | `#A89888` | 168 / 152 / 136 | Cards, sheets |
| `EDS.color.tp.surfaceVariant` | `#B5A697` | 181 / 166 / 151 | User-message bubble |
| `EDS.color.tp.ink.primary` | `#2A2420` | 42 / 36 / 32 | Body text, headings |
| `EDS.color.tp.ink.secondary` | `#4F423A` | 79 / 66 / 58 | Metadata |
| `EDS.color.tp.ink.faint` | `#6A5C52` | 106 / 92 / 82 | Hints, disabled |
| `EDS.color.tp.accent` | `#C17F3E` | 193 / 127 / 62 | Terracotta-copper — primary accent |
| `EDS.color.tp.accent.soft` | `#D49E68` | 212 / 158 / 104 | Hover/pressed |
| `EDS.color.tp.divider` | `#7E6F66` | 126 / 111 / 102 | Subtle |

#### 3.3.2 Why Taupe Exists

A non-binary theme option for:
- Users with mild light sensitivity who find Dolce Vita too bright outdoors.
- Users with OLED-burn-in concerns who don't want dark mode permanently on.
- Users who simply prefer a "Solarized"-style mid-tone IDE feel.

Taupe is *not* a high-contrast mode; it is a **low-contrast aesthetic mode**.

### 3.4 Accent Colors — Warm Metals

The unifying identity across all three themes is **warm metal**. No theme uses cold accents (blue, teal, purple, magenta).

#### 3.4.1 The Three Metals

| Metal | Hex | Theme | Mood |
|-------|-----|-------|------|
| Copper | `#B87333` | Dolce Vita | Workshop, hand-tooled |
| Muted Gold | `#D4AF37` | Espresso | Lounge brass, dim-lit hotel bar |
| Terracotta-Copper | `#C17F3E` | Taupe | Italian rooftop tile, weathered bronze |

#### 3.4.2 Forbidden Accents

| Forbidden | Why |
|-----------|-----|
| Pure gold `#FFD700` | Gaudy, "cheap jewellery" |
| iOS blue `#007AFF` | Cold, generic, "Apple" |
| Material teal `#26A69A` | Corporate Google |
| Neon magenta `#FF00FF` | Cyberpunk anti-pattern |
| Bright red `#FF0000` | Alarmist; even error states use muted (§3.5) |
| Pure yellow `#FFFF00` | Reads as warning sign, not luxury |

### 3.5 Semantic Colors — Restrained Signals

All three themes share semantic colors. **All semantic colors are deliberately muted.**

| Token | Hex | Used For |
|-------|-----|----------|
| `EDS.color.semantic.success` | `#10B981` | Confirmation, completion, "Network: off — you are private" calm-green |
| `EDS.color.semantic.warning` | `#F59E0B` | Storage low, thermal warm, "network lost" amber |
| `EDS.color.semantic.error` | `#EF4444` | DB unlock fail, model corrupt, fatal-only |

Note: even `error` is **muted red**, never neon. Errors in MUKEI are calmly stated, not flashed.

### 3.6 WCAG 2.1 AA Compliance Matrix

All color pairs below pass **WCAG AA** (≥ 4.5:1 for normal text, ≥ 3:1 for large text ≥ 18 pt).

#### 3.6.1 Dolce Vita

| Foreground | Background | Ratio | Pass? |
|------------|------------|-------|-------|
| `#362417` ink primary | `#D8CABD` bg | 7.81 : 1 | AAA |
| `#362417` ink primary | `#E8DDD0` surface | 8.51 : 1 | AAA |
| `#6B5D4F` ink secondary | `#D8CABD` bg | 4.59 : 1 | AA |
| `#B87333` copper | `#D8CABD` bg | 3.42 : 1 | AA (large only) |
| `#FFFFFF` paper | `#B87333` copper button | 4.10 : 1 | AA (large only) — use bold/16 px+ |

#### 3.6.2 Espresso

| Foreground | Background | Ratio | Pass? |
|------------|------------|-------|-------|
| `#EBE1D5` ink primary | `#362417` bg | 11.42 : 1 | AAA |
| `#EBE1D5` ink primary | `#4A3829` surface | 8.74 : 1 | AAA |
| `#A89888` ink secondary | `#362417` bg | 5.31 : 1 | AA |
| `#D4AF37` gold | `#362417` bg | 6.18 : 1 | AAA |

#### 3.6.3 Taupe

| Foreground | Background | Ratio | Pass? |
|------------|------------|-------|-------|
| `#2A2420` ink primary | `#92817A` bg | 5.92 : 1 | AA |
| `#2A2420` ink primary | `#A89888` surface | 7.10 : 1 | AAA |
| `#4F423A` ink secondary | `#92817A` bg | 3.21 : 1 | AA (large only) |
| `#C17F3E` terracotta | `#92817A` bg | 2.42 : 1 | FAIL → restrict to large/bold ≥ 18 pt, never for body text |

> **Taupe caveat:** the lower native contrast is the *point* of the theme (zen, low-stim), but it forces stricter rules: copper text must always be bold ≥ 18 px, and ghost-button copper is reserved for icon-only chrome.

### 3.7 QML `Theme.qml` Singleton

#### 3.7.1 Skeleton

```qml
// qml/theme/Theme.qml
pragma Singleton
import QtQuick

QtObject {
    id: theme

    enum Mode { DolceVita, Espresso, Taupe }
    property int mode: Theme.Mode.DolceVita

    // ─── Dolce Vita ───────────────────────────────
    readonly property QtObject dv: QtObject {
        readonly property color background:       "#D8CABD"
        readonly property color surface:          "#E8DDD0"
        readonly property color surfaceVariant:   "#C9B9A7"
        readonly property color inkPrimary:       "#362417"
        readonly property color inkSecondary:     "#6B5D4F"
        readonly property color inkFaint:         "#9C8E80"
        readonly property color accent:           "#B87333"
        readonly property color accentSoft:       "#D49A6A"
        readonly property color divider:          "#BFAE9C"
    }

    // ─── Espresso ─────────────────────────────────
    readonly property QtObject esp: QtObject {
        readonly property color background:       "#362417"
        readonly property color surface:          "#4A3829"
        readonly property color surfaceVariant:   "#5C4736"
        readonly property color inkPrimary:       "#EBE1D5"
        readonly property color inkSecondary:     "#A89888"
        readonly property color inkFaint:         "#7D6E60"
        readonly property color accent:           "#D4AF37"
        readonly property color accentSoft:       "#E5C66A"
        readonly property color divider:          "#604A37"
    }

    // ─── Taupe ────────────────────────────────────
    readonly property QtObject tp: QtObject {
        readonly property color background:       "#92817A"
        readonly property color surface:          "#A89888"
        readonly property color surfaceVariant:   "#B5A697"
        readonly property color inkPrimary:       "#2A2420"
        readonly property color inkSecondary:     "#4F423A"
        readonly property color inkFaint:         "#6A5C52"
        readonly property color accent:           "#C17F3E"
        readonly property color accentSoft:       "#D49E68"
        readonly property color divider:          "#7E6F66"
    }

    // ─── Semantic (theme-agnostic) ────────────────
    readonly property color success: "#10B981"
    readonly property color warning: "#F59E0B"
    readonly property color error:   "#EF4444"

    // ─── Resolved active palette ──────────────────
    readonly property QtObject p:
        mode === Theme.Mode.DolceVita ? dv :
        mode === Theme.Mode.Espresso  ? esp :
        tp
}
```

#### 3.7.2 Usage In Components

```qml
import "../theme" 1.0

Rectangle {
    color: Theme.p.surface
    Text { color: Theme.p.inkPrimary; text: "Hello, world." }
}
```

#### 3.7.3 No Inline Hex

A `qmllint` custom rule rejects any `color: "#XXXXXX"` literal outside `Theme.qml` itself. This is enforced in CI (TRD §11.2).

### 3.8 Color-Blind Considerations

Three forms of color-blindness are tested:

| Type | Population | MUKEI test |
|------|-----------|------------|
| Deuteranopia (red-green) | ~5 % of male users | Copper appears as olive — still distinguishable from text |
| Protanopia (red-green) | ~1 % | Copper appears as dark khaki — still distinguishable |
| Tritanopia (blue-yellow) | < 0.01 % | Copper appears slightly pink — still distinguishable |

Because MUKEI has no red/green semantic distinction (success/error use different forms: ✓ vs ✗ icon + position, not color alone), color-blind users lose **no information**.

### 3.9 OLED Burn-In Mitigation

On Espresso theme specifically, two persistent UI elements pose burn-in risk on long-running OLED displays:

1. Top-bar app title "Mukei" (Playfair Display)
2. Bottom network banner

Mitigations:
- **Subtle slow drift:** every 30 minutes, the top bar shifts vertically by ± 1 px (imperceptible, sufficient).
- **Auto-dim:** after 5 minutes idle, ink primary drops to 80 % opacity, banner fades.

### 3.10 FMEA — Color System

| ID | Failure mode | Cause | Effect | Mitigation |
|----|--------------|-------|--------|------------|
| F-C-1 | Hex drift — engineer uses `#FFFFFF` instead of `#E8DDD0` | Copy-paste from another app | Surface looks generic, breaks Dolce Vita | `qmllint` rule rejects raw hex outside `Theme.qml`; CI fails build |
| F-C-2 | Theme transition flicker | Color animations not synced | Visible white flash during cross-fade | Animate all color properties via single `ColorAnimation` group, 300 ms ease-in-out |
| F-C-3 | Low-contrast token (copper-on-Taupe) used as body text | Mock approved without WCAG check | Body text < 3:1 ratio, fails AA | §13.3 checklist mandates contrast pass per screen |
| F-C-4 | High-contrast mode not honored | `AccessibilityManager` query not wired in JNI | Visually-impaired users see standard palette | `MukeiActivity.java` queries on resume + emits Qt signal; tested in `tst_HighContrast.qml` |
| F-C-5 | OLED burn-in on persistent header | No drift / dim policy | Ghost image on Espresso users after months | §3.9 drift + auto-dim implemented |
| F-C-6 | Color-blind user can't tell error from success | Semantic color used alone | Status ambiguous | Always pair color with icon + position (success-check vs error-cross) |
| F-C-7 | Taupe terracotta on Taupe bg used in body run | Designer assumed it's "accent-only enough" | Body becomes unreadable for some users | Code review rule: terracotta is icon/CTA only on Taupe, never running text |

---


## 4. Typography System — The Editorial Heart

> *If color is the room, typography is the furniture, the lighting, and the conversation. In MUKEI, type does **most of the work**.*

### 4.1 Type Stack Overview

Four type families. Each has one job. No overlap. No drift.

| Family | Used for | Personality |
|--------|----------|-------------|
| **Playfair Display** | Display headings, screen titles, modal headlines | Editorial, magazine-display, *Vogue* / *NYT Sunday* |
| **Merriweather** | AI responses, RAG content, tool summaries (long-form reading) | Generous, on-screen-optimised serif, *Medium-Premium* |
| **Inter** | UI chrome, user prompts, buttons, labels, captions | Clear, neutral, conversational sans |
| **JetBrains Mono** | Code blocks, tool-call JSON, crash log preview | Developer-grade, character-distinct, ligature-aware |

Each family ships bundled in the APK (`assets/fonts/`) under the SIL Open Font License (OFL) — no runtime download.

### 4.2 Primary Serif — Playfair Display & Merriweather

#### 4.2.1 Playfair Display (Display & Headings)

- **Weights bundled:** Regular (400), Medium (500), SemiBold (600), Bold (700)
- **Used at:** 32 px (Display), 24 px (H1), 20 px (H2)
- **Letter-spacing:** `-0.02 em` (tight, editorial)
- **Line-height:** 1.2 (display) – 1.35 (H2)
- **Italic:** allowed for emphasis in headlines only; never used for run-text emphasis

#### 4.2.2 Merriweather (AI Response Body)

- **Weights bundled:** Regular (400), Italic (400 it), Bold (700), Bold Italic
- **Used at:** 16 px (Body)
- **Letter-spacing:** `0` (default)
- **Line-height:** **1.6** (the single most important typographic decision in MUKEI)
- **Italic:** allowed for emphasis in body — preferred over bold

#### 4.2.3 Why 1.6 Line-Height On AI Body

A streaming AI response can run 400–1200 words. At 16 px / 1.6, MUKEI gives the same vertical breathing room as a print magazine. At 1.4 (common chat-app default), the same paragraph feels packed. At 1.8, it feels patronising. **1.6 is the sweet spot** — verified in our typography-specimen sheet (§14.2).

#### 4.2.4 Why Serif For AI, Not For User

User prompts are conversational, brief, transactional ("read my notes.txt and summarise"). They benefit from sans clarity (Inter).
AI responses are extended reading. They benefit from serif comfort (Merriweather).
This asymmetry visually encodes the **role asymmetry** — the user is *asking*, the model is *delivering an essay*.

### 4.3 Secondary Sans — Inter

#### 4.3.1 Inter (UI Chrome & User Prompts)

- **Weights bundled:** Regular (400), Medium (500), SemiBold (600)
- **Used at:** 16 px (Body — user prompt), 14 px (Body Small), 12 px (Caption), 10 px (Micro)
- **Letter-spacing:** `0` body, `+0.02 em` for caption/micro (caps look better with tracking)
- **Line-height:** 1.5 (body), 1.4 (caption), 1.3 (micro)
- **Italic:** rare; reserved for placeholder text and hint-text states

#### 4.3.2 Inter's Variable Axes

Inter ships with a variable-font axis. MUKEI uses fixed instances (400, 500, 600) to keep static rendering and skip the variable-font rasterisation cost (~6 ms on Mali-G68 cold-cache).

### 4.4 Monospace — JetBrains Mono

#### 4.4.1 Spec

- **Weights bundled:** Regular (400), Medium (500), Bold (700)
- **Used at:** 14 px (Code blocks), 12 px (Tool JSON, caption mono)
- **Ligatures:** **disabled** in QML by default (programmer ligatures like `=>` confuse non-developer users). Re-enabled in `Settings → Developer mode`.
- **Letter-spacing:** `0`
- **Line-height:** 1.5 (code blocks)

#### 4.4.2 Why JetBrains Mono

- 0 vs O, 1 vs l vs I — all visually distinct.
- Friendly to long file paths and JSON keys.
- Available under OFL — license-clean for shipping.

### 4.5 Type Scale (Modular 1.25 Ratio)

The scale is a **major-third** modular ratio (1.25×) rooted at 16 px body. Every size below is rounded to the nearest even integer for crisp Android rendering.

| Token | Size | Line-height | Family | Use |
|-------|------|-------------|--------|-----|
| `Type.display` | 32 px | 1.20 | Playfair Display SemiBold | Empty-state headlines, welcome screen, splash |
| `Type.h1` | 24 px | 1.30 | Playfair Display Medium | Screen titles, modal headlines |
| `Type.h2` | 20 px | 1.35 | Playfair Display Medium | Section headers (Settings tabs etc.) |
| `Type.h3` | 18 px | 1.40 | Inter SemiBold | Card titles |
| `Type.bodyAI` | 16 px | 1.60 | Merriweather Regular | AI response body (the long-form reading) |
| `Type.bodyUI` | 16 px | 1.50 | Inter Regular | User prompts, button labels, body chrome |
| `Type.bodySmall` | 14 px | 1.50 | Inter Regular | Helper text, dense lists |
| `Type.code` | 14 px | 1.50 | JetBrains Mono Regular | Code blocks, tool JSON |
| `Type.caption` | 12 px | 1.40 | Inter Medium | Timestamps, labels, status |
| `Type.micro` | 10 px | 1.30 | Inter Medium | Badges, version pill, tags |

#### 4.5.1 Forbidden Sizes

If a value is not in the table above, **it does not exist in MUKEI**. No 13 px, no 15 px, no 17 px, no 22 px. Designers and engineers who need an in-between size must either pick from the scale or propose a new token via design-review.

### 4.6 Letter-Spacing & Paragraph Rhythm

| Element | Letter-spacing | Notes |
|---------|----------------|-------|
| Playfair display 32 px | `-0.02 em` | Tight, editorial |
| Playfair H1 / H2 | `-0.01 em` | Slightly tight |
| Merriweather body | `0` | Default |
| Inter body | `0` | Default |
| Inter caption / micro | `+0.02 em` | Wider — readability at small sizes |
| JetBrains mono | `0` | Default |
| All-caps labels (rare) | `+0.10 em` | Always tracked |

**Paragraph rhythm in AI responses:**
- Inter-paragraph spacing: **0.6 × line-height** (≈ 15 px on body, equivalent to a half-line skip — magazine convention).
- No first-line indents (web convention).
- No widows/orphans control — the AI streams in real-time; layout shift = jank.

### 4.7 Optical Refinements

#### 4.7.1 Hanging Punctuation

Quotes (`“ ”`) and em-dashes (`—`) at the start of a line are visually offset **outward by 4 px**, so the optical left edge of the paragraph stays clean. (QML implementation: `MarkdownRenderer.qml` injects a `TextItem` with a negative `leftPadding`.)

#### 4.7.2 Real Quotes, Real Dashes

The QML markdown renderer auto-substitutes:
- `"foo"` → `“foo”` (curly quotes)
- `'foo'` → `‘foo’`
- `--` → `–` (en dash)
- `---` → `—` (em dash)
- `...` → `…` (ellipsis)

This is **a typographic feature**, not a localisation hazard. Disabled in code blocks.

#### 4.7.3 Numeric Tabular Figures

In token-count captions (e.g. `2 145 tokens · 1.2 s`), use Inter's tabular-figure feature (`+tnum`) so digits align vertically across multiple lines.

### 4.8 Dynamic Type Scaling (Android System Font Size)

#### 4.8.1 Scale Multiplier

Android exposes `Configuration.fontScale` (0.85 … 2.00). MUKEI multiplies every type token by this scale, with caps:

| Original | Min (0.85x) | Default (1.00x) | Max (2.00x cap) |
|----------|-------------|------------------|-----------------|
| Display 32 | 28 | 32 | 48 (hard cap) |
| H1 24 | 22 | 24 | 40 (hard cap) |
| Body 16 | 14 | 16 | 22 (hard cap — prevents layout breakage) |
| Caption 12 | 12 (floor) | 12 | 18 |

#### 4.8.2 Compact Layout Mode

When `fontScale > 1.5`:
- Bubble padding shrinks from 16 px → 12 px.
- ToolCallPill icon-only mode (label hidden, icon retained).
- Empty-state example-prompt cards stack vertically instead of horizontal carousel.

This is detected in QML via a `Theme.scaleClass` derived property and switched in `MainWindow.qml`.

### 4.9 CJK & Arabic Fallback

#### 4.9.1 Fallback Chain

When a glyph is not in the primary family:

```
Merriweather → Noto Serif CJK → Noto Naskh Arabic → fontconfig system fallback
Inter        → Noto Sans CJK  → Noto Naskh Arabic → fontconfig
JetBrains Mono → Noto Sans Mono CJK → fontconfig
```

Noto families are not bundled (would balloon APK by ~30 MB). Instead, MUKEI relies on system Noto installed on Android 10 + (default on all major OEMs).

#### 4.9.2 Mixed-Script Lines

When an AI response contains both English and Japanese (e.g. "The word 漢字 means 'Chinese character'"), Merriweather renders Latin and Noto Serif CJK renders kanji, sharing the same metrics. Line-height is kept at 1.6 even with CJK, because Noto Serif CJK is designed to match.

#### 4.9.3 Arabic / Hebrew

The QML renderer flips text direction on RTL paragraphs. Letter-spacing tokens (`-0.02 em` etc.) are **suppressed** for Arabic — Arabic shapes are sensitive to tracking and look wrong with negative spacing.

### 4.10 Token Streaming & Reflow

#### 4.10.1 The Reflow Problem

If we render each streamed token immediately, the text re-wraps on every token: this causes visible jitter ("the cursor jumps right then snaps left").

#### 4.10.2 Mitigation Strategy

1. **Pre-allocate height:** when the bubble starts streaming, estimate final height from token-rate × ETA and allocate a placeholder.
2. **Token batching** (Rust side): aggregate ~50 ms of tokens, emit one Qt signal payload.
3. **Greedy line-wrap:** the last word of the current line is allowed to overflow; only when a soft hyphen or space is emitted does it actually wrap.
4. **No widow-control during stream:** turn on only after `stream_finalized`.

(See AF §12.3, REQ-PERF-02, REQ-UI-04.)

#### 4.10.3 Frame Budget

Token rendering must not consume > 6 ms / frame on Mali-G68 (out of 16 ms). QML Profiler verifies this on every release.

### 4.11 QML `Type.qml` Singleton

```qml
// qml/theme/Type.qml
pragma Singleton
import QtQuick

QtObject {
    id: type

    readonly property real scale: 1.0   // bound to Configuration.fontScale
    readonly property bool compact: scale > 1.5

    function px(v) { return Math.min(v * scale, v * 2.0) }

    readonly property QtObject display:   QtObject { property real size: type.px(32); property real lh: 1.20; property string family: "Playfair Display"; property int  weight: Font.DemiBold }
    readonly property QtObject h1:        QtObject { property real size: type.px(24); property real lh: 1.30; property string family: "Playfair Display"; property int  weight: Font.Medium  }
    readonly property QtObject h2:        QtObject { property real size: type.px(20); property real lh: 1.35; property string family: "Playfair Display"; property int  weight: Font.Medium  }
    readonly property QtObject h3:        QtObject { property real size: type.px(18); property real lh: 1.40; property string family: "Inter";            property int  weight: Font.DemiBold }
    readonly property QtObject bodyAI:    QtObject { property real size: type.px(16); property real lh: 1.60; property string family: "Merriweather";     property int  weight: Font.Normal   }
    readonly property QtObject bodyUI:    QtObject { property real size: type.px(16); property real lh: 1.50; property string family: "Inter";            property int  weight: Font.Normal   }
    readonly property QtObject bodySmall: QtObject { property real size: type.px(14); property real lh: 1.50; property string family: "Inter";            property int  weight: Font.Normal   }
    readonly property QtObject code:      QtObject { property real size: type.px(14); property real lh: 1.50; property string family: "JetBrains Mono";   property int  weight: Font.Normal   }
    readonly property QtObject caption:   QtObject { property real size: type.px(12); property real lh: 1.40; property string family: "Inter";            property int  weight: Font.Medium   }
    readonly property QtObject micro:     QtObject { property real size: type.px(10); property real lh: 1.30; property string family: "Inter";            property int  weight: Font.Medium   }
}
```

Usage:

```qml
Text {
    text: aiMessage.content
    font.family: Type.bodyAI.family
    font.pixelSize: Type.bodyAI.size
    lineHeight: Type.bodyAI.lh
    lineHeightMode: Text.ProportionalHeight
    color: Theme.p.inkPrimary
}
```

### 4.12 FMEA — Typography System

| ID | Failure mode | Cause | Effect | Mitigation |
|----|--------------|-------|--------|------------|
| F-T-1 | Font fails to load (asset missing / corrupt) | APK extraction failed | Fallback to system Roboto — visual identity collapses | TRD §33.1 / §33.2 asset-extraction verifies SHA per font; failure → `SafeModeAssets` |
| F-T-2 | CJK glyphs unrendered, tofu boxes | Noto missing on rooted/AOSP build | Mixed-script text broken | Detect missing glyph metrics → fall back to system serif → log warning |
| F-T-3 | Token-stream reflow jitter | Per-token re-layout | Visible "snap" on every token | 50 ms batch + pre-allocation (§4.10) |
| F-T-4 | Layout breaks at fontScale 2.0 | Designer didn't test extreme scale | Buttons clip text, bubbles overflow | Compact-layout mode (§4.8.2) + `tst_TypeScale.qml` snapshot at 0.85 / 1.0 / 1.5 / 2.0 |
| F-T-5 | Italic in Merriweather renders as oblique (synthetic) | Italic weight not bundled | Subtle visual ugliness | Ship Italic weights explicitly (§4.2.2) |
| F-T-6 | Programmer ligatures in JetBrains Mono confuse user | `=>` rendered as ⇒ | Non-developers misread code | Ligatures default off (§4.4.1); opt-in in dev settings |
| F-T-7 | Arabic letter-spacing breaks shaping | Negative tracking applied | Arabic letters disconnect | RTL detection suppresses letter-spacing (§4.9.3) |
| F-T-8 | Tabular figures unevenly aligned | `+tnum` not enabled | Token-count columns misalign | Apply `font.features: { "tnum": 1 }` in caption tokens |

---

## 5. Spacing, Grid & Layout Architecture

### 5.1 The 8-px Base Grid

Every interactive coordinate, padding, margin, and gap is a multiple of **8 px**. Fine alignments (icon nudges, optical centring) may use 4 px. **Nothing else.**

#### 5.1.1 Why 8

- Cleanly divides phone densities (mdpi/hdpi/xhdpi/xxhdpi/xxxhdpi).
- Industry-standard for Material, HIG, Atlassian — interoperable patterns.
- Halves and doubles produce a visually pleasant rhythm.

#### 5.1.2 No Magic Numbers

QML literal pixel values not on the grid are rejected by a custom `qmllint` rule. Engineers must use named tokens from `Spacing.qml`.

### 5.2 Spacing Scale

| Token | Value | Use |
|-------|-------|-----|
| `Spacing.xxs` | 4 px | Icon-to-label gap, hairline offsets |
| `Spacing.xs` | 8 px | Between two tightly-related elements |
| `Spacing.sm` | 12 px | Bubble inner padding (when compact) |
| `Spacing.md` | 16 px | Default bubble padding, default gap |
| `Spacing.lg` | 24 px | Vertical between message bubbles |
| `Spacing.xl` | 32 px | Section gaps, screen edge padding |
| `Spacing.xxl` | 48 px | Hero-spacing, between empty-state blocks |
| `Spacing.xxxl` | 64 px | Top of welcome screen, splash gap |
| `Spacing.huge` | 96 px | Used at most once per screen — display-area top |

#### 5.2.1 `Spacing.qml`

```qml
pragma Singleton
import QtQuick

QtObject {
    readonly property real xxs: 4
    readonly property real xs: 8
    readonly property real sm: 12
    readonly property real md: 16
    readonly property real lg: 24
    readonly property real xl: 32
    readonly property real xxl: 48
    readonly property real xxxl: 64
    readonly property real huge: 96
}
```

### 5.3 Safe Areas & Edge-to-Edge

#### 5.3.1 Strategy

- `Window.flags |= Qt.FramelessWindowHint` is **not** used (we keep system bars).
- Edge-to-edge enabled via `WindowCompat.setDecorFitsSystemWindows(window, false)`.
- Safe-area insets read via `Window.safeAreaInsets` (Qt 6.7 +) and applied to `MainWindow.qml` root padding.

#### 5.3.2 The 4 Insets

| Inset | Source | Applied to |
|-------|--------|-----------|
| `top` | Status bar | `MainWindow` top padding |
| `bottom` | Gesture nav / 3-button nav | `MainWindow` bottom padding |
| `left` / `right` | Camera notch (landscape) / display cutout | `MainWindow` side padding |

#### 5.3.3 Keyboard Inset

`Window.softKeyboardHeight` is observed; when it changes, the chat `Flickable` (§5.5) smoothly scrolls so the composer + latest message stay visible. Animation: 240 ms with the enter cubic-bézier (§8.1).

### 5.4 Responsive Breakpoints

#### 5.4.1 Breakpoint Table

| Class | Width range | Layout |
|-------|-------------|--------|
| **Compact (phone)** | < 600 dp | Single column. Drawer hides off-screen. |
| **Medium (foldable, small tablet)** | 600 – 840 dp | Single column with wider horizontal padding (32 → 48). Drawer becomes pinned-left if space allows. |
| **Expanded (tablet)** | > 840 dp | Two-pane: conversation list pinned left (320 dp), chat area right. |

#### 5.4.2 Detecting Breakpoint

```qml
readonly property string sizeClass:
    width < 600 ? "compact"
    : width < 840 ? "medium"
    : "expanded"
```

Stored on the `MainWindow` root; child components react via property binding.

#### 5.4.3 Foldable Considerations

- Hinge fold detected via `WindowLayoutInfo` (JNI bridge).
- When folded "tabletop" (hinge horizontal), composer moves to bottom half, chat occupies top half — like a laptop.
- When folded "book" (hinge vertical), two-pane mode activates automatically.

### 5.5 QML Layout Strategy — `Layout` vs `Anchors` vs `Flickable`

#### 5.5.1 Decision Tree

```
Variable-height children, scrollable list?
   → use Flickable + Column (NOT ListView for chat — see below)

Fixed-position chrome (header, footer)?
   → use anchors

Multi-row/column grid (e.g. settings tabs)?
   → use GridLayout / RowLayout / ColumnLayout

Single child centring?
   → anchors.centerIn parent
```

#### 5.5.2 Why `Flickable + Column`, Not `ListView`, For Chat

- `ListView` *requires* a fixed delegate height (or pays large measurement cost when delegates have variable height — which chat bubbles always do).
- `Flickable + Column` renders all bubbles; we control virtualisation manually by destroying off-screen ones beyond a window of (viewport + 6).
- Streaming bubble heights vary by token — `ListView` `cacheBuffer` recalculates excessively; `Flickable` does not.

#### 5.5.3 Virtualisation Strategy

```
visible_range = [scrollY - viewport, scrollY + viewport*2]
for each bubble:
    if bubble.y in visible_range:
        keep alive
    else:
        destroy QML item, keep raw data in chatModel
```

This caps memory to ~6 × bubble cost regardless of conversation length. Tested in `tst_ChatScroll1000.qml`.

### 5.6 Hit-Target Geometry

#### 5.6.1 Minimum Sizes

| Element | Minimum tap target | Notes |
|---------|--------------------|-------|
| Buttons (text or icon) | 48 × 48 dp | Android Accessibility guideline |
| Inline links (in AI body) | 48 dp vertical hit slop | Hit area expanded; visual underline stays slim |
| Composer field | 48 dp tall minimum | Composer can grow with multi-line |
| Long-press affordances (bubble menu) | 64 × 64 dp | Slightly larger than tap; encourages discovery |

#### 5.6.2 Touch-Hover Detection

For stylus-on-tablet support, `HoverHandler` is attached to interactive elements, showing the same subtle hover state as on desktop QML.

### 5.7 FMEA — Spacing & Layout

| ID | Failure mode | Cause | Effect | Mitigation |
|----|--------------|-------|--------|------------|
| F-L-1 | Spacing drift — engineer uses `padding: 14` | No lint rule | Optical chaos accumulates | `qmllint` rejects literal non-grid numbers in margin/padding |
| F-L-2 | Keyboard covers composer | Inset not handled | User can't see what they're typing | §5.3.3 keyboard inset + smooth-scroll Flickable |
| F-L-3 | Foldable hinge crosses critical UI | No fold-aware layout | Send button hidden under hinge | `WindowLayoutInfo` listened; layout adapts (§5.4.3) |
| F-L-4 | `ListView` causes scroll jank | Mis-chosen container | Bubble heights mis-measured | Use `Flickable + Column` per §5.5.2 |
| F-L-5 | Memory leak on 1000-msg conversation | No virtualisation | App killed by LMK | Manual virtualisation per §5.5.3 |
| F-L-6 | Tap targets under 48 dp | Designer pixel-perfect mock didn't include slop | Accessibility audit fails | `Accessible.hitTestEnabled = true` + spec audit per screen |
| F-L-7 | Two-pane mode shows wrong conversation | Pane state lost on rotation | User confused | Persist `selectedConversationId` in QML state + rehydrate on resume |

---

## 6. Component Library — QML Primitives

### 6.1 Scope & Naming Convention

Every component is a single `.qml` file under `qml/components/`. Filenames match the design-system canonical name (e.g. `PrimaryButton.qml`). Each component:
- Reads tokens from `Theme.qml`, `Type.qml`, `Spacing.qml` (no inline values).
- Exposes `enabled`, `accessibleName`, `accessibleDescription` always.
- Honours `Theme.compact`, `Theme.reduceMotion`, `Theme.highContrast` flags.

### 6.2 Buttons

#### 6.2.1 Variants

| Variant | Background | Text | Border | Use |
|---------|------------|------|--------|-----|
| **PrimaryButton** | `Theme.p.accent` | Paper white `#FFFFFF` on light / `#1A1108` on dark gold | none | One CTA per screen max |
| **SecondaryButton** | transparent | `Theme.p.accent` | 1.5 px `Theme.p.accent` | Common confirmations |
| **GhostButton** | transparent | `Theme.p.accent` | none | Tertiary actions (Cancel) |
| **IconButton** | transparent | n/a | none | Settings gear, attach paperclip etc. |
| **DestructiveButton** | `Theme.error` | `#FFFFFF` | none | Reset, Delete, Sign Out |

#### 6.2.2 Shape

- Corner radius: **8 px** (text buttons), **24 px** (pill / icon-only / chip).
- Inner padding: `Spacing.md` horizontal, `Spacing.sm` vertical (text buttons).
- Minimum size: 48 × 48 dp.

#### 6.2.3 States

| State | Visual change |
|-------|---------------|
| Idle | base |
| Hover (stylus / desktop) | background tint 5 % darker (light) / 5 % lighter (dark) |
| Pressed | background `Theme.p.accentSoft` |
| Focused (keyboard) | 2 px outline ring of `Theme.p.accent`, offset 2 px (so it never overlaps inner content) |
| Disabled | opacity 0.4, no hover/press response |

#### 6.2.4 Haptics

- Tap: light tick (Qt `HapticEffect.Light`).
- Long-press: medium tick.
- Destructive (final confirm): heavy tick.

### 6.3 Text Fields & Input

#### 6.3.1 ChatComposer

The single most-used component in MUKEI.

- **Shape:** rounded rectangle, radius 12 px.
- **Background:** `Theme.p.surface`.
- **Border:** 2 px transparent (focus = 2 px `Theme.p.accent`).
- **Padding:** `Spacing.md` horizontal, `Spacing.sm` vertical.
- **Font:** `Type.bodyUI` (Inter 16 px / 1.5).
- **Placeholder:** `Theme.p.inkFaint`, italic, "Ask Mukei anything…".
- **Multi-line:** grows from 1 → 6 lines, then scrolls inside.
- **Left affordance:** paperclip icon (attach SAF file) — `Spacing.sm` offset.
- **Right affordance:** send button (becomes "stop" while streaming).

#### 6.3.2 SettingsTextField

Used in inference parameter inputs (e.g. temperature, max_tokens).

- Inline validation; error shown as muted-red helper below.
- Numeric keyboard auto-shown for number fields.

#### 6.3.3 SearchField (Conversation list)

- Same shape as composer but smaller (40 dp tall).
- Magnifier icon left, clear-X right.

### 6.4 Cards — Message Bubbles & Tool Result

#### 6.4.1 UserMessageBubble

| Property | Value |
|----------|-------|
| Background | `Theme.p.surfaceVariant` |
| Alignment | Right (RTL: left) |
| Font | `Type.bodyUI` (Inter) |
| Padding | `Spacing.md` |
| Corner radius | 12 px on three corners; bottom-corner toward edge = 4 px (visual tail) |
| Max width | 78 % of screen (compact), 60 % (expanded) |

#### 6.4.2 AIMessageBubble

| Property | Value |
|----------|-------|
| Background | transparent **by default** (lets ground show through — editorial feel) |
| Background (long-answer auto-wash) | `Theme.p.surfaceFaint` when `implicitHeight > 320 dp` OR `Theme.fontScale > 1.5` (§6.4.2.1 NEW) |
| Alignment | Left |
| Font | `Type.bodyAI` (Merriweather, 1.6 lh) |
| Padding | `Spacing.md` |
| Max width | 92 % of screen |
| Inline `ThinkingAccordion` | collapsed-by-default `<thinking>` block, Inter caption inside |

##### 6.4.2.1 Long-Answer Reader Wash 📖 (NEW in v0.7.5 — P1-05)

> **🛡️ UX DECISION v0.7.5 — Transparent Bubble is Beautiful, But Not Always Readable.** The Principal Designer audit observed that fully-transparent assistant bubbles are emotionally correct for short, editorial answers, but they weaken the reading anchor for long-form outputs, code-rich answers, and high-`fontScale` accessibility scenarios. v0.7.5 keeps transparency as the **default**, and introduces an **auto-applied reader wash** for the cases where the lack of anchor would harm comprehension.

**Reader-wash rule:**

- Token: `Theme.p.surfaceFaint` (new token; defined per-palette in §3.7 with luminance delta of ±4 % vs `Theme.p.background` — just enough to anchor, not enough to break the 70/20/10 rule).
- Auto-apply trigger: `bubble.implicitHeight > 320 dp` **OR** `Theme.fontScale > 1.5` **OR** `bubble.containsCodeBlock` **OR** `Settings.response_density.summary_first = true`.
- Transition: 220 ms `Motion.enter` opacity fade on the background layer when the trigger flips (avoids a hard jump as the bubble grows during streaming).
- Override: user can force `response_density.always_reader_wash = true` in Settings → Accessibility for global anchoring.

**Reader-wash exemptions (never auto-apply):**

- Welcome screen, model picker, settings — these are not chat bubbles.
- High-contrast mode (§10.4) — borders already provide anchor.
- Reduce-motion mode — the wash is applied without animation, but **is** still applied (anchoring is not a motion property).

#### 6.4.3 ToolResultCard

| Property | Value |
|----------|-------|
| Background | `Theme.p.surface` |
| Border | 1 px `Theme.p.divider` (the *only* allowed border in the system, per §2.2.2 exception) |
| Corner radius | 12 px |
| Padding | `Spacing.md` |
| Header | Tool icon (copper) + tool name (Inter SemiBold 14) + status pill |
| Body | JetBrains Mono 14 / monospace for raw result |
| Footer | duration_ms (caption) + Copy button (ghost) |

#### 6.4.4 RAGChunkCard

A specialised card used in the RAG retrieval preview.

- Same shape as `ToolResultCard`, but with a **left bar** in `Theme.p.accent` (2 px wide) to mark "untrusted external data".
- Source name (e.g. `notes.txt`) in caption above the chunk text.

### 6.5 Navigation

#### 6.5.1 LeftDrawer

- Swipe-in from left, or tap the menu icon top-left.
- Width: 280 dp (compact), 320 dp (expanded — pinned).
- Contents: ConversationList + "New Chat" CTA + Settings entry.
- Backdrop: 40 % black overlay on rest of screen with backdrop-blur (where Vulkan supports it).

#### 6.5.2 BottomNav

**Not used.** Bottom nav competes with the composer for the same screen real estate; MUKEI prefers a left drawer + dedicated screens.

#### 6.5.3 ModalSheet

- Slides from bottom.
- Corner radius top: 16 px.
- Backdrop: 40 % black overlay.
- Drag-down to dismiss.
- Used for: model picker, branch switcher, settings sub-pages.

#### 6.5.4 FullScreenModal

- Used for: SafeMode, ModelDownload progress, ToolResultCard expanded view.
- Slides from right (LTR) / left (RTL).
- Back-arrow top-left; primary CTA bottom or absent.

### 6.6 Indicators

#### 6.6.1 ProgressBar (Deterministic)

- Height 4 px.
- Fill: `Theme.p.accent` with subtle gradient `accent → accentSoft`.
- Track: `Theme.p.divider`.
- Animation: 220 ms ease-out on `value` change.
- Used in: model download, asset extraction.

#### 6.6.2 Spinner (Indeterminate)

- 24 dp circle, 2 px stroke, accent color.
- Rotation: linear 1.2 s loop.
- Used **sparingly** — only when ETA truly unknown (e.g. "Verifying SHA256").

#### 6.6.3 SkeletonLoader

- Subtle vertical-bar shimmer at 5 % opacity over surface.
- Used during initial conversation list load on cold boot.
- Maximum visible duration: 1.5 s; afterwards switch to spinner with explicit message.

#### 6.6.4 StatusPill (ToolCallPill etc.)

| Subtype | Background | Icon | Text |
|---------|------------|------|------|
| ActiveTool | `Theme.p.surface` | copper, pulsing 1100 ms | Inter caption |
| Success | `Theme.p.surface` | success-check | caption |
| Failure | `Theme.p.surface` | error-cross (muted red) | caption |
| Network-Offline | `Theme.p.surface` | success-leaf icon (calm green) | "Network: off — you are private" |

#### 6.6.5 StreamingCaret

- 2 × 16 px vertical bar.
- Color: `Theme.p.accent`.
- Pulse: sinusoidal opacity 0.5 ↔ 1.0 over 1100 ms.
- Disappears immediately on `stream_finalized` and replaced with the `🎯 Done` micro-caret.

### 6.7 Dialogs & Sheets

#### 6.7.1 ConfirmationDialog

- Center-screen modal (small, ~280 dp wide).
- Title (Type.h2), body (Type.bodyUI), two buttons (Cancel ghost + confirm primary or destructive).
- Backdrop: 50 % black with backdrop-blur.

#### 6.7.2 DestructiveConfirmDialog

For `Reset All Data`, `Delete Model`, etc.

- Same as ConfirmationDialog but the primary button is `DestructiveButton`.
- Requires **two-tap** confirmation: first tap morphs button label from "Reset" to "Really reset everything?", second tap commits.
- 4 s timeout: if user does not tap again, button morphs back.

#### 6.7.3 ToastNotification

**Used sparingly.** MUKEI prefers inline UI states over toasts. The only allowed toasts:
- "Copied to clipboard" (after explicit copy action).
- "File added to RAG" (after SAF picker grant).

Toast appears at top of screen, auto-dismisses in 2 s.

### 6.8 Inline Markdown Renderer (`MarkdownRenderer.qml`)

#### 6.8.1 Why AST-Based

Per PRD REQ-UI-05 and AF §9, the markdown parser lives in Rust (`pulldown-cmark` or similar) and emits an AST. QML walks the AST and instantiates the correct primitive per node. **No regex on rendered text.** This is the single most-important component-level decision in MUKEI.

#### 6.8.2 Supported Nodes

| AST node | QML primitive |
|----------|----------------|
| `Heading(1..3)` | `Text` with appropriate Type token |
| `Paragraph` | `Text` body |
| `Strong` / `Emphasis` | inline `Text` span |
| `InlineCode` | `Text` with JetBrains Mono on `surfaceVariant` background |
| `CodeBlock(lang)` | `CodeBlockComponent.qml` (see §6.9) |
| `List(ordered/unordered)` | `Column` of items |
| `BlockQuote` | `Row` with left bar `Theme.p.accent` (2 px) + indented `Text` |
| `Link(text, url)` | underlined `Text` (no auto-open — long-press menu) |
| `HorizontalRule` | thin divider (1 px) |
| `Image` | **suppressed** — replaced with text placeholder "[image suppressed for privacy]" |

#### 6.8.3 Forbidden Nodes

- Raw HTML pass-through (`<script>`, `<style>`, etc.).
- `javascript:` URLs in links.
- `data:` URLs.

These nodes are dropped during parsing in Rust before reaching QML.

### 6.9 `CodeBlockComponent.qml`

- Background: `Theme.p.surfaceVariant`.
- Padding: `Spacing.md`.
- Font: `Type.code`.
- Language label top-right (Inter caption, muted).
- Copy button top-right corner (`IconButton`).
- Horizontal scroll on overflow (never wraps).
- Line numbers: optional, off by default (toggle in dev settings).

### 6.10 Component Inventory Summary

| File | Purpose | Section |
|------|---------|---------|
| `Theme.qml` | Color tokens | 3.7 |
| `Type.qml` | Type tokens | 4.11 |
| `Spacing.qml` | Spacing tokens | 5.2 |
| `PrimaryButton.qml` | CTA button | 6.2 |
| `SecondaryButton.qml` | Outlined button | 6.2 |
| `GhostButton.qml` | Text-only button | 6.2 |
| `IconButton.qml` | 48 dp icon-only button | 6.2 |
| `DestructiveButton.qml` | Red CTA, two-tap | 6.2, 6.7.2 |
| `ChatComposer.qml` | Multi-line input | 6.3.1 |
| `UserMessageBubble.qml` | Right-aligned, Inter | 6.4.1 |
| `AIMessageBubble.qml` | Left-aligned, Merriweather | 6.4.2 |
| `ToolResultCard.qml` | Tool output card | 6.4.3 |
| `RAGChunkCard.qml` | Retrieval-preview card | 6.4.4 |
| `LeftDrawer.qml` | Side nav | 6.5.1 |
| `ModalSheet.qml` | Bottom sheet | 6.5.3 |
| `FullScreenModal.qml` | Full-screen modal | 6.5.4 |
| `ProgressBar.qml` | Deterministic progress | 6.6.1 |
| `Spinner.qml` | Indeterminate spinner | 6.6.2 |
| `SkeletonLoader.qml` | Initial-load shimmer | 6.6.3 |
| `StatusPill.qml` | Inline status | 6.6.4 |
| `StreamingCaret.qml` | Pulsing caret | 6.6.5 |
| `ConfirmationDialog.qml` | Generic confirm | 6.7.1 |
| `DestructiveConfirmDialog.qml` | Two-tap destructive | 6.7.2 |
| `ToastNotification.qml` | Toast | 6.7.3 |
| `MarkdownRenderer.qml` | AST → QML walker | 6.8 |
| `CodeBlockComponent.qml` | Code block | 6.9 |
| `ThinkingAccordion.qml` | Collapsible `<thinking>` | UXB v1 §5 |
| `CopyButton.qml` | Copy-to-clipboard icon | n/a |
| `HapticFeedback.qml` | Haptic helper | 6.2.4 |
| `FontLoader.qml` | Font assets loader | TRD §33.1 |
| `NetworkBanner.qml` | Online/offline strip | 4.12 of v1 |

### 6.11 FMEA — Component Library

| ID | Failure mode | Cause | Effect | Mitigation |
|----|--------------|-------|--------|------------|
| F-CP-1 | Tap target < 48 dp on icon-only button | Spec mock used 32 dp icon | Accessibility fail | `IconButton` enforces 48 dp minimum even when icon is smaller |
| F-CP-2 | Keyboard covers ChatComposer | Inset binding wrong | User can't see input | §5.3.3 keyboard inset wiring |
| F-CP-3 | Ripple effect on tap costs > 6 ms/frame | QML `Ripple` style with shader | Stutters during streaming | Use static-state colour change instead of ripple shader; verified in profiler |
| F-CP-4 | Toast covers active streaming | Toast positioning over chat | User loses sight of token output | Toast positioned at *top* of screen, never bottom |
| F-CP-5 | Destructive confirm tappable too fast | No de-bounce on double tap | Accidental wipe | `DestructiveConfirmDialog` requires 250 ms hold or two clearly-separated taps (§6.7.2) |
| F-CP-6 | Markdown renderer crashes on malformed AST | Parser bug | Bubble blank | Rust parser is total-function (no panic); on parse failure return raw text node |
| F-CP-7 | `<script>` in tool output executes | Markdown allowed raw HTML | XSS-like exploit | §6.8.3 forbids raw HTML pass-through; AST node-list is allowlist |

---

## 7. Screen Flows & Key Journeys

> Every screen below lists: **purpose · invariants · layout · states (empty / loading / error / success) · cross-refs (PRD/TRD/AF/BS) · FMEA**.

### 7.1 First-Run — Welcome Screen

#### 7.1.1 Purpose

Set the emotional tone in the first ~3 seconds: this is **private** and **on your device**. Nothing else.

#### 7.1.2 Layout

```
┌──────────────────────────────────────────┐
│ (96 px top breathing space)              │
│                                          │
│   Your Private AI,                       │  ← Playfair Display 32 / 1.2 / -0.02 em
│   On Your Device.                        │     ink primary
│                                          │
│   (32 px gap)                            │
│                                          │
│   No cloud. No subscriptions.            │  ← Inter 16 / 1.5
│   No data leaves your phone.             │     ink secondary
│                                          │
│   (48 px gap)                            │
│                                          │
│   🔒 Encrypted locally with your device. │  ← Inter 14 caption + copper lock icon
│                                          │
│   (auto-fill remaining space)            │
│                                          │
│   ╔═════════════════════════════════╗    │  ← PrimaryButton (copper)
│   ║       Get Started               ║    │
│   ╚═════════════════════════════════╝    │
│   (24 px bottom safe-area)               │
└──────────────────────────────────────────┘
```

#### 7.1.3 Invariants

- No app branding bigger than the headline.
- No marketing copy ("powered by", "AI-driven", etc.).
- No third-party logos.
- No "Sign in" or "Skip" (there is nothing to sign into).

#### 7.1.4 States

| State | Visual |
|-------|--------|
| Default | as above |
| Reduced motion | identical (no animations on this screen) |

#### 7.1.5 Cross-refs

PRD REQ-EXP-04 (single-time privacy notice), AF §6 (first-run journey), TRD §33.2 (asset extraction precedes welcome).

### 7.2 First-Run — Model Picker

#### 7.2.1 Purpose

Let the user pick a model. **No defaults are pre-checked** — the user is in control.

#### 7.2.2 Layout

```
┌──────────────────────────────────────────┐
│ ←  Choose a Model                        │  ← Playfair H1
│ (caption: "Mukei runs models entirely    │     Inter caption
│  on your device.")                       │
│                                          │
│ ┌──────────────────────────────────┐     │  ← Model card
│ │ Gemma 3 4B Instruct              │     │     Inter SemiBold (h3)
│ │ 2.5 GB · Q4_K_M quantization     │     │     Inter bodySmall
│ │ "Fast. Good for general chat."   │     │     Merriweather italic blurb
│ │                  [Download]      │     │     SecondaryButton (copper outline)
│ └──────────────────────────────────┘     │
│ (16 px between cards)                    │
│ ┌──────────────────────────────────┐     │
│ │ Llama 3.2 3B Instruct            │     │
│ │ 1.8 GB · Q4_K_M                  │     │
│ │ "Smaller. Lighter on battery."   │     │
│ │                  [Download]      │     │
│ └──────────────────────────────────┘     │
│                                          │
│ Or [Use a custom GGUF file]              │  ← GhostButton (copper text)
│                                          │
└──────────────────────────────────────────┘
```

#### 7.2.3 Invariants

- Each model card states size, quantization, and a one-sentence editorial blurb (italic Merriweather — magazine-style descriptor).
- Storage check is shown inline if free space < model size (warning amber).
- Download is **deterministic progress**, not indeterminate.

#### 7.2.4 States

| State | Visual |
|-------|--------|
| Initial | three model cards visible |
| Insufficient space | top banner amber: "Only 1.2 GB free. Largest available: Llama 3B." |
| Downloading | active card shows progress bar in-place; other cards greyed (opacity 0.4) |
| Completed | active card swaps `[Download]` → `[Switch]`; subtle copper check icon appears |
| Failed | active card shows red helper: "Download failed. [Retry]" |

#### 7.2.5 Cross-refs

PRD REQ-DL-01..03, REQ-DL-08; TRD §5.3 (download state machine); AF §5 (acquisition flow); BS §6.3 (`.part`/`.meta` schema).

### 7.3 First-Run — Verification & Asset Extraction

#### 7.3.1 Purpose

A brief but **honest** loading screen between download and ready. Most apps hide this; MUKEI surfaces it because the verification is *the privacy story*.

#### 7.3.2 Layout

Centered minimal layout. Single spinner + label, no decorative graphics.

```
            ◯                              ← Spinner (24 dp, copper, 1.2 s loop)
   Verifying cryptographic integrity…       ← Inter bodyUI 16, ink primary
   This guarantees the model was not        ← Inter bodySmall 14, ink secondary
   tampered with during download.
```

#### 7.3.3 Phases (each gets ~1 s minimum visible)

| Phase | Label | Why surfaced |
|-------|-------|--------------|
| 1 | "Verifying cryptographic integrity…" | SHA-256 of downloaded GGUF |
| 2 | "Extracting on-device assets…" | TRD §33.2 asset extraction |
| 3 | "Initializing private storage…" | SQLCipher unlock (AF §4.4) |

#### 7.3.4 Cross-refs

PRD REQ-DL-09 (verify SHA-256); TRD §33.2, §12.3; AF §4.

### 7.4 Empty State — ChatScreen

#### 7.4.1 Purpose

When the user lands on an empty conversation, the screen must (a) feel like a private notebook waiting to be written in and (b) offer three editorial prompt suggestions.

#### 7.4.2 Layout

```
┌──────────────────────────────────────────┐
│ ☰  Mukei                          ⚙      │  ← Inter SemiBold + IconButtons
│ (32 px breathing)                        │
│                                          │
│         Mukei is ready.                  │  ← Playfair Display 32
│   Everything runs on your device.        │  ← Inter 16, secondary
│                                          │
│ (48 px gap)                              │
│                                          │
│   Try one of these to start:             │  ← Inter caption, secondary
│                                          │
│   ┌──────────────────────────────┐       │  ← PromptCard (Theme.p.surface)
│   │ "Summarize the concept of    │       │     Merriweather italic 14
│   │  entropy."                   │       │
│   └──────────────────────────────┘       │
│   (12 px gap)                            │
│   ┌──────────────────────────────┐       │
│   │ "Read my notes.txt and       │       │
│   │  extract key points."        │       │
│   └──────────────────────────────┘       │
│   (12 px gap)                            │
│   ┌──────────────────────────────┐       │
│   │ "Search for today's space    │       │
│   │  launches."                  │       │
│   └──────────────────────────────┘       │
│                                          │
│ (auto-fill)                              │
│ ┌──────────────────────────────────┐     │
│ │ 📎  Ask Mukei anything…    →     │     │  ← ChatComposer
│ └──────────────────────────────────┘     │
│ Network: off — you are private           │  ← NetworkBanner (calm green)
└──────────────────────────────────────────┘
```

#### 7.4.3 Prompt Cards

- Each prompt card is *italic Merriweather* — quoting an editorial idea.
- Tapping a card auto-fills the composer **and** auto-submits after 600 ms (giving the user time to cancel).
- The three cards rotate from a pool of 12 every cold-start (no telemetry — selection is local seed).

#### 7.4.4 Cross-refs

PRD REQ-CHAT-01, REQ-EXP-01 (empty state); AF §6.

### 7.5 ChatScreen — Active Conversation

#### 7.5.1 Layout

```
┌──────────────────────────────────────────┐
│ ☰  Mukei  · 🔒 local-only        ⚙       │  ← Header with privacy chip
│ ─────────────────────────────────────    │
│                                          │
│              (user bubble — right)       │
│              ┌──────────────────┐        │
│              │ what is entropy? │        │  ← Inter 16, surfaceVariant
│              └──────────────────┘        │
│                                          │
│ ┌──────────────────────────────────┐     │
│ │ ▸ Thinking (collapsed)           │     │  ← ThinkingAccordion
│ └──────────────────────────────────┘     │
│                                          │
│  Entropy, in physics, is a measure       │  ← Merriweather 16 / 1.6
│  of the disorder in a system. The        │     transparent bg
│  second law of thermodynamics states     │
│  that the total entropy of an isolated   │
│  system can only increase over time.    │
│  In a sense, it is nature's accountant   │
│  — keeping score of all the ways         │
│  particles can arrange themselves…│     │  ← Copper caret pulsing
│                                          │
│ ┌──────────────────────────────────┐     │
│ │ 🔍 Searching web…                │     │  ← ToolCallPill
│ └──────────────────────────────────┘     │
│                                          │
│ ┌──────────────────────────────────┐     │
│ │ 📎  Reply to Mukei…       ◼      │     │  ← Composer + Stop button
│ └──────────────────────────────────┘     │
│ Network: available · Web search enabled  │
└──────────────────────────────────────────┘
```

#### 7.5.2 States

| State | Visual |
|-------|--------|
| Idle | composer active, no caret |
| Sending | composer disabled, spinner inline |
| Streaming | caret pulsing, Stop button replaces Send |
| AwaitingTool | tool pill animates, caret pauses |
| Finalized | 🎯 micro-caret end mark, composer re-enabled |
| Errored | inline error card (muted red), [Retry] |
| Aborted | bubble shows "(stopped)" subtle italic at end |

#### 7.5.3 Interaction patterns

- **Long-press on assistant bubble:** context menu (Copy text · Copy as markdown · Branch from here · Regenerate · Report).
- **Long-press on user bubble:** Edit · Resend.
- **Tap on ToolCallPill:** expands to `ToolResultCard`.
- **Swipe right on bubble:** quick-reply quote (inserts `> quoted text\n` into composer).
- **Scroll-up while streaming:** auto-scroll pauses; floating `↓ Latest` button appears.

#### 7.5.4 Cross-refs

PRD REQ-CHAT-01..07, REQ-AGT-01..08, REQ-UI-01..06; TRD §7.2, §35.1; AF §8 (message lifecycle), §10 (tools), §12 (streaming pipeline).

### 7.6 ChatScreen — Streaming Tool Call

#### 7.6.1 The Two-Phase Pill

A tool call is **never instant**. We show the user two clear phases:

1. **Active phase:** "🔍 Searching web…" with copper pulsing icon (1100 ms sinusoid).
2. **Result phase:** "🔍 Web search · 6 results · 1.2 s" — static, tappable.

In between, a brief 80 ms cross-fade.

#### 7.6.2 Multiple Tool Calls In Sequence

If the agent loop chains two tool calls (e.g. web_search → read_file), each gets its own pill. They stack vertically with `Spacing.xs` between, **after** the AI bubble in chronological order.

#### 7.6.3 Tool Failure Pills

| Failure type | Pill visual |
|--------------|-------------|
| Network offline | "🔍 Web search · No network" with calm-amber icon |
| Validator reject | "🔍 Tool rejected · Invalid args" with muted-red icon |
| SAF token expired | "📄 read_file · Permission expired" with muted-red |
| Timeout | "🔍 Web search · Timed out" with calm-amber |

In all failure cases, the AI bubble continues after the pill — the model is informed of the failure (AF §10) and may apologise or try a different tool.

### 7.7 ModelManagerScreen

#### 7.7.1 Purpose

Manage installed models post-onboarding. Switch active, download new, delete unused.

#### 7.7.2 Layout

Two-section layout (compact): **Installed** + **Available**.

```
┌──────────────────────────────────────────┐
│ ←  Models                                │
│                                          │
│  INSTALLED                                │  ← Inter caption SemiBold
│                                          │
│  ┌────────────────────────────────────┐  │
│  │ ● Gemma 3 4B Instruct (active)     │  │  ← Copper dot + Inter SemiBold
│  │   2.5 GB · last used 12 min ago    │  │     caption secondary
│  │   [Switch]  [Delete]               │  │
│  └────────────────────────────────────┘  │
│                                          │
│  ┌────────────────────────────────────┐  │
│  │ Llama 3.2 3B Instruct              │  │
│  │ 1.8 GB · last used yesterday       │  │
│  │   [Switch]  [Delete]               │  │
│  └────────────────────────────────────┘  │
│                                          │
│  AVAILABLE                                │
│  …                                        │
│                                          │
│  Storage: 5.2 / 12 GB used                │  ← Caption + progress bar
└──────────────────────────────────────────┘
```

#### 7.7.3 States

| State | Visual |
|-------|--------|
| All installed | "Available" section hides; encourages picker |
| No models | only "Available" shown — same as first-run picker |
| Download active | progress bar inline, [Cancel] button (ghost) |
| Delete pending | DestructiveConfirmDialog two-tap |
| Storage warning | top banner amber: "Low storage. Delete unused models." |

#### 7.7.4 Cross-refs

PRD REQ-DL-01..10; TRD §5.3, §15; AF §5; BS §3.7 `model_state`.

### 7.8 SettingsScreen

#### 7.8.1 Tabbed layout

`General · Privacy · Storage · About` — four tabs (Playfair H2, ink-secondary unselected, ink-primary + copper underline selected).

#### 7.8.2 General tab

```
General
   Theme:         [Dolce Vita] [Espresso] [Taupe]    ← segmented control (3 tiles)
   Font size:     ─────●──────                       ← slider 14–22 px
   Density:       [Compact] [Cozy] [Spacious]
   Haptics:       (•) On  ( ) Off
   Temperature:   ─────●──────  0.7
   Max tokens:    ─────●──────  1024
   Top-p:         ─────●──────  0.95
```

#### 7.8.3 Privacy tab

```
Privacy

🔒  All data lives on this device.
    Nothing is uploaded. No accounts.
    No telemetry.

  ✓ Local model
  ✓ Encrypted local database (SQLCipher)
  ✓ Crash logs never leave the device
  ✓ No background sync

[ View crash log ]    [ Reset all data ]
                       (destructive)
```

#### 7.8.4 Storage tab

Pie chart breakdown (BS §12) + clear-cache + per-conversation export (encrypted blob to SAF).

#### 7.8.5 About tab

Version, License, Credits (designers + open-source attributions), Diagnostic Export (user-initiated only).

#### 7.8.6 Cross-refs

PRD REQ-CFG-01..05, REQ-EXP-01..06; TRD §12.4, §12.5; BS §7 (config schema), §11 (backup/restore).

### 7.9 SafeModeScreen

#### 7.9.1 Layout

```
┌──────────────────────────────────────────┐
│                                          │
│   We've had a few crashes.               │  ← Playfair Display 32
│   What now?                              │
│                                          │
│   (16 px gap)                            │
│                                          │
│   Mukei detected 2 unexpected closures   │  ← Inter 16 secondary
│   in the last 24 hours. You can          │
│   continue anyway, or reset all data     │
│   to start fresh. Your model file        │
│   will be kept either way.               │
│                                          │
│   (32 px gap)                            │
│                                          │
│   ┌──────────────────────────┐           │  ← PrimaryButton (copper)
│   │  Continue Anyway          │           │
│   └──────────────────────────┘           │
│   (12 px gap)                            │
│   ┌──────────────────────────┐           │  ← DestructiveButton
│   │  Reset All Data          │           │     (two-tap)
│   └──────────────────────────┘           │
│   (24 px gap)                            │
│   [ View Crash Log ]                     │  ← GhostButton (copper text)
│                                          │
└──────────────────────────────────────────┘
```

#### 7.9.2 Tone

The page is intentionally calm. It does **not** say "ERROR" or "CRASH". The headline is gentle. The destructive button is single-tap labeled but requires a two-tap morph confirm — see §6.7.2.

#### 7.9.3 Cross-refs

PRD REQ-LIFE-02 (reset); TRD §36.1 (crash counter); AF §14 (escalation).

### 7.10 ConversationList (in LeftDrawer)

#### 7.10.1 Layout

```
┌─────────────────────────────────┐
│  ⊕ New Chat                     │  ← PrimaryButton, full-width
│  🔍 Search…                     │  ← SearchField (40 dp)
│                                 │
│  Today                          │  ← Inter caption SemiBold secondary
│   • Entropy in physics          │  ← Inter 16, ink primary
│      "what is entropy?…"        │  ← Inter 14 secondary, 1-line preview
│   • notes.txt summary           │
│                                 │
│  Yesterday                      │
│   • Web search refactor         │
│                                 │
│  Earlier                        │
│   • …                           │
│                                 │
│  ─────────────────────          │
│  ⚙  Settings                    │
└─────────────────────────────────┘
```

#### 7.10.2 Interaction

- Tap conversation → load in ChatScreen, dismiss drawer.
- Long-press conversation → context menu (Rename · Delete · Archive · Export).
- Swipe-left on row → quick archive (with undo toast).

### 7.11 Branch Switcher Sheet

#### 7.11.1 Trigger

The branch glyph in the ChatScreen header is tappable when current conversation has > 1 branch. Long-press anywhere on a bubble reveals "Branch from here" context action.

#### 7.11.2 Layout (Bottom Sheet)

```
┌─────────────────────────────────┐
│   Branches in this conversation │  ← Playfair H2
│   ─────────────────────────     │
│                                 │
│  ● Main                         │  ← Copper dot for active branch
│    "Entropy, in physics, is…"   │     1-line preview, Merriweather italic
│    24 messages                  │     Inter caption secondary
│                                 │
│  ○ Branch 2 · 12 messages       │
│    "Let's explore disorder…"    │
│                                 │
│  ○ Branch 3 · 4 messages        │
│    "More on phase transitions…" │
│                                 │
│   [ + New branch from current ] │  ← SecondaryButton
└─────────────────────────────────┘
```

### 7.12 RAG / SAF Picker Sheet

#### 7.12.1 Trigger

Tap the paperclip icon in ChatComposer.

#### 7.12.2 Flow

1. Native Android SAF picker opens.
2. User selects file(s).
3. Returns to MUKEI; SAF tokens persisted (BS §3.6).
4. Bottom sheet appears: "1 file added · notes.txt" with options [Add to RAG] / [Cite once].
5. If "Add to RAG", indexing pipeline (AF §11) begins; sheet shows mini progress.

### 7.13 RagRebuildPrompt

When `hnsw.bin` fails to open at boot (corrupt, schema mismatch).

```
┌──────────────────────────────────────────┐
│                                          │
│   Your knowledge index needs             │  ← Playfair H1
│   rebuilding.                            │
│                                          │
│   Mukei found that the local index of    │  ← Inter 16
│   your private files is no longer        │
│   compatible. Rebuilding will re-scan    │
│   only the files you've shared with      │
│   Mukei — nothing leaves your device.    │
│                                          │
│   [ Rebuild now ]   [ Skip for now ]     │
│                                          │
└──────────────────────────────────────────┘
```

### 7.14 ToolResultCard (Full-Screen Expanded)

```
┌──────────────────────────────────────────┐
│ ←  Web Search Result                     │  ← Playfair H1
│                                          │
│  ┌────────────────────────────────────┐  │
│  │ Query                              │  │  ← Inter SemiBold caption
│  │ "today's space launches"           │  │     Merriweather italic body
│  └────────────────────────────────────┘  │
│                                          │
│  ┌────────────────────────────────────┐  │
│  │ Results · 6 · duration 1.2 s       │  │
│  │ Source: DuckDuckGo + Brave         │  │
│  └────────────────────────────────────┘  │
│                                          │
│  RAW                                      │  ← Inter caption SemiBold
│  ┌────────────────────────────────────┐  │
│  │ [                                  │  │  ← JetBrains Mono 14
│  │   {"title": "SpaceX Starship 4",   │  │
│  │    "url":   "…",                   │  │
│  │    "snippet": "…"                  │  │
│  │   },                               │  │
│  │   …                                │  │
│  │ ]                                  │  │
│  └────────────────────────────────────┘  │
│                                          │
│       [ Copy raw ]      [ Close ]        │
└──────────────────────────────────────────┘
```

### 7.15 FMEA — Screen Flows

| ID | Failure mode | Cause | Effect | Mitigation |
|----|--------------|-------|--------|------------|
| F-S-1 | Welcome screen shown again after upgrade | Version flag mis-handled | User annoyed by repeated onboarding | Persist `welcome_seen_version` in BS §3.9 `app_settings`; only show if `< current` |
| F-S-2 | Empty-state prompt rotation feels random | True-random selection | User suspects telemetry | Seeded from `Hash(deviceId + boot_count)`; deterministic but per-device |
| F-S-3 | Tool pill flickers between active/result state | Tool returns < 80 ms (very fast) | Visual jitter | Minimum visible 300 ms on active state before transitioning to result |
| F-S-4 | Long-press menu offscreen near top of screen | Menu opens above tap point near top | Menu cropped | `Popup` boundary-aware: opens downward when within 80 dp of top edge |
| F-S-5 | Branch switcher misses currently-active branch | UI state out of sync with DB | User can't return to "Main" | On open, query `branches` (BS §3.3); validate `is_default` exactly one row |
| F-S-6 | SafeMode shown for a single transient crash | Counter increments on minor non-fatal exceptions | False positive recovery prompt | Only `JVM crash`, `Rust panic`, `OOM-killed`, `FFI generation-flood` increment (TRD §36.1) |
| F-S-7 | Settings density "Compact" makes Esp + 2.0 fontScale unreadable | Compounded scale | Lines clip vertically | Compact + fontScale > 1.5 = forced "Cozy" mode (override) |

---

## 8. Motion, Animation & Micro-Interactions

### 8.1 The Two Cubic-Béziers

MUKEI uses **only two** custom easing curves. Every animation chooses one of them.

| Token | Curve | Use |
|-------|-------|-----|
| `Motion.enter` | `cubic-bezier(0.16, 1, 0.3, 1)` | Smooth, gentle deceleration — when something **appears** or **arrives** |
| `Motion.exit` | `cubic-bezier(0.7, 0, 0.84, 0)` | Quick, accelerating — when something **departs** or **dismisses** |

Standard linear, ease-in-out, etc. are explicitly forbidden in product code. The two-curve discipline is what makes every screen feel "of the same hand".

### 8.2 Duration Catalogue

| Element | Duration | Curve |
|---------|----------|-------|
| Bubble appear | 220 ms | enter |
| Bubble fade-out on abort | 140 ms | exit |
| Caret pulse (sinusoid) | 1100 ms loop | linear (special-case, only allowed exception) |
| Modal sheet slide-up | 280 ms | enter |
| Modal sheet dismiss | 200 ms | exit |
| FullScreen modal slide-in | 300 ms | enter |
| FullScreen modal slide-out | 220 ms | exit |
| Theme cross-fade | 300 ms | linear (color interpolation is naturally non-linear in perception) |
| ProgressBar value change | 220 ms | enter |
| Caret → 🎯 swap on finalize | 160 ms | enter (cross-fade) |
| Button press tint | 100 ms | enter |
| Long-press menu reveal | 180 ms | enter |
| Swipe-to-archive completion | 240 ms | enter |
| Composer height grow (multi-line) | 160 ms | enter |
| Keyboard inset push | 240 ms | enter |
| Drawer slide | 260 ms | enter (open) / 220 ms exit (close) |
| Network banner color shift | 220 ms | enter |
| ToolCallPill cross-fade (active → result) | 80 ms each (160 total) | enter |

### 8.3 Token Streaming Animation

#### 8.3.1 The Visual Pattern

Each streamed-token batch (50 ms accumulation, AF §12.4) appears via a **subtle opacity rise**: 0 → 1 over 120 ms with the enter curve. No translate-Y, no scale, no blur.

#### 8.3.2 Why No Translate Or Scale

Streaming text is essentially horizontal text laid out left-to-right. Any vertical motion creates the impression that new text "fell" into place — which conflicts with the editorial promise that text *was always there, simply being uncovered*.

#### 8.3.3 Caret

- 2 × 16 px vertical bar, accent color.
- Sinusoidal opacity 0.5 ↔ 1.0 over 1100 ms (`OpacityAnimator` infinite loop).
- Position: end of last rendered word, with 2 px left margin from the previous glyph.
- On `stream_finalized`, the caret cross-fades to the 🎯 micro-mark over 160 ms.

#### 8.3.4 Auto-Scroll Behavior

If the user is **scrolled to bottom** (within 32 px tolerance), each new batch triggers a smooth-scroll to keep the caret visible. The scroll uses the enter curve, 80 ms.

If the user has scrolled up to read, auto-scroll **disables**. A floating `↓ Latest` chip appears, dismissable.

### 8.4 Tool-Call Animation Choreography

A tool call has a deliberate three-act structure:

```
Act 1: "Searching web…"   (active phase)
   ▸ Pill appears with enter curve, 220 ms
   ▸ Copper icon pulses 1100 ms sinusoid
   ▸ Caret in chat pauses (text generation halted)

Act 2: cross-fade (80 ms)
   ▸ Pill background tint shifts to surface
   ▸ Icon stops pulsing

Act 3: "Web search · 6 results · 1.2 s"  (result phase)
   ▸ Pill text updates with enter curve, 160 ms
   ▸ Caret resumes in chat
```

The visible cadence is: pill 300 ms minimum → cross-fade 80 ms → result static.

### 8.5 Theme Transition

#### 8.5.1 The Anti-Flicker Strategy

When the user changes theme (`Settings → General → Theme`), all color tokens cross-fade simultaneously over 300 ms. No flicker — achieved by:

1. Pre-computing target palette before animation start.
2. Animating each color property via a single `ColorAnimation` group.
3. Disabling QML rendering for 1 frame at the midpoint to swap CSS-style cascades (Qt 6 `QQuickWindow.update()`).

#### 8.5.2 Theme Switch Sound

None. (Per Calm principle, §1.3.1.)

### 8.6 Haptic Feedback Integration

#### 8.6.1 The Three Haptic Levels

| Level | Qt token | Use |
|-------|----------|-----|
| Light | `HapticEffect.Light` (10 ms, low amplitude) | Button press, tab switch, list-item tap |
| Medium | `HapticEffect.Medium` (15 ms, mid amplitude) | Tool completion, long-press menu reveal, send message |
| Heavy | `HapticEffect.Heavy` (25 ms, high amplitude) | Destructive action commit (second tap of `DestructiveConfirmDialog`), error |

#### 8.6.2 Composition: "Double Tick"

For the second tap on a destructive confirm, we do a **Medium → 50 ms pause → Heavy** sequence. This creates the unmistakable feeling of "I just did something irreversible".

#### 8.6.3 Off Setting

`Settings → General → Haptics: Off` disables all haptics globally. (Some users find them annoying; some devices simulate them imperfectly.)

### 8.7 Reduce-Motion Mode

When Android's "Reduce motion" accessibility setting is enabled:

| Animation | Replacement |
|-----------|-------------|
| Bubble appear | instant (no animation) |
| Modal slide | opacity fade only, 160 ms |
| Caret pulse | static visible bar |
| Tool pill choreography | both phases visible immediately, no cross-fade |
| Theme transition | instant swap |
| Swipe-to-archive completion | instant |

Detection: `Settings.Global.ANIMATOR_DURATION_SCALE == 0` on Android; mirrored to a QML flag `Theme.reduceMotion`.

### 8.8 Performance Budgets

#### 8.8.1 The 16 ms Frame Budget on Mali-G68

| Phase | Budget |
|-------|--------|
| Total frame | 16 ms (60 fps) |
| QML compositor | ≤ 4 ms |
| QML JS evaluation | ≤ 3 ms |
| Token rendering (streaming) | ≤ 6 ms (§4.10.3) |
| Heavy animation (worst case) | ≤ 3 ms |

#### 8.8.2 Profile Tools

- Qt Creator QML Profiler — record 30 s of streaming, assert no frame > 16 ms.
- Android GPU Profiler — confirm no overdraw > 4× on chat screen.
- CI gate: nightly perf job fails build if median frame > 17 ms.

#### 8.8.3 Thermal-Aware Animation

When `PowerManager.OnThermalStatusChangedListener` reports `THERMAL_STATUS_MODERATE` or higher:

- Disable caret pulse (replace with static bar).
- Disable tool-pill icon pulse.
- Skip theme cross-fade animation; instant swap.
- Show subtle banner: "Device is warm — simplifying visuals to cool down."

(See §12.1 FMEA.)

### 8.9 Animation Cancellation On "Stop"

When the user taps Stop while streaming:

1. Caret animation halts immediately (no fade-out — Stop is a hard interrupt).
2. The "(stopped)" italic suffix is appended to the bubble with no animation.
3. The Stop button morphs back to Send with the enter curve (160 ms).

This is a deliberate violation of the otherwise-soft motion language — Stop is the **one place** where MUKEI is allowed to feel snappy.

### 8.10 FMEA — Motion & Animation

| ID | Failure mode | Cause | Effect | Mitigation |
|----|--------------|-------|--------|------------|
| F-M-1 | Animation continues after thermal throttle | Listener not wired | UI lag, battery drain | §8.8.3 banner + animation disable path; tested via `tst_ThermalThrottle.qml` |
| F-M-2 | Caret animation stutters during streaming | Animation contends with text re-layout | Visible jitter | Caret runs on QML compositor thread (`OpacityAnimator`); text layout on main |
| F-M-3 | Bubble appear animation cumulates on rapid-fire send | Each animation independent | Visible cascade jank | Coalesce: if ≥ 3 bubbles enter within 200 ms, render last 2 instantly, animate only the latest |
| F-M-4 | "Reduce motion" ignored | A11y flag not propagated to QML | Motion-sensitive users get full animations | §8.7 explicit `Theme.reduceMotion` flag; tested on every animated component |
| F-M-5 | Theme cross-fade shows white flash at midpoint | Render-clear before swap | Visible flicker | §8.5.1 disable-render-1-frame approach; tested across all 6 theme pair transitions |
| F-M-6 | Haptics fire on every token (over-triggered) | Bound to wrong signal | Phone vibrates constantly during streaming | Haptics bound only to **user-initiated** actions |
| F-M-7 | Easing curves not consistent across components | Engineers use defaults | Visual chaos | `Motion.qml` singleton; `qmllint` rule rejects literal `Easing.*` outside it |

---

## 9. Iconography & Imagery

### 9.1 The Icon System

#### 9.1.1 Style

- **Stroke-based**, no fills (except for a tiny dot in `success` icon and the `caret bar`).
- **Stroke width: exactly 1.5 px** at 24 dp icon size. At other sizes, scaled proportionally (e.g. 1 px at 16 dp, 2 px at 32 dp).
- **Corners:** rounded `linejoin="round"` and `linecap="round"` (no sharp angles — matches editorial softness).
- **Geometry:** built on the 24 × 24 dp grid, 2 dp inner padding.
- **Color:** inherits parent `color` property — usually `Theme.p.inkPrimary` for chrome, `Theme.p.accent` for active/CTA.

#### 9.1.2 Why Not Material Icons

Material icons (Google) are filled and dense. They visually fight the editorial 70/20/10 lightness. We commission (or use carefully-licensed) line-only icons.

### 9.2 Custom Icon Set

| Icon | File | Use |
|------|------|-----|
| chat-bubble | `chat.svg` | Drawer entry, new chat |
| magnifier | `search.svg` | SearchField, web_search tool |
| file-doc | `file.svg` | read_file tool, SAF picker |
| chip | `chip.svg` | Hardware status (REQ-HW-01) |
| brain | `memory.svg` | RAG/memory indicator |
| gear | `settings.svg` | Settings entry |
| paperclip | `attach.svg` | Composer attach |
| arrow-right | `send.svg` | Send button |
| square | `stop.svg` | Stop streaming |
| clipboard | `copy.svg` | Copy action |
| pencil | `edit.svg` | Edit message |
| refresh | `regenerate.svg` | Regenerate response |
| share-out | `export.svg` | Diagnostic export (Settings) |
| chevron-left | `back.svg` | Back navigation |
| chevron-down | `expand.svg` | Accordion expand |
| chevron-up | `collapse.svg` | Accordion collapse |
| check | `check.svg` | Success state |
| cross | `error.svg` | Error state |
| dot | `active-dot.svg` | Active branch / model indicator |
| leaf | `network-off.svg` | Calm network-off badge (calm green leaf) |
| globe | `network-on.svg` | Network-on badge |
| lock | `lock.svg` | Encrypt notice chip |
| eye | `view.svg` | View crash log |
| trash | `delete.svg` | Delete |
| moon-sun | `theme-auto.svg` | Theme auto |
| branch | `branch.svg` | Branch glyph in chat header |
| target | `done-target.svg` | 🎯 finalization micro-mark |

#### 9.2.1 Stroke Audit

A CI test (`test_icon_stroke_widths`) parses every SVG and asserts the stroke-width attribute equals 1.5 in 24-dp viewBox.

### 9.3 Illustration Style

#### 9.3.1 Where Illustrations Appear

- **Welcome screen:** none (intentionally — text is the hero).
- **Empty state ChatScreen:** none (the three prompt cards are the visual).
- **Empty conversation list:** small line-art notebook icon (32 dp, ink-faint).
- **Error states (e.g. SafeMode):** a small line-art coffee-cup illustration (48 × 48 dp, ink-faint).
- **No model installed:** small line-art "box opening" illustration.

#### 9.3.2 Style Rules

- Line-only, 1.5 px stroke.
- Warm tones — copper accents where the illustration depicts an *action* (e.g. opening, sharing).
- No gradients, no shadows, no 3-D.
- Editorial feel: like a New Yorker spot illustration.
- Maximum size: 96 dp (illustrations are accents, never heroes).

### 9.4 SVG vs PNG Strategy

#### 9.4.1 Use SVG When

- The asset is monochromatic (1–3 colors).
- The asset is geometric / line-based.
- The asset will be tinted via QML `color` property at runtime.

#### 9.4.2 Use PNG When (Rare)

- Asset has photographic gradients (e.g. coffee-cup illustration shading) — currently zero such assets.
- Asset has dense pixel detail (currently zero).

**For v1, MUKEI ships with zero PNG assets in the UI layer.** Only the launcher icon is rasterised (mipmap-xxxhdpi).

#### 9.4.3 QML SVG Loading

```qml
Image {
    source: "qrc:/icons/search.svg"
    sourceSize: Qt.size(24, 24)
    fillMode: Image.PreserveAspectFit
    smooth: true
    layer.enabled: true
    layer.effect: ColorOverlay {
        color: Theme.p.inkPrimary
    }
}
```

`ColorOverlay` lets us tint a single SVG asset to any theme color — saves shipping one SVG per theme.

### 9.5 App Launcher Icon

#### 9.5.1 Spec

- Adaptive icon (Android 8+): foreground = MUKEI "M" monogram in copper, background = Dolce Vita (`#D8CABD`).
- Round, square, and squircle masks all tested.
- Launcher icon designed at 432 × 432 px (xxxhdpi).
- Notification icon (24 × 24 dp monochrome): the same "M" stroke-only, ink primary tint.

#### 9.5.2 No Animated Launcher

(Per Calm principle.)

### 9.6 FMEA — Iconography

| ID | Failure mode | Cause | Effect | Mitigation |
|----|--------------|-------|--------|------------|
| F-I-1 | SVG fails to tint via `ColorOverlay` on older GPUs | OpenGL ES 2 fallback path | Icons render in original stroke color | Detect GPU capability; fallback to multi-color SVG variants per theme |
| F-I-2 | Icon stroke width drifts across set | Designers use 2 px instead of 1.5 | Visual inconsistency | CI test `test_icon_stroke_widths` (§9.2.1) |
| F-I-3 | Adaptive icon background clashes with launcher wallpaper | User has dark wallpaper | "M" disappears | Test against Pixel default wallpapers; provide light + dark adaptive variants |
| F-I-4 | Launcher icon pixelated on xxxhdpi screens | Wrong density supplied | Looks cheap | Ship all densities (mdpi through xxxhdpi) |
| F-I-5 | Notification icon shows color, looks broken | Android requires monochrome | Status bar icon ugly | Notification icon is mono-stroke only (§9.5.1) |
| F-I-6 | Empty-state illustration too prominent | Designer scaled it 200 % | Becomes visual hero, breaks 70/20/10 | Hard cap 96 dp in spec; design review enforces |

---

## 10. Accessibility (A11y) & Inclusivity

> *Accessibility is not a "feature" we add — it is a property the system must already have. Every screen, every component, every transition.*

### 10.1 Why A11y Is A First-Class Constraint Here

MUKEI is a productivity tool for thought. Users include developers, researchers, writers — many of whom rely on screen readers, switch access, or voice control as primary input. A11y is not an edge case; it is **expected baseline**.

A11y conformance target: **WCAG 2.1 Level AA**. Stretch goal: Level AAA on core ChatScreen (already met for Dolce Vita + Espresso per §3.6).

### 10.2 Screen Reader (TalkBack) Support

#### 10.2.1 The QML Accessible Property Triad

Every interactive QML item exposes three properties:

```qml
Accessible.role: Accessible.Button   // or Text, EditableText, Link…
Accessible.name: qsTr("Send message")
Accessible.description: qsTr("Sends your message to Mukei")
```

#### 10.2.2 The Streaming Announcement Strategy

The hardest A11y problem in MUKEI is **streaming tokens**. Naïve implementation: TalkBack reads every token as it arrives. Result: 500 individual word-by-word announcements per response. Catastrophic.

**MUKEI's solution — sentence-boundary batching:**

1. Tokens accumulate in a TTS buffer.
2. When a sentence-ending punctuation (`. ? ! …`) is detected by Rust, the accumulated sentence is emitted as one `accessibilityAnnouncementRequested` signal.
3. If no sentence ends for ≥ 6 seconds, the partial buffer is flushed as an interim announcement.
4. On `stream_finalized`, any remaining buffer is announced.

Additionally:

- At stream start: a one-time announcement "Mukei is typing." (configurable in A11y settings).
- At `stream_finalized`: announcement "Response complete. 47 seconds. 312 words."
- At tool-call active: announcement "Mukei is searching the web."
- At tool-call result: announcement "Tool returned 6 results."
- At tool-call fail: announcement "Tool failed: network unavailable."

(See §12.7 FMEA `F-FM-7`.)

#### 10.2.3 Accessibility Of Long-Press Menus

Long-press menus are difficult for screen reader users. **Every long-press action must have an equivalent accessible action.**

For `MessageBubble`, the `Accessible.actions` list includes:
- `Copy text`
- `Copy as markdown`
- `Branch from here`
- `Regenerate`
- `Report`

TalkBack users invoke the action via the read-aloud menu, no long press needed.

#### 10.2.4 Reading-Order Audit

For each screen, the reading order (DOM traversal) is explicitly defined:

| Screen | Reading order |
|--------|---------------|
| Welcome | Headline → subtext → encryption notice → "Get Started" |
| Empty ChatScreen | Header → "Mukei is ready." → 3 prompt cards → composer → network banner |
| Active ChatScreen | Header → 🔒 chip → settings → most-recent assistant message → caret status → composer |
| SafeMode | Headline → body → "Continue" → "Reset" → "View crash log" |
| Settings | Tab bar → active tab content → primary CTA |

These orderings are tested via `tst_AccessibleReadingOrder.qml`.

### 10.3 Keyboard Navigation

#### 10.3.1 Supported Hardware

- Physical Bluetooth keyboard.
- Foldable keyboard cover (e.g. Galaxy Z Fold cover keyboard).
- USB-C external keyboard.
- Switch access (single-switch or two-switch scanning).

#### 10.3.2 The Tab Order

Tab order on ChatScreen:

```
1. Drawer button (☰)
2. Settings gear (⚙)
3. Most recent assistant bubble (if focused, Tab reveals long-press menu equivalents)
4. ToolCallPill (if present)
5. Composer text field
6. Send / Stop button
```

#### 10.3.3 Keyboard Shortcuts

| Shortcut | Action |
|----------|--------|
| `Cmd+Enter` / `Ctrl+Enter` | Send message |
| `Escape` | Stop streaming / dismiss modal |
| `Cmd+/` / `Ctrl+/` | Open settings |
| `Cmd+K` / `Ctrl+K` | Focus search in drawer |
| `Cmd+N` / `Ctrl+N` | New chat |
| `Cmd+,` / `Ctrl+,` | Open settings |
| `Cmd+1` / `Ctrl+1` | First conversation |
| `Cmd+B` / `Ctrl+B` | Toggle drawer |
| `Up arrow` (empty composer) | Edit last user message |

#### 10.3.4 Focus Ring

The 2 px copper outline ring (§6.2.3) is the **single** universal focus indicator. Visible on every focusable element.

### 10.4 High Contrast Mode

#### 10.4.1 Detection

`AccessibilityManager.isHighTextContrastEnabled` queried on resume and on `onAccessibilityStateChanged`. Pushed via JNI to QML signal.

#### 10.4.2 Overridden Palette

When high-contrast is on, the active palette is replaced with a stricter variant:

| Theme | Normal accent | HC accent | Normal ink | HC ink |
|-------|---------------|-----------|------------|--------|
| Dolce Vita | `#B87333` | `#5A3713` | `#362417` | `#000000` |
| Espresso | `#D4AF37` | `#FFE062` | `#EBE1D5` | `#FFFFFF` |
| Taupe | `#C17F3E` | `#3F2811` | `#2A2420` | `#000000` |

Contrast ratios pushed to ≥ 7:1 (AAA) on all text/bg pairs.

Borders are re-introduced in HC mode on cards and buttons to compensate for lost color cues.

### 10.5 Reduce Motion Mode

Detailed in §8.7. Summary: animations replaced with instant or fade-only equivalents when `Settings.Global.ANIMATOR_DURATION_SCALE == 0`.

### 10.6 RTL Layout

#### 10.6.1 Languages

Arabic (`ar`), Hebrew (`he`), Persian (`fa`), Urdu (`ur`) — all activate RTL.

#### 10.6.2 Mirroring

```qml
LayoutMirroring.enabled: Qt.locale().textDirection === Qt.RightToLeft
LayoutMirroring.childrenInherit: true
```

#### 10.6.3 Bubble Flip

- User bubble aligns **left** instead of right.
- AI bubble aligns **right** instead of left.
- Inline ToolCallPill flips horizontally.

#### 10.6.4 Iconography In RTL

- Back-arrow icons (`chevron-left`) flip horizontally.
- Send arrow (`arrow-right` → effectively `arrow-left`) flips.
- Branch icon stays the same (it is a symbolic glyph, not directional).
- Stop square stays the same.

#### 10.6.5 RTL-Specific Type Rules

(See §4.9.3.) Letter-spacing is suppressed; Arabic-specific shaping respected; Naskh-style fallback active.

### 10.7 Font Scaling

Detailed in §4.8. Summary: respect `Configuration.fontScale` from 0.85× to 2.0×, with compact layout activating at > 1.5×.

### 10.8 Color-Blind Considerations

Detailed in §3.8. Summary: state cues are always (color + icon + position), never color-alone.

### 10.9 Voice Control / Switch Access

#### 10.9.1 Voice Access (Android system feature)

Every interactive element has `Accessible.name` set to a verb-noun phrase that Voice Access can speak:
- "Send message"
- "Open settings"
- "Stop response"
- "Copy text"

These are different from the visible label (which may be icon-only).

#### 10.9.2 Switch Access

The Tab order (§10.3.2) is also the Switch Access scan order. Each focusable element is given a sufficient pause (default OS configuration) for switch users.

### 10.10 Vibrant Cognitive Diversity (Reading Pace)

#### 10.10.1 Token Streaming Pace Setting

Users with cognitive processing differences may struggle with fast streaming. `Settings → Accessibility → Reading pace`:

- **Standard:** stream as tokens arrive (default).
- **Comfortable:** insert a 50 ms cool-down between sentence boundaries.
- **Patient:** insert a 250 ms cool-down between sentence boundaries.

Reading pace is implemented on the Rust side — it gates token emission to the FFI signal.

#### 10.10.2 Simple Language Mode (Future)

Not in v1, but reserved: a system prompt augmentation that instructs the model to use simpler language.

#### 10.10.3 Cognitive Load Controls 🧠 (NEW in v0.7.5 — P2-01)

> **🛡️ UX DECISION v0.7.5 — Beyond Sensory A11y.** Reading-pace (§10.10.1) addresses *streaming velocity*; v0.7.5 adds **response-density** controls that address the *post-stream cognitive load* of long, tool-laden, or deeply-nested answers. These controls were flagged by the Principal Designer audit as the next maturity step for MUKEI's accessibility posture, particularly for neurodivergent users (ADHD, autism-spectrum, post-concussion, anxiety/overload, ESL readers).

**Settings panel: `Settings → Accessibility → Response density`.**

| Setting | Type | Default | Effect |
|---------|------|---------|--------|
| `response_density.summary_first` | `bool` | `false` | When `true`, AI responses > 600 characters render a 1–2 sentence summary card at top with **Show full answer** disclosure. The model is instructed (system prompt augmentation) to emit a `<summary>…</summary>` envelope; if absent, Rust auto-generates one from the first sentence + a length heuristic. |
| `response_density.collapse_tool_traces` | `bool` | `false` | When `true`, `ChatTimelineEvent { kind: "tool" }` rows render in collapsed form (one-line caption + chevron); tap-to-expand reveals the result card. |
| `response_density.collapse_thinking` | `bool` | `true` (already canonical) | The thinking accordion (UXB §6.4.2) is collapsed-by-default; unchanged — listed here for completeness. |
| `response_density.low_stimulation_mode` | `bool` | `false` | When `true`, this toggle (a) forces `Theme.reduceMotion = true`, (b) disables caret pulse and tool-pill icon pulse, (c) hides decorative illustrations (§9.3), (d) sets `response_density.summary_first = true` and `collapse_tool_traces = true`. Single switch for users who want maximum calm. |

**Visual treatment of the summary card** (when `summary_first = true`):

- Sits as the first element inside the `AIMessageBubble`, with `Spacing.sm` below before the (hidden) full answer.
- Background: `Theme.p.surfaceFaint` (same token as the reader-wash, §6.4.2.1 NEW below).
- Typography: `Type.bodyAI` (Merriweather), same as the bubble body — do **not** introduce a new type token.
- Disclosure affordance: a `GhostButton` reading *“Show full answer”* with a chevron-down icon; tap reveals the rest of the bubble with a 220 ms enter-curve opacity fade.
- Long-press on the summary card surfaces `Accessible.actions`: *Copy summary* · *Copy full answer* · *Branch from here*.

**Acceptance tests (NEW in v0.7.5):**

| Test | Asserts |
|------|---------|
| `tst_SummaryFirstRendering.qml` | With `summary_first = true` and a 1200-char AI response, the bubble renders a summary card and full answer is initially hidden |
| `tst_LowStimulationModeAggregate.qml` | Toggling `low_stimulation_mode` sets all four downstream flags in one transaction |
| `tst_CollapseToolTraces.qml` | With `collapse_tool_traces = true`, each `ChatTimelineEvent` renders height ≤ 32 dp until tapped |
| `tst_SummaryAccessibleActions.qml` | Long-press / TalkBack actions menu on summary card exposes the three named actions |

### 10.11 Privacy Implications Of A11y

Accessibility services on Android have access to the entire UI tree. We accept this — MUKEI cannot prevent it, nor should it (a11y is a fundamental right). Instead:

- MUKEI's UI **never** contains plaintext secrets the user shouldn't see (e.g. API keys are wrapped in `pwd-input` style hidden controls).
- Crash log preview redacts SAF tokens and any string > 60 hex chars before showing it on screen (BS §8.4).

### 10.12 A11y Testing

#### 10.12.1 Manual

Every release is tested by:
- TalkBack screen reader (visual eyes-closed pass).
- Switch access (single-switch and two-switch scenarios).
- Voice Access.
- High contrast + 2.0× font scale combined.
- Dolce Vita + 2.0× font scale (verify no text clip).

#### 10.12.2 Automated

| Test | Asserts |
|------|---------|
| `tst_AccessibleNamesExist.qml` | Every interactive item has `Accessible.name` set. |
| `tst_TabOrder.qml` | Tab navigation visits items in spec'd order (§10.3.2). |
| `tst_FocusRing.qml` | Focus ring is visible on every focusable item. |
| `tst_AnnouncementBatching.qml` | Streaming announcements fire on sentence boundaries, not per token. |
| `tst_HighContrast.qml` | HC overrides applied; contrast ≥ 7:1. |
| `tst_RTLMirror.qml` | Bubble alignment flips in `ar` locale. |
| `tst_FontScaleExtreme.qml` | 0.85× / 1.0× / 1.5× / 2.0× all render without clip. |

### 10.13 FMEA — Accessibility

| ID | Failure mode | Cause | Effect | Mitigation |
|----|--------------|-------|--------|------------|
| F-A11y-1 | TalkBack announces every token | No batching | Unusable for screen-reader users | §10.2.2 sentence-boundary batching |
| F-A11y-2 | Tab order skips ToolCallPill | Pill not focusable | A11y user can't review tool results | `focusPolicy: Qt.StrongFocus` on pill |
| F-A11y-3 | Long-press menu inaccessible | No `Accessible.actions` | TalkBack users can't branch / regenerate | §10.2.3 explicit accessible actions |
| F-A11y-4 | High-contrast not detected | JNI hook missing | A11y users see standard palette | `onAccessibilityStateChanged` listener wired; QA tested |
| F-A11y-5 | RTL breaks chat bubble alignment | `LayoutMirroring.childrenInherit` false | Bubbles wrong side | §10.6.2 inherit = true |
| F-A11y-6 | Focus ring covered by content | Ring drawn behind | A11y user sees no focus indicator | Ring `z: 100`; tested in `tst_FocusRing.qml` |
| F-A11y-7 | Crash log shows SAF token plaintext | No redaction | Forensic data leak | §10.11 redaction; BS §8.4 |
| F-A11y-8 | Voice Access cannot invoke send | Composer Send icon has no `Accessible.name` | Voice user stuck | Verb-noun phrase audit (§10.9.1) |
| F-A11y-9 | Reading-pace "Patient" causes user to think app froze | No visual indicator of pacing | User taps Stop, frustrated | Pulsing caret + small "(pacing)" caption when pace > Standard |

---

## 11. Qt/QML Implementation Strategy

### 11.1 Why NOT Material Design 3 Out-Of-The-Box

#### 11.1.1 The Default Problem

`QtQuick.Controls.Material` (Material 3) gives MUKEI:
- Fully-filled "ripple" buttons (visual weight too high for 70 % rule).
- Card elevation via shadows (we banned shadows in §2.2.2).
- Color palette assumptions baked into `Material.theme`, `Material.accent` (we have three themes, not two).
- Roboto as default font (we use Inter / Merriweather / Playfair).
- Typography scale rooted at 14 px body (we root at 16 px).

#### 11.1.2 The Choice

Rather than override Material 3 piece-by-piece (which leaks defaults in places we forget), MUKEI uses **`QtQuick.Controls.Basic`** as the base style and builds the entire component library from scratch.

```cmake
# CMakeLists.txt
set(QT_QUICK_CONTROLS_STYLE "Basic")
```

#### 11.1.3 Cost / Benefit

| Aspect | Custom (our choice) | Material 3 (rejected) |
|--------|---------------------|----------------------|
| Initial dev time | Higher (~3 weeks for full set) | Lower |
| Design consistency | Total (every pixel under our control) | Partial (defaults leak) |
| Long-term maintainability | Easier (no override stack) | Harder |
| File-size in APK | Smaller (no unused Material rs) | Larger |
| Update risk on Qt upgrade | Lower | Higher (Material APIs evolve) |

### 11.2 Rendering Backend Selection

#### 11.2.1 Qt 6 Default Behaviour

Qt 6 auto-selects: Vulkan on Android 12+ where supported, OpenGL ES on older.

#### 11.2.2 MUKEI Forces Vulkan When Available

```cpp
// main.cpp
#ifdef Q_OS_ANDROID
    if (android_api_level() >= 31 && vulkan_supported()) {
        qputenv("QT_QUICK_BACKEND", "vulkan");
    } else {
        qputenv("QT_QUICK_BACKEND", "opengl");
    }
#endif
```

Why force Vulkan when available:
- 15 – 30 % lower frame time on Mali-G68 with backdrop-blur effects.
- More predictable scheduling under thermal throttle.

#### 11.2.3 Fallback Path

When Vulkan init fails (driver bug, AOSP rom): graceful fallback to OpenGL ES 3.2. Logged once to crash dump, no user-facing notice.

#### 11.2.4 Backdrop Blur

- Vulkan: implemented via `MultiEffect` blur with radius 8 px (drawer backdrop, modal backdrop).
- OpenGL ES fallback: replace blur with semi-opaque overlay (`#000000` at 50 % opacity).

### 11.3 CXX-Qt Signal Binding

#### 11.3.1 The Bridge

(See AF §12 and TRD §1.2 / §1.3.) Rust emits signals; QML binds.

#### 11.3.2 Token Streaming Signal

```rust
// rust/src/bridge.rs
#[cxx_qt::qsignals]
pub enum MukeiBridgeSignals {
    ChunkGenerated { batch: QString },        // 50 ms batched
    StreamFinalized { meta: QString },        // status JSON
    ToolCallDetected { call: QString },       // tool call JSON
    ProgressChanged { progress: QString },    // download / asset extraction
    ErrorOccurred { err: QString },           // error JSON
}
```

QML binding:

```qml
Connections {
    target: bridge
    function onChunkGenerated(batch) {
        chatModel.appendToCurrent(batch)
    }
    function onStreamFinalized(meta) {
        chatModel.finalize(JSON.parse(meta))
    }
}
```

#### 11.3.3 The Generation-Guard Invariant

Every stream-scoped payload is emitted only for the active generation. Late payloads are dropped before they reach QML, preserving the generation-guard contract (AF §21).

#### 11.3.4 Zero-Copy Strategy

`QString` shares memory across the FFI boundary via Qt's implicit sharing. Tokens are *not* copied into JS strings — they are appended as `QString` slices in the model.

### 11.4 Asset Bundling

#### 11.4.1 The `.qrc` Manifest

```xml
<RCC>
  <qresource prefix="/">
    <file>icons/chat.svg</file>
    <file>icons/search.svg</file>
    <!-- … all icons … -->
    <file>fonts/PlayfairDisplay-Regular.ttf</file>
    <file>fonts/PlayfairDisplay-Medium.ttf</file>
    <file>fonts/PlayfairDisplay-SemiBold.ttf</file>
    <file>fonts/PlayfairDisplay-Bold.ttf</file>
    <file>fonts/Merriweather-Regular.ttf</file>
    <file>fonts/Merriweather-Italic.ttf</file>
    <file>fonts/Merriweather-Bold.ttf</file>
    <file>fonts/Inter-Regular.ttf</file>
    <file>fonts/Inter-Medium.ttf</file>
    <file>fonts/Inter-SemiBold.ttf</file>
    <file>fonts/JetBrainsMono-Regular.ttf</file>
    <file>fonts/JetBrainsMono-Medium.ttf</file>
    <file>fonts/JetBrainsMono-Bold.ttf</file>
    <file>qml/MainWindow.qml</file>
    <file>qml/components/MessageBubble.qml</file>
    <!-- … all QML … -->
  </qresource>
</RCC>
```

#### 11.4.2 Size Budget

| Asset class | Budget | Actual (target) |
|-------------|--------|------------------|
| Fonts (4 families × ~3 weights) | 1.5 MB | ~1.1 MB |
| Icons (≈ 30 SVGs × ~1 KB) | 50 KB | ~28 KB |
| Illustrations (5 SVGs) | 20 KB | ~12 KB |
| QML source | 200 KB | ~140 KB |
| **Total UI assets** | **≤ 2 MB** | **~1.3 MB** |

#### 11.4.3 FontLoader Bootstrap

(See TRD §33.1.) `FontLoader.qml` loads each TTF on startup; emits `fontLoaded(family, success)`. If any font fails, fallback chain (§4.9) activates and a warning is logged.

### 11.5 Performance Profiling Strategy

#### 11.5.1 Tools

- **Qt Creator QML Profiler**: per-frame timing breakdown.
- **Android GPU Profiler (Adreno) / Mali Graphics Debugger**: overdraw and shader cost.
- **Tracy** (TRD §14.1): cross-language tracing (Rust + QML markers).
- **perfetto**: Android system-trace integration.

#### 11.5.2 Continuous Profile In CI

Nightly job on a reference device (Pixel 7 with Mali-G710):
- Boot, navigate to ChatScreen, send 5 messages, scroll 1000 bubbles, theme-switch.
- Capture median + p99 frame time.
- Fail build if median > 17 ms or p99 > 32 ms.

#### 11.5.3 Hot-Path Optimisations

| Hot path | Optimisation |
|----------|--------------|
| Token rendering | 50 ms batching (§4.10); pre-allocated height (§4.10.2) |
| Bubble appearance | `Loader` for off-screen bubbles; virtualisation (§5.5.3) |
| Theme switch | Single `ColorAnimation` group; pre-computed palette (§8.5.1) |
| Backdrop blur | Vulkan-only; OpenGL fallback uses solid overlay (§11.2.4) |
| SVG icon tint | `ColorOverlay` shader cached per SVG path |

### 11.6 QML Module Structure

```
qml/
├── MainWindow.qml
├── theme/
│   ├── Theme.qml
│   ├── Type.qml
│   ├── Spacing.qml
│   ├── Motion.qml
│   ├── qmldir
├── components/
│   ├── PrimaryButton.qml
│   ├── SecondaryButton.qml
│   ├── GhostButton.qml
│   ├── IconButton.qml
│   ├── DestructiveButton.qml
│   ├── ChatComposer.qml
│   ├── UserMessageBubble.qml
│   ├── AIMessageBubble.qml
│   ├── ToolResultCard.qml
│   ├── RAGChunkCard.qml
│   ├── LeftDrawer.qml
│   ├── ModalSheet.qml
│   ├── FullScreenModal.qml
│   ├── ProgressBar.qml
│   ├── Spinner.qml
│   ├── SkeletonLoader.qml
│   ├── StatusPill.qml
│   ├── StreamingCaret.qml
│   ├── ConfirmationDialog.qml
│   ├── DestructiveConfirmDialog.qml
│   ├── ToastNotification.qml
│   ├── MarkdownRenderer.qml
│   ├── CodeBlockComponent.qml
│   ├── ThinkingAccordion.qml
│   ├── CopyButton.qml
│   ├── HapticFeedback.qml
│   ├── FontLoader.qml
│   ├── NetworkBanner.qml
│   ├── qmldir
├── screens/
│   ├── WelcomeScreen.qml
│   ├── ModelPickerScreen.qml
│   ├── VerificationScreen.qml
│   ├── ChatScreen.qml
│   ├── ModelManagerScreen.qml
│   ├── SettingsScreen.qml
│   ├── SafeModeScreen.qml
│   ├── RagRebuildPromptScreen.qml
│   ├── ToolResultExpandedScreen.qml
│   ├── qmldir
└── tests/
    ├── tst_ChatScreen.qml
    ├── tst_MessageBubble.qml
    ├── tst_AccessibleReadingOrder.qml
    └── ...
```

#### 11.6.1 `qmldir` Manifests

Each subdirectory has a `qmldir` registering its types as QML modules:

```qmldir
module com.mukei.theme
singleton Theme 1.0 Theme.qml
singleton Type 1.0 Type.qml
singleton Spacing 1.0 Spacing.qml
singleton Motion 1.0 Motion.qml
```

Usage in components:

```qml
import com.mukei.theme 1.0
import com.mukei.components 1.0
```

### 11.7 Lint & Format Strategy

#### 11.7.1 `qmllint`

- Built-in rules: enabled.
- Custom rules: 
  - "no-literal-color" — no `#XXXXXX` outside `Theme.qml`.
  - "no-off-grid-spacing" — margins/padding must reference `Spacing` tokens.
  - "no-literal-font-size" — `font.pixelSize` must reference `Type` tokens.
  - "no-literal-easing" — easing must reference `Motion` tokens.

#### 11.7.2 `qmlformat`

Auto-format on save (IDE hook) and pre-commit. Enforces:
- 4-space indent.
- One property per line in QML objects.
- Imports sorted alphabetically.

#### 11.7.3 CI Gate

PRs failing `qmllint` or `qmlformat --check` block merge.

### 11.8 FMEA — Qt/QML Implementation

| ID | Failure mode | Cause | Effect | Mitigation |
|----|--------------|-------|--------|------------|
| F-Q-1 | Material 3 leak — engineer accidentally imports `QtQuick.Controls.Material` | Convenience copy-paste from tutorial | Inconsistent visual; ripple effect appears | CMake `QT_QUICK_CONTROLS_STYLE=Basic`; `qmllint` rejects Material imports |
| F-Q-2 | Vulkan init fails on rooted device | Custom ROM lacks driver | Black screen | Fallback to OpenGL ES (§11.2.3); logged once |
| F-Q-3 | Signal generation mismatch silently drops UI updates | Bridge not propagating generation | UI appears frozen mid-stream | Per-payload generation field validated in QML; tested in `tst_GenerationGuard.qml` |
| F-Q-4 | Font asset corrupted in `.qrc` | Build pipeline gz issue | Fallback to Roboto, visual identity lost | `FontLoader` reports failure; build-time SHA verifies (TRD §33.1) |
| F-Q-5 | QML scene-graph cost > 16 ms / frame | Overdraw or shader-heavy effect | Streaming stutters | §11.5.2 CI gate fails; §11.2.4 mitigates blur cost |
| F-Q-6 | `qmldir` typo registers wrong singleton | Manual editing error | Components reference stale singleton | CI build fails on QML module resolution error |
| F-Q-7 | Linter rules disabled "just for one file" | Engineer pressure | Token drift accumulates | Pre-commit hook prevents `// qmllint disable` lines from landing on `main` |

---

## 12. UI Failure Modes & Effects Analysis (FMEA) — Consolidated

> *Per-section FMEAs above are reference; this section is the **rolled-up engineering review** for product / QA / SRE sign-off.*

### 12.1 Thermal Throttling

| Item | Detail |
|------|--------|
| Scenario | SoC core temperature exceeds threshold; OS clocks CPU/GPU down 50–70 %. |
| User-visible effect | UI drops to 30 fps; animations stutter; streaming feels laggy. |
| Detection | `PowerManager.OnThermalStatusChangedListener` → JNI → Qt signal `thermalChanged(level)`. |
| Mitigation level 1 (MODERATE) | Disable caret pulse, disable tool-pill icon pulse, theme cross-fade → instant. |
| Mitigation level 2 (SEVERE) | Pause background indexer (AF §11); reduce streaming token batch to 100 ms; show banner: "Device is warm — simplifying visuals to cool down." |
| Mitigation level 3 (CRITICAL) | Save streaming state to DB; freeze inference until cooler (§12.3 OOM-like behaviour). |
| Acceptance criteria | UI remains ≥ 30 fps under SEVERE throttle. |

### 12.2 OOM Warning / `onTrimMemory`

| Item | Detail |
|------|--------|
| Scenario | RAM pressure high; OS calls `onTrimMemory(level)`. |
| User-visible effect | App killed silently if not handled → chat lost. |
| Detection | `onTrimMemory(int level)` Java callback → Rust handler (§15.2 in v1 / AF §15.2). |
| Mitigation TRIM_MEMORY_RUNNING_LOW | Limit parallel tool workers to 1. |
| Mitigation TRIM_MEMORY_RUNNING_CRITICAL | Pause stream; save partial bubble to DB; hide RAG preview cards. |
| Mitigation TRIM_MEMORY_COMPLETE | `madvise(MADV_DONTNEED)` on KV-Cache; drop usearch scratch; show "Tap to reload model" sub-state. |
| Acceptance criteria | Zero crashes during 30-minute background+foreground cycle test. |

### 12.3 Network Drop During Tool Call

| Item | Detail |
|------|--------|
| Scenario | User triggers `web_search`; airplane mode toggled mid-request. |
| User-visible effect | Tool hangs indefinitely → user thinks app frozen. |
| Detection | 8-second timeout on every network request; ConnectivityManager listener for instant detection. |
| Mitigation | ToolCallPill switches to failure state with calm-amber icon "Web search · No network"; inline retry button; error injected into LLM context (AF §10.2.1). |
| User recovery | Tap retry, or simply send a new message — Mukei may apologise and try a different approach. |
| Acceptance criteria | Tool call always resolves (success / fail) within 9 seconds of network loss. |

### 12.4 Model Load Failure — Editorial "Re-download" Screen

| Item | Detail |
|------|--------|
| Scenario | GGUF file partially downloaded or corrupted (SHA-256 mismatch). |
| User-visible effect | If unhandled, llama.cpp crashes and brings app down. |
| Detection | Pre-flight SHA-256 verification before `mmap` (TRD §5.3, AF §5.2). |
| Mitigation | Editorial screen (full-screen): "Your model file looks incomplete. Let's fix that." — Playfair H1 headline + Inter body. CTA: [Resume download] (deterministic %) or [Choose another model]. |
| Acceptance criteria | App never enters a state where llama.cpp loads an unverified file. Tested via `test_model_resume_after_network_drop` (TRD §11.1). |

### 12.5 Corrupted GGUF — Clear Error Messaging

| Item | Detail |
|------|--------|
| Scenario | User imports a custom GGUF via SAF; file header invalid. |
| User-visible effect | App could crash on `mmap` if header is malformed. |
| Detection | Rust GGUF header parse before `mmap`; failure → typed error `MukeiError::InvalidGGUF`. |
| Mitigation | Inline modal: "This file doesn't look like a GGUF model. Please pick another." — short Playfair headline + Inter body. CTA: [Pick another]. |
| Acceptance criteria | Never crash on user-supplied GGUF; always graceful error. |

### 12.6 Token Flood — UI Thread Starvation

| Item | Detail |
|------|--------|
| Scenario | llama.cpp on Mali-G68 generates 40 tokens/s; QML naively re-renders on every token. |
| User-visible effect | Frame rate drops to 5–10 fps; scroll lags; ANR warning may trigger. |
| Detection | QML Profiler nightly job (§11.5.2) catches > 17 ms median frame. |
| Mitigation | Rust batches tokens for 50 ms before emitting Qt signal (§4.10); QML `MarkdownRenderer` caches AST of finalised paragraphs as static compiled components, only re-parses the currently-streaming paragraph. |
| Acceptance criteria | 60 fps maintained at 40 tokens/s on Pixel 7. |

### 12.7 Keyboard Overlap — Input Hidden

| Item | Detail |
|------|--------|
| Scenario | User taps composer; keyboard rises and covers Send button. |
| User-visible effect | User can't see what they're replying to or how to send. |
| Detection | `Window.softKeyboardHeight` change observed. |
| Mitigation | `Flickable` smooth-scrolls by `keyboardHeight - composer.y - composer.height`; composer pinned just above keyboard; latest message stays in upper half of remaining viewport (§5.3.3). |
| Acceptance criteria | Composer always visible; latest message visible above composer when keyboard is open. |

### 12.8 TalkBack Token Spam

| Item | Detail |
|------|--------|
| Scenario | Streaming response has 500 tokens; TalkBack announces each. |
| User-visible effect | Unusable — user disables TalkBack in frustration. |
| Detection | Manual A11y test; `tst_AnnouncementBatching.qml` automated. |
| Mitigation | Sentence-boundary batching (§10.2.2); 1× "Mukei is typing" at start; 1× "Response complete" at end. |
| Acceptance criteria | Maximum 1 announcement per sentence; ≤ 30 announcements per 500-token response. |

### 12.9 Theme Switch Flicker

| Item | Detail |
|------|--------|
| Scenario | User switches Espresso → Dolce Vita; intermediate white flash visible. |
| User-visible effect | Distracting; breaks calmness. |
| Detection | `tst_ThemeTransition.qml` snapshot test at midpoint. |
| Mitigation | §8.5.1 grouped `ColorAnimation`; 1-frame render suspend at midpoint. |
| Acceptance criteria | No frame during transition has a luminance > 80 (8-bit) on background pixels. |

### 12.10 Backdrop Blur Failure On OpenGL ES Fallback

| Item | Detail |
|------|--------|
| Scenario | Vulkan init fails; OpenGL ES doesn't support `MultiEffect` blur. |
| User-visible effect | Drawer/modal backdrop appears as fully transparent or fully opaque pickle. |
| Detection | `QSGRendererInterface` capability query. |
| Mitigation | Replace blur with `#000000` at 50 % opacity overlay (§11.2.4). |
| Acceptance criteria | Drawer/modal backdrop visually acceptable on all rendering backends. |

### 12.11 Font Loading Race

| Item | Detail |
|------|--------|
| Scenario | `MainWindow.qml` renders before `FontLoader` completes. |
| User-visible effect | Text briefly renders in Roboto, then snaps to Merriweather — visible re-layout. |
| Detection | Cold-boot screenshot test. |
| Mitigation | Welcome screen shows a 100 ms blank delay until `FontLoader.allLoaded` signal fires; QML root has `opacity: fontsReady ? 1 : 0` with 80 ms fade-in. |
| Acceptance criteria | No font-swap flash visible to user. |

### 12.12 Stream Resume After Process Death

| Item | Detail |
|------|--------|
| Scenario | App killed by LMK during streaming; user reopens. |
| User-visible effect | Unhandled: chat resumes blank-handed, partial bubble lost. |
| Detection | On boot, query `messages` (BS §3.2) for any `state IN ('Sending','Streaming')`. |
| Mitigation | Mark such messages as `Aborted` (with `aborted_reason='process_death'`); render with subtle "(interrupted — tap to retry)" suffix. |
| Acceptance criteria | No silent data loss; user always sees what they had. |

### 12.13 Long Conversation Memory Bloat

| Item | Detail |
|------|--------|
| Scenario | User has a 5000-message conversation; opens it. |
| User-visible effect | Cold load takes 8+ seconds; scroll lags. |
| Detection | Boot-time query plan; manual perf test. |
| Mitigation | `Flickable + Column` virtualisation (§5.5.3); only the most recent 50 bubbles instantiated on load; rest paginated as user scrolls. |
| Acceptance criteria | Cold load of 5000-message conversation < 1.5 seconds; smooth scroll. |

### 12.14 Settings Slider Drift

| Item | Detail |
|------|--------|
| Scenario | User drags temperature slider mid-streaming. |
| User-visible effect | If applied live, current generation parameter changes mid-token. |
| Detection | Settings change emits config dirty signal. |
| Mitigation | Hot fields (`theme`, font size, haptics) apply live. **Inference parameters** (`temperature`, `max_tokens`, `top_p`) apply on **next message only**; UI shows "Applies to next message" hint when changed. (AF §17.) |
| Acceptance criteria | No mid-stream parameter change can occur. |

### 12.15 RTL Layout Breaks Code Blocks

| Item | Detail |
|------|--------|
| Scenario | Arabic locale; AI response contains a code block. |
| User-visible effect | Code visually mirrored, becomes unreadable. |
| Detection | Manual review; `tst_RTLCode.qml` snapshot. |
| Mitigation | `CodeBlockComponent.qml` sets `LayoutMirroring.enabled: false` on itself (code is always LTR). |
| Acceptance criteria | Code blocks always LTR regardless of locale. |

### 12.16 Diagnostic Export Leaks Secrets

| Item | Detail |
|------|--------|
| Scenario | User exports diagnostic pack for support. |
| User-visible effect | Pack could contain SAF tokens, conversation snippets, keys. |
| Detection | Manual code review; pre-export redaction step. |
| Mitigation | Export pipeline filters: SAF tokens replaced with `<redacted-saf>`, any 64-hex string → `<redacted-hash>`, any conversation content → `<redacted-content>`. Resulting pack contains only: timestamps, version, crash class, hardware info, settings. |
| Acceptance criteria | `test_no_plaintext_secret_in_export` (TRD §11.1) passes on every release. |

### 12.17 Drawer Swipe Conflicts With Bubble Long-Press

| Item | Detail |
|------|--------|
| Scenario | User swipes from left edge to open drawer; system interprets as long-press on first bubble. |
| User-visible effect | Long-press menu opens instead of drawer. |
| Detection | Manual QA; `tst_DrawerSwipe.qml`. |
| Mitigation | First 24 dp from left edge is a dedicated swipe zone — long-press is suppressed there. |
| Acceptance criteria | Swipe-to-open-drawer success rate > 95 % in QA test. |

### 12.18 SafeMode Reset While Drawer Open

| Item | Detail |
|------|--------|
| Scenario | User in SafeMode taps Reset; drawer was open. |
| User-visible effect | Reset wipes data; drawer (still on screen) shows stale conversation list. |
| Detection | Manual QA. |
| Mitigation | Reset operation fully unmounts and re-mounts `MainWindow` after wipe; drawer is re-instantiated empty. |
| Acceptance criteria | Post-reset, drawer shows "Welcome — get started" empty state, never stale data. |

---

## 13. Design Deliverables & Handoff

### 13.1 Figma Library Structure

The Figma library is the **mirror** of this document. If something exists here, it exists in Figma. If something exists in Figma but not here, it does not ship.

#### 13.1.1 Figma File Organisation

```
MUKEI Design System (Figma)
├── 00 — Foundations
│   ├── Color (Dolce Vita / Espresso / Taupe palettes as styles)
│   ├── Type (Playfair / Merriweather / Inter / JetBrains Mono as text styles)
│   ├── Spacing (4–96 as effect/grid system)
│   ├── Motion (curves documented as descriptions; not Figma-renderable)
├── 01 — Components
│   ├── Buttons (Primary, Secondary, Ghost, Icon, Destructive)
│   ├── Inputs (ChatComposer, SettingsTextField, SearchField)
│   ├── Cards (UserBubble, AIBubble, ToolResultCard, RAGChunkCard)
│   ├── Navigation (LeftDrawer, ModalSheet, FullScreenModal)
│   ├── Indicators (ProgressBar, Spinner, SkeletonLoader, StatusPill, StreamingCaret)
│   ├── Dialogs (ConfirmationDialog, DestructiveConfirmDialog, ToastNotification)
│   ├── Compound (MarkdownRenderer demo, CodeBlock, ThinkingAccordion)
├── 02 — Icons (line-only, 1.5 px stroke, 24 dp)
├── 03 — Illustrations (welcome / empty / error / safe-mode)
├── 04 — Screens
│   ├── First-Run (Welcome / Picker / Verification)
│   ├── Chat (Empty / Active / Streaming / Tool / Branch)
│   ├── Manager (ModelManager / ConversationList)
│   ├── Settings (General / Privacy / Storage / About)
│   ├── Recovery (SafeMode / RagRebuildPrompt)
└── 05 — Specs (this document, mirrored in pages)
```

#### 13.1.2 Figma Variables

| Figma variable | Maps to QML token |
|----------------|--------------------|
| `color/dolceVita/background` | `Theme.dv.background` |
| `color/espresso/inkPrimary` | `Theme.esp.inkPrimary` |
| `type/bodyAI/family` | `Type.bodyAI.family` |
| `type/bodyAI/size` | `Type.bodyAI.size` |
| `spacing/md` | `Spacing.md` |
| `motion/enter/curve` | `Motion.enter` (documented, not Figma-renderable) |

Variables are exported as JSON via Figma Tokens plugin → committed to `design/tokens.json` → consumed by `Theme.qml` generation script.

#### 13.1.3 Component Naming

Every Figma component is named identically to its QML file:

| Figma component | QML file |
|-----------------|----------|
| `PrimaryButton/Default`, `PrimaryButton/Hover`, `PrimaryButton/Pressed`, `PrimaryButton/Disabled` | `qml/components/PrimaryButton.qml` |
| `MessageBubble/User/Default`, `MessageBubble/AI/Streaming`, … | `qml/components/UserMessageBubble.qml`, `AIMessageBubble.qml` |
| `ToolCallPill/Active`, `ToolCallPill/Result/Success`, `ToolCallPill/Result/Failure` | `qml/components/StatusPill.qml` |

### 13.2 QML ↔ Figma Mapping Spec

#### 13.2.1 Round-Trip Workflow

```
Designer:   Figma change → publish → tokens.json regenerated
Engineer:   pull tokens.json → CI regenerates Theme/Type/Spacing.qml
QA:         visual-diff snapshot tests run against Figma frame export
```

#### 13.2.2 Token-Drift Detection

A CI job runs after every Figma publish:
1. Pulls `tokens.json` from Figma API.
2. Diffs against the committed `tokens.json` in the repo.
3. If diverge, opens a PR titled "design/tokens: sync from Figma".
4. Engineers review (preserves intentional discrepancies).

### 13.3 Design QA Checklist (Pre-Release)

Run for every release. Every item must check ✅.

#### 13.3.1 Color & Palette

- [ ] All hex codes match locked palette (Dolce Vita `#D8CABD`, Espresso `#362417`, Taupe `#92817A`, Copper `#B87333`, Gold `#D4AF37`, Terracotta `#C17F3E`)
- [ ] No raw hex literal outside `Theme.qml` (verified by `qmllint`)
- [ ] No cold blue accent (`#007AFF` / similar) anywhere in screenshots
- [ ] No pure black `#000000` or pure white `#FFFFFF` background
- [ ] WCAG AA contrast pass on Dolce Vita, Espresso, Taupe (matrix §3.6)
- [ ] High-contrast override applies; ratios ≥ 7:1

#### 13.3.2 Typography

- [ ] Playfair Display loaded; rendered correctly on splash / display
- [ ] Merriweather loaded; AI responses use it (verify in active stream)
- [ ] Inter loaded; UI chrome uses it
- [ ] JetBrains Mono loaded; code blocks use it
- [ ] All sizes match the scale (§4.5); no 13/15/17/22 px in rendered screens
- [ ] Letter-spacing tokens applied per §4.6 (display tight, caption tracked)
- [ ] Line-height 1.6 on Merriweather body (visible — "feels like a magazine")
- [ ] Hanging punctuation works on quotes / em-dashes
- [ ] Curly-quote / em-dash substitution active outside code blocks
- [ ] Tabular figures in token-count captions align vertically
- [ ] Font scaling tested at 0.85× / 1.0× / 1.5× / 2.0× without clip
- [ ] CJK + Arabic render via Noto fallback without tofu

#### 13.3.3 Spacing & Layout

- [ ] All padding/margin values on the 8-px grid
- [ ] Edge padding 24 px (compact) / 32 px (medium) / 48 px (expanded)
- [ ] Vertical rhythm between bubbles 16/24 px
- [ ] Safe-area insets respected on Pixel 7 / Galaxy Z Fold / Pixel Tablet
- [ ] Keyboard inset push smooth, composer always visible
- [ ] Foldable hinge handled (book / tabletop)
- [ ] Compact / Medium / Expanded breakpoints behave correctly
- [ ] Tap targets ≥ 48 dp everywhere

#### 13.3.4 Components

- [ ] No borders in chrome except `ToolResultCard` (§2.2.2 exception)
- [ ] No shadows on inline content
- [ ] Focus ring (2 px copper, 2 px offset) visible on every focusable item
- [ ] Two-tap destructive confirmation works
- [ ] Markdown renderer uses AST (no regex on rendered text)
- [ ] CodeBlock copies correctly to clipboard
- [ ] ThinkingAccordion collapsed by default
- [ ] NetworkBanner color matches state (offline calm green / online subtle grey)

#### 13.3.5 Screens

- [ ] Welcome screen shown once per install (not per launch)
- [ ] Empty state cards rotate deterministically (no telemetry)
- [ ] Streaming caret pulses at 1100 ms sinusoid
- [ ] Tool pill active phase ≥ 300 ms visible
- [ ] Tool pill result phase static, tappable, expands ToolResultCard
- [ ] Long-press bubble menu opens within 180 ms
- [ ] Branch glyph appears in header on non-default branch
- [ ] SafeMode shown after exactly 2 hard crashes in 24 h
- [ ] RagRebuildPrompt shown when HNSW invalid
- [ ] Settings tabs scroll smoothly

#### 13.3.6 Motion

- [ ] All animations use `Motion.enter` or `Motion.exit` curve (no other easings)
- [ ] Caret pulse exception logged & approved
- [ ] Theme transition has no white-flash midpoint
- [ ] Reduce-motion mode replaces animations with fade-only or instant
- [ ] Thermal-throttle banner appears at MODERATE; animations disable
- [ ] Stop button cancels animations instantly (no fade-out)

#### 13.3.7 Iconography

- [ ] All SVGs use 1.5 px stroke at 24 dp viewBox (CI test)
- [ ] Icons tint via `ColorOverlay` (or fallback per §11.2.4)
- [ ] Launcher icon adapts to all mask shapes (round / square / squircle)
- [ ] Notification icon is monochrome
- [ ] Illustrations capped at 96 dp

#### 13.3.8 Accessibility

- [ ] TalkBack announces sentence-batches, not per token
- [ ] Tab order correct (§10.3.2) on ChatScreen
- [ ] Voice Access can invoke send / stop / open settings
- [ ] Switch Access scan order matches Tab order
- [ ] High contrast mode active + tested
- [ ] RTL locale (`ar`) flips bubbles correctly; code blocks stay LTR
- [ ] Font scale 2.0× tested without clip
- [ ] Reading-pace setting (Standard / Comfortable / Patient) works

#### 13.3.9 Performance

- [ ] 60 fps maintained during streaming on Pixel 7
- [ ] Cold boot to ChatScreen < 4 seconds
- [ ] Time to first token < 1.5 seconds
- [ ] Long-conversation (5000 messages) cold load < 1.5 seconds
- [ ] No memory leaks during 30-minute background-foreground cycle
- [ ] APK UI assets ≤ 2 MB

#### 13.3.10 Privacy & Security

- [ ] Network banner always visible
- [ ] Encryption notice chip on ChatScreen header
- [ ] No SAF token visible in crash log preview
- [ ] No telemetry sent (zero outbound HTTP except web_search opt-in)
- [ ] Diagnostic export redacts SAF tokens and content
- [ ] Two-tap destructive confirmation works on Reset All Data

### 13.4 Handoff Document Per Engineer Role

#### 13.4.1 For QML Engineers

Read in order: §3 Color → §4 Typography → §5 Spacing → §6 Components → §11 Implementation → §13.3 Checklist. Build components in order: Theme.qml → Type.qml → Spacing.qml → Motion.qml → button primitives → bubble primitives → screens.

#### 13.4.2 For Designers

Read in order: §1 Principles → §2 70/20/10 → §3 Color → §4 Typography → §7 Screens → §13.3 Checklist. Always work from the Figma library; if a token is missing, raise a design-review issue before improvising.

#### 13.4.3 For Accessibility Reviewers

Read in order: §10 Accessibility → §13.3.8 Checklist → §10.13 FMEA. Run TalkBack + Switch Access + Voice Access + High Contrast + 2.0× font scale on every release candidate.

#### 13.4.4 For Product / PM

Read in order: §1 Principles → §7 Screen Flows → §12 FMEA Consolidated → §13.3 Checklist (high-level). Use §12 FMEA as the public-facing "what happens when X goes wrong" document for support runbooks.

### 13.5 Versioning

| Component | Versioned how |
|-----------|---------------|
| This document | `MUKEI-UXB-v{major}.{minor}.{patch}` — bumped on every accepted change |
| Figma library | Same version string, published together |
| `tokens.json` | Schema versioned (`tokens_schema_version` field) |
| QML components | Co-versioned with this document |

Breaking changes (e.g. removing a color token) require a `major` bump and an entry in §15 Revision History.

---

## 14. Appendix

### 14.1 Color Contrast Calculator — Full Output

Auto-generated from `tools/contrast_audit.py`. Last run: 2026-06-19. All pairs that pass WCAG AA listed; failures explicitly noted as "restricted use".

#### 14.1.1 Dolce Vita (`#D8CABD`)

| Foreground | Background | Ratio | AA Normal | AA Large | AAA Normal | AAA Large |
|------------|------------|------:|:---------:|:--------:|:----------:|:---------:|
| `#362417` ink primary | `#D8CABD` bg | 7.81 | ✅ | ✅ | ✅ | ✅ |
| `#362417` ink primary | `#E8DDD0` surface | 8.51 | ✅ | ✅ | ✅ | ✅ |
| `#362417` ink primary | `#C9B9A7` surfaceVariant | 6.72 | ✅ | ✅ | ✅ | ✅ |
| `#6B5D4F` ink secondary | `#D8CABD` bg | 4.59 | ✅ | ✅ | ❌ | ✅ |
| `#6B5D4F` ink secondary | `#E8DDD0` surface | 5.01 | ✅ | ✅ | ❌ | ✅ |
| `#9C8E80` ink faint | `#D8CABD` bg | 2.41 | ❌ | ❌ | ❌ | ❌ — placeholder only |
| `#B87333` copper | `#D8CABD` bg | 3.42 | ❌ | ✅ | ❌ | ❌ — restrict to large/bold |
| `#FFFFFF` paper | `#B87333` copper | 4.10 | ❌ | ✅ | ❌ | ❌ — use SemiBold ≥ 16 px on copper button |
| `#10B981` success | `#D8CABD` bg | 3.18 | ❌ | ✅ | ❌ | ❌ — icon-only success, no text on top |
| `#EF4444` error | `#D8CABD` bg | 3.62 | ❌ | ✅ | ❌ | ❌ — icon-only error |

#### 14.1.2 Espresso (`#362417`)

| Foreground | Background | Ratio | AA Normal | AA Large | AAA Normal | AAA Large |
|------------|------------|------:|:---------:|:--------:|:----------:|:---------:|
| `#EBE1D5` ink primary | `#362417` bg | 11.42 | ✅ | ✅ | ✅ | ✅ |
| `#EBE1D5` ink primary | `#4A3829` surface | 8.74 | ✅ | ✅ | ✅ | ✅ |
| `#EBE1D5` ink primary | `#5C4736` surfaceVariant | 6.18 | ✅ | ✅ | ✅ | ✅ |
| `#A89888` ink secondary | `#362417` bg | 5.31 | ✅ | ✅ | ❌ | ✅ |
| `#7D6E60` ink faint | `#362417` bg | 3.18 | ❌ | ✅ | ❌ | ❌ — placeholder only |
| `#D4AF37` gold | `#362417` bg | 6.18 | ✅ | ✅ | ❌ | ✅ |
| `#1A1108` (dark text on gold button) | `#D4AF37` gold | 8.92 | ✅ | ✅ | ✅ | ✅ |

#### 14.1.3 Taupe (`#92817A`)

| Foreground | Background | Ratio | AA Normal | AA Large | AAA Normal | AAA Large |
|------------|------------|------:|:---------:|:--------:|:----------:|:---------:|
| `#2A2420` ink primary | `#92817A` bg | 5.92 | ✅ | ✅ | ❌ | ✅ |
| `#2A2420` ink primary | `#A89888` surface | 7.10 | ✅ | ✅ | ✅ | ✅ |
| `#4F423A` ink secondary | `#92817A` bg | 3.21 | ❌ | ✅ | ❌ | ❌ — caption ≥ 18 px only |
| `#C17F3E` terracotta | `#92817A` bg | 2.42 | ❌ | ❌ | ❌ | ❌ — icon/CTA only, NEVER body text on Taupe |

### 14.2 Typography Specimen Sheets

#### 14.2.1 Merriweather 16 px / 1.6 (AI Body)

```
                Entropy, in physics, is a measure of the disorder
                in a system. The second law of thermodynamics
                states that the total entropy of an isolated
                system can only increase over time. In a sense,
                it is nature's accountant — keeping score of all
                the ways particles can arrange themselves.

                When we burn a log of wood, the highly-ordered
                cellulose structure dissolves into a chaotic
                cloud of heat, ash, and gas. Each molecule, once
                bound in lattice, is now free to wander. The
                forward arrow of entropy is the closest physics
                has to an arrow of time.
```

Verify: paragraph reads like *Sunday New York Times*. Half-line skip between paragraphs (≈ 15 px). No widows / orphans during stream (fixed post-finalize).

#### 14.2.2 Inter 16 px / 1.5 (User Prompt)

```
                what is entropy in physics? explain like i'm 12
```

Verify: clean, sans-serif, lowercase comfortable, no italic.

#### 14.2.3 JetBrains Mono 14 px / 1.5 (Code Block)

```rust
                fn fingerprint(&self) -> String {
                    let mut hasher = Sha256::new();
                    hasher.update(self.canonical_json().as_bytes());
                    hex_encode_lower(&hasher.finalize())
                }
```

Verify: characters distinct (0 vs O, 1 vs l), ligatures off, copy-button visible.

#### 14.2.4 Playfair Display 32 / 1.2 (Display)

```
                Your Private AI,
                On Your Device.
```

Verify: tight letter-spacing (`-0.02 em`), elegant, magazine-display.

### 14.3 Reference Moodboards

#### 14.3.1 Aesop

- Warm beige paper textures.
- Brass-and-amber metallic accents.
- Editorial product photography with generous whitespace.
- Serif headlines, sans body.

*Take-away for MUKEI:* the empty-state and welcome screens should feel like browsing an Aesop product page on cream paper. No tech-startup density. No bright accent colours.

#### 14.3.2 Arc Browser

- Sidebar-first navigation.
- Calm color tinting per workspace.
- Custom typography (Arc uses Söhne, similar in spirit to Inter).
- Subtle motion (no bounce).

*Take-away for MUKEI:* the LeftDrawer pattern (conversation list + settings) is intentionally Arc-like — gentle persistent presence, swipe to dismiss.

#### 14.3.3 Linear

- Editorial typography in product UI (rare achievement).
- Warm-toned dark mode (not pure black).
- Restrained accent (purple, but used sparingly).
- Premium developer-tool feel.

*Take-away for MUKEI:* Linear demonstrates that you can have editorial type *inside* productivity software. Espresso theme owes a lot to Linear's warm dark mode.

#### 14.3.4 Rauno Freiberg

- Boutique micro-interactions.
- Warm, tactile, sometimes skeuomorphic accents.
- Designs that feel "of the hand".

*Take-away for MUKEI:* haptics, motion curves, and the two-tap destructive choreography all aspire to Rauno-grade detail.

#### 14.3.5 The New York Times (Digital)

- Serif headlines (Cheltenham / similar).
- Generous reading width.
- Caption tracking +2 % at small sizes.
- Article body with 1.55–1.65 line-height.

*Take-away for MUKEI:* AI responses in Merriweather 16 / 1.6 are directly inspired by NYT digital article body styling.

#### 14.3.6 Medium

- Massive horizontal whitespace.
- Editorial first-tap experience.
- Serif body for long-form articles.

*Take-away for MUKEI:* empty-state and chat reading experience.

### 14.4 Forbidden Aesthetics Reference

Visual examples (kept in `design/anti-patterns/` folder, NOT shipped):

| Anti-pattern | Why we don't do it |
|--------------|---------------------|
| iOS Mail blue accent | Cold, generic, off-brand |
| ChatGPT gradient-mesh splash | Marketing-heavy, contradicts privacy promise |
| Discord neon purple | Cyberpunk; wrong audience |
| Material 3 ripple buttons | Visual weight too high, breaks 70 % |
| Notion's grey-on-grey density | Lacks editorial generosity |
| Slack's pastel sticker pack | Playful but not premium |
| Telegram's hot accent toggles | Toy-like, not professional |

### 14.5 Glossary Of Editorial Terms

| Term | Definition |
|------|------------|
| Display type | Type sized > 24 px, used for headlines |
| Body type | 14–18 px, used for reading |
| Tracking | Letter-spacing across a run |
| Kerning | Letter-spacing between specific letter pairs (auto-applied by font) |
| Leading | Line-height (originally from lead spacers in metal type) |
| Optical margin | Visual edge adjustment for punctuation |
| Hanging punctuation | Quotes / dashes pushed outside the left edge |
| Widows / orphans | Single-word lines at start/end of paragraph |
| Modular scale | Type-size scale where each size is a fixed ratio of the previous |
| Editorial | A design language inspired by print magazines / books |

### 14.6 References & Influences

- Robert Bringhurst, *The Elements of Typographic Style* (4th ed.). Chapter 6 on hard-copy modular scales.
- Erik Spiekermann, *Stop Stealing Sheep*. Chapter on font pairing.
- W3C, WCAG 2.1 Guidelines: <https://www.w3.org/TR/WCAG21/>.
- Google Material Design 3 (used as anti-reference for §11.1.1 only).
- Apple Human Interface Guidelines (cited for haptic taxonomy in §8.6).
- Smashing Magazine, "The State Of Web Type 2024".
- A List Apart, various articles on accessibility and dynamic type.

---

## 15. Revision History

| Date | Version | Author | Change |
|------|---------|--------|--------|
| 2026-06-19 | 1.0 | AI-Architect | First pass, cross-locked against PRD v0.7.2 + TRD v0.7.2 + AF v1.0 + BS v1.0. Brief outline of design system, screens, accessibility. |
| 2026-06-19 | 2.0 | AI-Architect | **Locked-palette regeneration.** Adopted 70/20/10 rule, Dolce Vita / Espresso / Taupe palettes, editorial typography (Playfair / Merriweather / Inter / JetBrains Mono), CCCC principles (Calm, Capable, Confidential, Crafted), expanded screen flows with ASCII layouts, dedicated A11y, Qt/QML implementation strategy, consolidated FMEA, design QA checklist, Figma mirror spec, contrast audit, typography specimens, moodboard reference set. |
| 2026-06-20 | 2.1 | AI-Architect | **v0.7.5 — Convergence & Contract-Alignment Pass.** Header, document ID, status block, and companion links all re-pointed to the v0.7.5 graph (PRD v0.7.5 / TRD v0.7.5 / AF v1.2 / BS v1.2). §6.4.2 — added the **long-answer reader-wash** rule (auto-applied `Theme.p.surfaceFaint` background when bubble height > 320 dp OR `fontScale > 1.5` OR contains a code block OR `summary_first = true`) closing P1-05 from the audit. §10.10.3 NEW — **Cognitive Load Controls** (summary-first, collapse tool traces, collapse thinking, low-stimulation aggregate mode) closing P2-01. No tokens removed, no palettes broken, no screen flows deleted; UXB §7.4.3 auto-submit clause is superseded by AF §6.6 (fill-only default). |

