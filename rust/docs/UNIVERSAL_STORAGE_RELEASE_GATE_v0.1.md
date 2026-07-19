# Universal Storage v0.1 — Release Gate

Status: **candidate-gate contract**  
Branch: `temp/universal-storage-workspace-v0.1`

A candidate is releasable only when every automated hard gate is green on the **same immutable commit SHA** and every manual acceptance item is recorded against that SHA. A newer push invalidates prior gate evidence.

## 1. Automated hard gates

### A. `storage-release-gate / release-gate`

Run `.github/workflows/storage-release-gate.yml` on the exact candidate SHA. It must pass all of the following:

- canonical V015 bytes are frozen at Git blob `9804b7f325ac49c86fe0799f3f8d0bddc4cac57f`;
- V016 is present and declares `PRAGMA user_version = 16`;
- `cargo fmt --all -- --check`;
- embedded migrations are contiguous and include V016;
- canonical V015 database upgrades by applying V016 only;
- repeated boot after V016 is a migration no-op;
- V016 adversarial identity/recovery/isolation guards pass;
- full `mukei-core` test suite with `rusqlite` passes;
- Clippy passes with warnings denied.

The reproducible local entry point is:

```bash
bash scripts/storage-release-gate.sh
```

### B. Existing branch validation

The exact same candidate SHA must also have these existing repository checks green:

- `storage-branch-validation`;
- `full-rust-workspace`.

These remain required because they cover surfaces outside the focused storage release runner, including broader workspace and Android/JNI validation configured by the repository CI.

## 2. Migration safety gate

Before release:

- V001–V015 are historical and must not be edited.
- New schema/invariant changes must use V017+.
- A database whose migration ledger ends at V015 must upgrade to V016 without reset, reinstall, or destructive schema rebuild.
- A V016 database must reopen with no pending migration.
- Migration checksum/order mismatch must remain fail-closed.
- Pre-migration backup creation must remain enabled for persistent upgrade paths.

Any historical migration-byte change is an automatic **NO-GO**.

## 3. Storage integrity gate

The candidate must preserve these invariants:

- workspace/scope/node identities cannot be rebound in place;
- import authorization targets cannot be silently retargeted;
- import progress cannot regress;
- terminal imports cannot be resurrected by stale workers;
- immutable object identity metadata cannot be rewritten;
- file-version lineage remains append-only;
- operation-journal node/transaction evidence cannot cross scopes;
- terminal journal evidence cannot be rewritten;
- corrupted/deduplicated objects fail closed rather than being silently accepted.

## 4. Crash/recovery acceptance

Run targeted fault/restart scenarios against the candidate build:

1. process termination during staged copy;
2. process termination after object publication but before logical-node commit;
3. process termination during node commit/indexing;
4. restart with `cancel_requested` import;
5. restart with incomplete journal evidence;
6. repeated recovery invocation.

Acceptance criteria:

- no cross-workspace visibility;
- no duplicate logical publication caused by replay;
- no backwards progress;
- no terminal-state resurrection;
- deterministic recovery queue behavior;
- orphaned temporary/staging material is recoverable or cleanable without exposing it as committed user data.

## 5. Android/device boundary

Before promoting the storage foundation into a production Android release:

- build the release candidate through the repository's Android release-hardening pipeline;
- validate the native/JNI library packaging expected by the production branch;
- install an officially signed/test-signed ARM64 candidate on a physical device;
- verify encrypted DB bootstrap/open, workspace bootstrap, import, reopen-after-process-death, and readback;
- verify insufficient-storage, cancelled-picker/import, malformed/unsupported file, and corrupted-object failure paths;
- record APK/artifact SHA-256 and source commit SHA together.

A compile-only Android result is not sufficient for release acceptance.

## 6. Release evidence packet

Record, for one immutable candidate SHA:

- source commit SHA;
- `storage-release-gate / release-gate` run URL/result;
- `storage-branch-validation` run URL/result;
- `full-rust-workspace` run URL/result;
- Android release-hardening run/artifact identifier;
- signed APK SHA-256 for device acceptance;
- V015→V016 upgrade test result;
- crash/recovery acceptance result;
- known limitations and explicit deferred risks.

## 7. GO / NO-GO rule

**GO** only when all hard automated gates are green on the same SHA, migration history is immutable, upgrade/recovery acceptance passes, and required device evidence is attached.

**NO-GO** for any of the following:

- historical migration mutation;
- failed or missing required CI status;
- migration requiring database reset/reinstall;
- integrity/isolation regression;
- recovery that can duplicate, rebind, or expose partially committed data;
- release artifact not traceable to the accepted source SHA.
