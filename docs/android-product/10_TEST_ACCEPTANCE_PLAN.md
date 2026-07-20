# 10 — Test and Acceptance Plan

Status: **Draft v0.1**

This document defines when an Android product slice is actually done.

Passing compilation/unit tests is necessary but insufficient. Mukei crosses Kotlin, JNI, Rust, SQLCipher, Android Keystore, native shared libraries, file/storage APIs, Compose lifecycle, and model/runtime capability boundaries. Acceptance must validate the assembled app on Android.

## Core rule

> A feature is complete only when the intended end-to-end user flow passes its acceptance criteria on an installable, correctly signed APK on a representative Android device/emulator.

---

# 1. Test pyramid

```text
Fast unit/static tests
        ↓
Module/integration tests
        ↓
Rust/JNI/security tests
        ↓
APK assembly/package verification
        ↓
Instrumented/emulator flows
        ↓
Real-device smoke/acceptance
```

No layer substitutes for the layer above it.

---

# 2. Required CI gates

Canonical Android CI should include:

## Kotlin/Android

- compile all affected modules;
- JVM unit tests;
- lint;
- Compose/UI tests where available;
- debug/offline assembly;
- release/R8 assembly.

## Rust

- core unit/integration tests;
- feature-matrix checks;
- SQLCipher/storage migrations;
- security boundary tests;
- JNI crate build/tests;
- storage crash/isolation tests once ported.

## Native release

- build `arm64-v8a`;
- build `x86_64` emulator ABI;
- package required transitive shared libraries;
- verify JNI symbols/ABI outputs;
- verify release APK contains expected native dependencies.

## Artifact

- APKs generated for intended ABIs;
- package is aligned;
- release/test APK signed by official Android tooling;
- `apksigner verify --verbose --print-certs` passes;
- checksums recorded for shared test artifacts.

---

# 3. APK package acceptance

Before any APK is handed to a tester:

## Signature

MUST pass official:

```text
apksigner verify --verbose --print-certs <apk>
```

For temporary test builds, certificate identity must be documented.

If a new temporary key is used, testers must be warned that an existing differently-signed package may need uninstall before install.

## Native libraries

For each ABI APK, verify required libraries exist.

Current release baseline includes at least:

```text
lib/<abi>/libmukei_android.so
lib/<abi>/libmukei_llama_native.so
lib/<abi>/libc++_shared.so
```

Native dependency verification SHOULD inspect ELF `DT_NEEDED` dependencies and fail CI when an app-packaged dependency is missing.

Do not limit checks to `libmukei_android.so exists`.

## Alignment

Verify native library/page alignment according to current Android/NDK requirements used by the project.

## Manifest

Verify:

- package/application ID expected;
- min/target SDK expected;
- permissions appropriate to build flavor;
- offline flavor does not accidentally include network permission/capability where policy forbids it.

---

# 4. Installation acceptance

Every release-hardening candidate used for testing must be installed using Android package manager, not merely unzipped/inspected.

Minimum checks:

```text
adb install <apk> → success
launch main activity → process remains alive
force-stop + relaunch → success
uninstall/reinstall → clean first-launch success
```

For upgrade scenarios:

- install previous compatible signed build;
- create representative data;
- upgrade same-signature build;
- verify database/migration/runtime startup;
- verify data remains accessible.

Temporary signing-key builds cannot validate production upgrade compatibility across different certificates.

---

# 5. Device/API matrix

The app min SDK and target/compile SDK define the supported range; test a representative matrix rather than only latest emulator.

Recommended baseline categories:

- **minimum supported API** — catches old-platform behavior;
- **mid-range API/device** — representative installed base;
- **modern API** — current Android behavior/security restrictions;
- **current target API emulator** — newest platform compatibility;
- **physical ARM64 device** — mandatory native/runtime smoke.

At minimum for each release candidate:

```text
x86_64 emulator: launch + core flow
ARM64 physical device: install + cold start + core flow
```

Storage/file-picker tests should include at least one device/OEM file-provider environment beyond a single emulator before release certification.

---

# 6. F01 — Startup/readiness acceptance

## Fresh install

1. Install APK.
2. Launch.
3. Native library loads successfully.
4. Database key is generated/wrapped.
5. SQLCipher database opens.
6. migrations complete.
7. runtime initializes.
8. product shell appears.

Expected:

- no `database_key_generation_failed`;
- no generic `backend_runtime_failed` caused by missing packaged dependencies;
- healthy backend + missing model is represented as usable shell with model capability unavailable, not app failure.

## Relaunch

- existing wrapped database key unwraps;
- existing encrypted DB opens;
- no silent key replacement/data orphaning;
- product shell restores truthful readiness.

## Failure injection

Where feasible test:

- invalid/corrupt DB/key scenario;
- unavailable storage directory;
- protocol incompatibility fixture;
- missing/corrupt native dependency in package test fixture;
- migration failure fixture.

User-facing state must provide stable recovery/diagnostic code without exposing raw secrets.

---

# 7. M1 — Product shell acceptance

## Home

- launches into Home after readiness;
- composer visually primary;
- no mandatory capability selection;
- drawer opens/closes;
- New Chat produces clean starting state;
- large font does not hide composer/action;
- keyboard opening does not create destructive layout jump.

## Drawer

- destinations reachable offline where applicable;
- selected item perceivable without color only;
- Settings remains reachable with long chat list;
- Back closes drawer first;
- screen reader focus remains trapped/restored correctly for modal drawer.

---

# 8. M2 — Conversation acceptance

## Send

- enter text;
- tap Send once;
- exactly one user intent/command submitted;
- duplicate taps do not duplicate message;
- acknowledgement accepted transitions to Running;
- response appears through typed projection.

## Stop

- Stop visible only while supported;
- tap Stop;
- state becomes Cancelling;
- duplicate Stop suppressed;
- terminal cancelled/completed race resolves truthfully.

## Streaming

- composed chunks do not duplicate final content;
- scrolling older content is not yanked to bottom;
- new-response-below indicator works;
- rotation/recomposition does not restart command.

## Model unavailable

- app remains navigable;
- conversation submission explains capability requirement;
- direct navigation to Models/provider config works;
- backend is not mislabeled as globally unavailable.

## Process death

During/after a conversation:

- kill process;
- relaunch;
- durable messages restore;
- active operation is reconciled according to runtime recovery policy;
- UI does not remain permanently `Running` from stale local state;
- interrupted work offers correct recovery action.

---

# 9. Activity acceptance

- collapsed Activity shows meaningful phase, not raw log;
- expanded Activity shows grouped operations;
- parallel file/search actions do not spam main conversation;
- real counts/progress only;
- no fake percentage;
- approval requirement becomes visible/actionable;
- failure identifies what remains preserved.

Screen reader announcements should occur for meaningful phase changes, not every item/token.

---

# 10. M3 — Storage/import acceptance

## Import happy path

- choose file via system picker;
- URI/access handled correctly;
- file admitted under policy;
- staged plaintext remains app-private;
- encrypted object written;
- logical node/version committed;
- file appears in correct scope;
- temporary staging cleaned;
- optional indexing state progresses separately.

## Required failure cases

- unsupported file;
- oversized file;
- permission revoked;
- picker cancelled;
- source changes during import;
- low storage/I/O failure;
- encryption/object publication failure;
- DB commit failure;
- indexing failure after storage success;
- process death at recoverable transaction phases.

## Assertions

- no published ghost node when commit failed;
- no plaintext left indefinitely in staging;
- no cross-workspace import;
- `stored but indexing failed` is represented truthfully;
- retry is idempotent and does not duplicate committed file unexpectedly.

---

# 11. M4 — Workspace acceptance

- workspace opens from conversation card;
- file list matches authoritative scope;
- Created/Edited/Imported states accurate;
- active mutation does not hide existing files;
- partial build failure preserves committed files;
- workspace A cannot read/mutate workspace B through manipulated IDs;
- process restart restores workspace state;
- export action uses correct workspace target.

## Isolation security tests

Attempt:

- parent node from different scope;
- import target from different scope;
- stale workspace ID;
- chat/workspace mismatch;
- root/system role duplication.

All must fail closed.

---

# 12. M5 — Artifact/export acceptance

- completed deliverable appears as ArtifactCard;
- name/type/size/count accurate when available;
- internal artifact remains after export/share;
- picker cancellation does not delete/mark artifact failed;
- external write failure leaves internal artifact ready;
- successful export confirmation is truthful;
- re-export works after conversation restart if artifact exists.

---

# 13. M6 — Models acceptance

## Inventory

- installed models shown accurately;
- active state accurate;
- local/remote label correct;
- incompatible model clearly explained.

## Download

- real byte progress when available;
- cancellation works;
- pause/resume only shown/tested if implemented;
- interrupted download recovery policy tested;
- hash/verification failure rejected;
- low storage handled without corrupt installed state.

## Activate

- installed model activation transitions explicitly;
- activation failure does not falsely mark Active;
- backend readiness and inference readiness remain distinct.

---

# 14. M7 — Projects acceptance

After ADR/domain implementation:

- project create/open/rename/delete;
- chat attach/detach;
- workspace/context association;
- active project context visible;
- mutating action targets explicit project/workspace;
- deleting chat does not silently delete independent project files;
- deleting project obeys documented ownership/cascade semantics.

---

# 15. Settings/privacy acceptance

- settings persist across restart where intended;
- secure secrets are not displayed/logged in plaintext;
- provider configuration explains remote behavior;
- memory deletion scope explicit;
- storage deletion/reset confirmation explicit;
- reduced motion setting affects transitions;
- appearance/text-size changes do not break layouts.

---

# 16. Accessibility acceptance

For each primary screen:

## Screen reader

- meaningful content labels;
- controls have names/roles/states;
- selected/active state announced;
- focus order logical;
- modal surfaces trap/restore focus;
- Activity does not flood announcements.

## Dynamic text

Test at large accessibility font scales:

- no essential text clipping;
- primary actions reachable;
- drawer/sheets scroll;
- composer remains usable;
- cards reflow rather than overlap.

## Touch

- interactive targets minimum 48×48dp;
- adjacent icon buttons do not create accidental overlap.

## Color

- status distinguishable without color;
- contrast verified for semantic text/surfaces.

## Motion

With reduced motion:

- translations/expressive motion removed/reduced;
- no loss of state understanding.

---

# 17. Offline/local-first acceptance

Test device with network disabled.

Expected:

- app starts;
- local storage/workspace/settings remain usable;
- local model works if installed;
- remote-dependent action explains unavailability;
- drawer/navigation not blocked;
- no repeated network-error spam;
- offline flavor obeys permission/network policy.

---

# 18. Security/privacy acceptance

## Logs

Inspect debug/release logs for:

- database key material;
- object encryption keys;
- API keys/provider secrets;
- full private document contents where not explicitly diagnostic;
- unsafe internal filesystem disclosure in user-facing errors.

None should leak.

## Storage at rest

- SQLCipher DB does not expose plain SQLite header/content according to security design;
- object-store bytes are encrypted, not plaintext originals;
- plaintext staging cleaned/recovered;
- wrong key fails closed;
- integrity-corrupt object is not read as valid.

## Scope/auth

- cross-workspace access denied;
- stale/invalid command scopes rejected;
- idempotency replay conflicts do not duplicate mutations.

---

# 19. Process/lifecycle stress

Test:

- rotate during startup;
- rotate during generation;
- background/foreground during long operation;
- Activity recreation;
- force-stop/relaunch;
- process kill from developer options/ADB;
- low-memory process recreation where reproducible.

Assertions:

- no duplicate native runtime/workers;
- no duplicate external picker launches;
- no duplicate command submission;
- durable state restores;
- transient state reconciles truthfully;
- app does not depend on `Application.onTerminate()` for correctness.

---

# 20. Event-stream robustness

Automated tests should cover:

- duplicate event;
- out-of-order event within invalid sequence;
- sequence gap;
- runtime session ID changes;
- unknown forward event;
- oversized/invalid event payload rejection;
- terminal event arriving near cancellation.

Expected:

- duplicates ignored;
- gaps trigger snapshot/recovery;
- stale session events rejected;
- UI never enters contradictory states.

---

# 21. Performance acceptance

Performance thresholds should be measured and locked over time; v0.1 initially requires qualitative gates plus baseline collection.

Measure:

- cold-start time to shell/readiness;
- time to Home after backend ready;
- conversation scroll jank;
- large chat rendering;
- Storage list paging;
- memory during local inference/model load;
- APK/native library size;
- import throughput for supported file sizes.

## Qualitative blockers

- repeated main-thread JNI blocking;
- ANR during startup/import/model action;
- visible frame stalls from JSON parsing/event projection;
- unbounded all-files/all-chats load on screen open.

---

# 22. Migration/database acceptance

For every schema migration:

- clean database applies all migrations;
- previous supported schema upgrades successfully;
- migration checksums/order valid;
- failure is atomic/recoverable according to migration design;
- encrypted pre-migration backup behavior tested where implemented;
- schema-too-new fails safely;
- legacy compatibility repair has explicit tests.

Storage migrations additionally test scope/isolation triggers.

---

# 23. Destructive-action acceptance

For chat/project/workspace/file/model delete:

- confirmation names target;
- scope/cascade described;
- Cancel leaves state unchanged;
- repeated confirmation cannot double-delete incorrectly;
- exported external copies are not falsely claimed deleted;
- trash vs permanent delete behavior correct.

---

# 24. Release candidate checklist

A release candidate may be distributed only when:

```text
[ ] Kotlin tests/lint green
[ ] Rust/security matrix green
[ ] Native dual-ABI build green
[ ] R8 release APK build green
[ ] Required native DT_NEEDED libraries packaged
[ ] APK alignment verified
[ ] Official apksigner verification passes
[ ] ARM64 APK installs on physical device
[ ] Cold start reaches product shell
[ ] Relaunch reaches product shell with existing encrypted data
[ ] Core milestone user flow passes
[ ] No blocker crash/ANR
[ ] Accessibility smoke passes
[ ] Privacy/log smoke passes
[ ] Known limitations documented
```

For builds with model artifacts intentionally absent, acceptance may end at truthful `model required` state as long as the shell/backend/storage remain usable.

---

# 25. Regression tests from discovered failures

The following real failures become permanent regression requirements.

## R-001 — Database key bootstrap must not depend on premature JNI load

Test verifies key generation/wrapping path can execute before native runtime creation and produces a valid 32-byte key lifecycle according to security design.

## R-002 — Native transitive dependency packaging

CI verifies `libc++_shared.so` is packaged for each ABI when `libmukei_android.so`/dependency graph requires it.

## R-003 — Official APK signature verification

No custom/unverified signing implementation may be labeled installable. Distribution job must use official Android SDK `apksigner` and verify output.

## R-004 — Model artifacts missing is not backend failure

With secure backend/storage healthy but no inference artifacts, app opens usable shell and routes model-dependent actions appropriately.

## R-005 — Stable diagnostic specificity

Distinct bootstrap failures must not all collapse irretrievably into one generic code. Stable machine diagnostics remain available while user copy stays humane.

---

# 26. Milestone exit criteria

## M1 — App shell

- real signed APK installs;
- cold/relaunch stable;
- Home/drawer/navigation accepted;
- backend/model readiness separated;
- accessibility smoke.

## M2 — Conversation

- send/stop/stream/recovery end-to-end;
- process-death reconciliation;
- no duplicate commands;
- model unavailable routing.

## M3 — Storage

- encrypted import/list/open basic flow;
- failure/recovery matrix;
- scope isolation;
- indexing separation.

## M4 — Workspace

- structured files visible;
- partial failure preservation;
- isolated scopes;
- workspace recovery.

## M5 — Artifacts

- generate/retrieve/export/re-export;
- external failure preserves internal artifact.

## M6–M8

Each feature must satisfy its domain-specific sections plus all global install/start/security/accessibility gates.

---

# 27. Evidence requirements

Acceptance results should retain useful evidence:

- CI run URL/ID;
- commit SHA;
- APK SHA-256;
- signing certificate fingerprint for test/release channel;
- device/API/ABI;
- screenshots for major flows where useful;
- logcat excerpt only when redacted/necessary;
- failure reproduction steps;
- test result report.

A screenshot alone does not prove backend correctness, but it is useful evidence when paired with state/log/test assertions.

---

# 28. Definition of Done

A user-facing feature is Done only when:

1. product flow/spec is implemented;
2. state model is explicit;
3. backend contract is authoritative and bounded;
4. security/privacy invariants hold;
5. automated tests cover critical transitions;
6. assembled APK passes packaging/signature checks;
7. feature passes representative emulator/device flow;
8. process restart/failure behavior is truthful;
9. accessibility requirements pass;
10. documentation/known limitations are updated.
