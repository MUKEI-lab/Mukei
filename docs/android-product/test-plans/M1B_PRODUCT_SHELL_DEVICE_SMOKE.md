# M1B Product Shell — Real-Device Smoke Plan

Status: **Execution checklist**  
Target: signed ARM64 test APK produced from the exact M1B review commit.

This checklist records whether the first product shell satisfies its bounded milestone contract on a physical Android device. It is not a substitute for later Conversation/Storage/Models acceptance tests.

## Test evidence header

Record before testing:

- APK source commit / PR head:
- CI run ID:
- signing verification run ID:
- APK SHA-256:
- device model:
- Android version:
- fresh install or upgrade:
- test timestamp:

---

## A. Install and cold launch

### A1 — Officially verified APK installs

1. Remove an older Mukei test build if it uses a different temporary signing certificate.
2. Install the officially `apksigner`-verified ARM64 APK.

Pass:

- Android installs the package without `App not installed` or signature parse failure.

Fail evidence:

- capture Android/package-installer message;
- do not retry with an unofficial signer.

### A2 — Cold launch reaches bounded startup then product shell

1. Force-stop Mukei.
2. Launch from launcher.

Pass:

- no immediate native-load crash;
- bounded `Opening Mukei…` startup state may appear briefly;
- successful startup transitions to the Home/product shell;
- old backend diagnostic landing page is not the primary success surface.

### A3 — Model artifacts missing is not total app failure

On a device without installed inference artifacts:

Pass:

- Home remains navigable;
- UI explains that model artifacts are required;
- secure backend/storage are not labeled as totally unavailable;
- `Open Models` navigates to Models.

---

## B. Home opening contract

### B1 — Opening hierarchy

Pass if Home visibly prioritizes:

1. quiet top bar;
2. time-aware/neutral greeting;
3. `What’s on your mind?`;
4. composer;
5. optional capability chips.

Fail if:

- Home becomes a dashboard;
- a large repeated `Mukei`/`Home` branding title dominates;
- recent projects/files dominate opening state.

### B2 — Top bar

Pass:

- left Menu affordance is present without a text `Menu` label;
- New Chat affordance is present;
- Options affordance is present but visibly/non-interactively unavailable in this bounded build rather than exposing fake actions.

### B3 — Composer honesty

Pass:

- text can be entered as a local draft;
- Send is disabled in this milestone;
- no fake assistant response or fake backend action occurs;
- UI does not imply Conversation MVP is already implemented.

---

## C. Capability chips

For each visible chip:

- Deep Research
- Build App
- Read Files
- Write
- Code

Tap one chip.

Pass:

- chip selection is optional;
- selecting a chip changes composer context/placeholder;
- user can deselect it;
- no chip is required before typing;
- no backend operation starts merely by selecting a chip.

---

## D. Navigation drawer

### D1 — Locked hierarchy

Open drawer.

Expected order/grouping:

```text
Mukei

Storage
Projects
Models

Chats

Settings  (anchored at bottom)
```

Pass:

- hierarchy matches;
- Settings appears at the bottom;
- drawer feels modal rather than replacing the whole app.

### D2 — Destination behavior

Open each destination.

Pass:

- Storage / Projects / Chats / Settings render explicit reserved-state content rather than fake working features;
- Models renders current inference readiness explanation;
- Mukei returns Home.

---

## E. Back behavior

### E1 — Drawer first

1. Open drawer.
2. Press Android Back.

Pass:

- drawer closes;
- app does not exit.

### E2 — Top-level destination to Home

1. Navigate to Storage, Projects, Models, Chats, or Settings.
2. Press Android Back.

Pass:

- returns to Home first;
- does not exit immediately from the non-Home destination.

### E3 — Home back

From Home with no transient UI open:

Pass:

- Android system back behavior is not hijacked by an infinite in-app loop.

---

## F. New Chat bounded behavior

1. On Home, type a draft.
2. Select a capability chip.
3. Navigate to another top-level destination.
4. Tap New Chat.

Pass:

- returns to Home;
- local draft is cleared;
- selected capability context is cleared;
- no fake conversation record is created merely by pressing New Chat in this bounded shell milestone.

---

## G. Layout resilience

### G1 — Small phone / portrait

Pass:

- primary Home content remains reachable;
- no critical horizontal clipping;
- long readiness copy can be scrolled.

### G2 — Large font

Set Android font size to a large accessibility setting and relaunch.

Pass:

- greeting, readiness card, composer, chips and actions remain reachable;
- content can scroll vertically;
- controls do not overlap in a way that prevents use.

### G3 — Dark theme sanity

Enable system dark theme.

Pass for this provisional milestone:

- text remains readable;
- surfaces remain distinguishable;
- no obvious white-on-white / dark-on-dark failures.

Dark palette polish is not considered locked by this test.

---

## H. Lifecycle smoke

### H1 — Background / return

1. Open Home.
2. Background app.
3. Return.

Pass:

- app does not crash;
- shell returns to a truthful state.

### H2 — Force-stop / relaunch

1. Force-stop app.
2. Relaunch.

Pass:

- secure runtime starts again;
- shell reaches Home/readiness state without requiring app-data deletion.

In-process Retry after a terminal runtime failure belongs to the separate runtime-lifecycle hardening slice and must not be claimed unless that fix is included in the tested APK.

---

# Acceptance result

Mark one:

- [ ] PASS — M1B device smoke accepted for merge.
- [ ] PASS WITH FOLLOW-UPS — no milestone blocker; record issues below.
- [ ] FAIL — blocker found; do not merge M1B.

## Findings

Record each issue with:

- step ID;
- screenshot/video;
- observed behavior;
- expected behavior;
- reproducibility;
- device/Android version;
- blocker or follow-up classification.

## Merge rule

M1B should not be treated as device-accepted merely because CI compiles or a screenshot looks plausible. The exact officially signed APK from the review commit must pass the applicable checks above on a real device.
