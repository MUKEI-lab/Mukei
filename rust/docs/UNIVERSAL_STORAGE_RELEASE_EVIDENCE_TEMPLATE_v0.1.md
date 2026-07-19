# Universal Storage v0.1 — Release Evidence

Use one copy of this template per immutable release candidate. Do not combine evidence from different source SHAs.

## Candidate identity

- Candidate source SHA:
- Promotion mode: `branch-native` / `production-integration`
- Intended production base SHA, if applicable:
- UTC gate start:
- Reviewer/owner:

## Automated gates

| Gate | Required | Result | Run URL | SHA matches candidate |
|---|---:|---|---|---:|
| `storage-release-gate` | Yes |  |  |  |
| `storage-branch-validation` | Yes |  |  |  |
| `full-rust-workspace` | Yes |  |  |  |
| Android release-hardening | Before production promotion |  |  |  |

## Migration evidence

- V015 Git blob: `9804b7f325ac49c86fe0799f3f8d0bddc4cac57f`
- V015 unchanged: Yes / No
- V015→V016 upgrade test: Pass / Fail
- Only V016 applied to canonical V015 DB: Yes / No
- Reopen at V016 is no-op: Yes / No
- `PRAGMA user_version`: expected `16`, observed:
- Migration checksum/order mismatch remains fail-closed: Yes / No
- Pre-migration backup path exercised: Yes / No / Not applicable

## Crash/recovery evidence

| Scenario | Result | Notes/evidence |
|---|---|---|
| Kill during staged copy |  |  |
| Kill after object publication, before node commit |  |  |
| Kill during node commit/indexing |  |  |
| Restart with `cancel_requested` |  |  |
| Restart with incomplete journal evidence |  |  |
| Repeat recovery invocation |  |  |

Required observations:

- Cross-workspace visibility observed: Yes / **No required**
- Duplicate logical publication observed: Yes / **No required**
- Backwards progress observed: Yes / **No required**
- Terminal-state resurrection observed: Yes / **No required**
- Partially committed data exposed as committed user data: Yes / **No required**

## Android/device evidence

- Android release-hardening run:
- Artifact identifier:
- ARM64 APK SHA-256:
- Signing mode/identity:
- Physical device model / Android version:
- Encrypted DB bootstrap/open: Pass / Fail
- Workspace bootstrap: Pass / Fail
- Import + readback: Pass / Fail
- Process-death reopen/recovery: Pass / Fail
- Insufficient-storage path: Pass / Fail
- Cancelled picker/import path: Pass / Fail
- Unsupported/malformed file path: Pass / Fail
- Corrupted-object failure path: Pass / Fail

## Promotion topology evidence

- Ahead/behind state recalculated immediately before promotion: Yes / No
- Selective-port review required: Yes / No
- Runtime/JNI conflicts reviewed: Yes / No / Not applicable
- Migration registry conflicts reviewed: Yes / No / Not applicable
- Cargo lock/toolchain conflicts reviewed: Yes / No / Not applicable
- Android CI/build conflicts reviewed: Yes / No / Not applicable
- Final integration SHA reran all required gates: Yes / No / Not applicable

## Known limitations / deferred risks

- 

## Final decision

- Decision: `GO` / `NO-GO`
- Decision SHA:
- Decision UTC:
- Approved by:
- Reason:
