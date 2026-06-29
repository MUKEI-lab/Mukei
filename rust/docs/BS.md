# MUKEI — Backend Schema Document (BS) — v1.2 (companion to TRD v0.7.5)

| Field | Value |
|-------|-------|
| **Document ID** | MUKEI-BS-v1.2 |
| **Supersedes** | BS v1.0 (2026-06-19, first pass) · BS v1.1 (2026-06-19, v0.7.4 i18n schema) |
| **Status** | 🟢 AI-Architect Pass — Cross-Locked against PRD v0.7.5 + TRD v0.7.5 + AF v1.2 + UXB v2.1 |
| **Audience** | Database / Rust engineers, Security review, Forensic engineers |
| **Companion docs** | [PRD v0.7.5](PRD.md) · [TRD v0.7.5](TRD.md) · [Application Flow v1.2](AF.md) · [UI/UX Brief v2.1](UXB.md) |
| **Out of scope** | UI behaviour — see [UI/UX Brief v2.1](UXB.md) |
| **Notation** | Diagrams use ASCII. SQL is the current schema; **never** edit by hand — only via migrations (TRD §6). |

> **Hard rule:** No schema change without a `V0xx__name.sql` migration in `migrations/` and an entry in `migrations_applied`. Direct edits are checked by a pre-commit test (TRD §11.1 `test_no_direct_schema_edit`).

---

## Table of Contents

1.  [Document Conventions](#1-document-conventions)
2.  [SQLCipher Database — Top-Level](#2-sqlcipher-database--top-level)
3.  [Per-Table Specifications](#3-per-table-specifications)
4.  [Vector Store (usearch HNSW)](#4-vector-store-usearch-hnsw)
5.  [Migrations System](#5-migrations-system)
6.  [Key, Blob, and Side-Storage Layout](#6-key-blob-and-side-storage-layout)
7.  [Configuration Schema (`config.toml`)](#7-configuration-schema-configtoml)
8.  [Encryption Boundaries](#8-encryption-boundaries)
9.  [Indexes & Query Plans](#9-indexes--query-plans)
10. [Concurrency Model](#10-concurrency-model)
11. [Backup / Restore / Wipe](#11-backup--restore--wipe)
12. [Retention, Eviction, Vacuum](#12-retention-eviction-vacuum)
13. [FFI Surface Schema](#13-ffi-surface-schema)
14. [On-Disk Layout Map](#14-on-disk-layout-map)
15. [Privacy-Boundary Map](#15-privacy-boundary-map)
16. [Type Codes / Hash Format](#16-type-codes--hash-format)
17. [Test & Regression Hooks](#17-test--regression-hooks)
18. [Revision History](#18-revision-history)

---

## 1. Document Conventions

### 1.1 Column Type Conventions

| Type | Meaning |
|------|---------|
| `TEXT` | UTF-8 string (NULL = absent) |
| `TEXT NOT NULL` | enforced at schema level, validated at app |
| `INTEGER` | signed 64-bit (SQLite standard) |
| `INTEGER NOT NULL` | time-sortable 64-bit row identity (see §3.1.1) |
| `BLOB` | raw bytes (PRAGMA key doesn't leak length) |
| `BLOB NOT NULL` | opaque, never logged |

### 1.2 Naming

- Tables `snake_case`, plural where appropriate.
- Columns `snake_case`.
- Indexes `idx_<table>_<composite or single column>`.
- Migrations `V<NUM>__<slug>.sql` where NUM is zero-padded to 3 digits.

---

## 2. SQLCipher Database — Top-Level

### 2.1 Encryption

- Cipher: **SQLCipher 4** (AES-256-CBC, HMAC-SHA512).
- Key: 32 random bytes from `OsRng`, generated in Rust, wrapped by Android Keystore (AES/GCM) at boot, stored as `db_key.enc` (TRD §12.3).
- Page size: 4096.
- Journal mode: `WAL` (write-ahead-log; concurrent reads fine).
- Synchronous: `FULL`.

(PRD REQ-SEC-19; REQ-DB-01..06.)

### 2.2 File Location

```
/data/data/com.mukei.app/
├── files/
│   ├── mukei.db            # SQLCipher container
│   ├── mukei.db-wal        # WAL
│   ├── mukei.db-shm        # shared memory
│   ├── db_key.enc          # Wrapping-key ciphertext (IV ‖ CT)
│   ├── brave_key.enc       # Wrapped Brave API key (optional)
│   ├── hnsw.bin            # Vector store snapshot
│   ├── hnsw.bin.tmp        # Atomic-rename intermediate
│   └── crash.dump          # last Rust panic, scrubbed of secrets
└── cache/
    ├── user-files/<uuid>/  # SAF-tokenized paths (canonicalized)
    └── models/.part/.meta  # download state (TRD §5.3)
```

### 2.3 Page-Level Privacy

The DB file is opaque; no metadata (table names, indexes, free pages) is recoverable without the key. `db_key.enc` is encrypted; only Keystore can decrypt it.

### 2.4 PRAGMA Inventory

```
PRAGMA key = "x'<hex_key>'";
PRAGMA cipher_page_size = 4096;
PRAGMA kdf_iter = 256000;
PRAGMA cipher_hmac_algorithm = HMAC_SHA512;
PRAGMA cipher_kdf_algorithm = PBKDF2_HMAC_SHA512;
PRAGMA journal_mode = WAL;
PRAGMA synchronous = FULL;
PRAGMA foreign_keys = ON;
PRAGMA temp_store = MEMORY;
```

`user_version` is queried on boot to validate against expected schema version.

---

## 3. Per-Table Specifications

### 3.1 `conversations` (BS.md §2.1)

| Column | Type | Notes |
|--------|------|-------|
| id | INTEGER PK | time-sortable 64-bit conversation id |
| title | TEXT NULL | user-set or auto-derived from first 6 words |
| created_at | TEXT NOT NULL | ISO-8601 UTC ms |
| updated_at | TEXT NOT NULL | updated on any child write |
| archived | INTEGER NOT NULL DEFAULT 0 | 0/1 (avoid BOOLEAN portability) |
| schema_version | INTEGER NOT NULL | Bumped per message-mutating migration |

Indexes:
- `idx_conversations_updated_at` `(updated_at DESC)` — list view ordering.
- `idx_conversations_archived` `(archived)` partial WHERE `archived = 0`.

Invariants:
- Every conversation has ≥ 1 branch row (created lazily).

### 3.1.1 ID Format (Time-Sortable 64-bit)

```
1849453561827713025
└── high bits: millisecond timestamp
    └── low bits: per-process sequence / entropy
```

The documentation sometimes used “ULID-derived” as shorthand for *time-sortable*. The persisted SQLite primary keys in this schema are **INTEGER** ids, not 26-character ULID strings. They are generated in Rust as monotonic 64-bit identifiers and inserted directly into the table's `id` column.

### 3.2 `messages`

| Column | Type | Notes |
|--------|------|-------|
| id | INTEGER PK | time-sortable 64-bit message id |
| conversation_id | INTEGER NOT NULL | FK conversations(id) |
| branch_id | INTEGER NOT NULL | FK branches(id) |
| parent_message_id | INTEGER NULL | nullable for root |
| role | TEXT NOT NULL | one of: `user`, `assistant`, `tool`, `system` |
| content | TEXT NOT NULL | full message text (validated UTF-8) |
| content_format | TEXT NOT NULL DEFAULT 'markdown' | future-proofing |
| thinking | TEXT NULL | `<thinking>` block if any |
| tool_call_id | TEXT NULL | if `role=tool` |
| token_count | INTEGER NULL | reconciled at finalize |
| state | TEXT NOT NULL | enum: `Draft|Sending|Streaming|Finalized|Aborted|Errored` |
| generation | INTEGER NULL | FFI generation guard id |
| created_at | TEXT NOT NULL | inserted at first token |
| finalized_at | TEXT NULL | set when state → Finalized |

Indexes:
- `idx_messages_branch` `(conversation_id, branch_id, id)`.
- `idx_messages_parent` `(parent_message_id)` (gives O(1) child lookup).
- `idx_messages_state` `(state)` partial WHERE `state NOT IN ('Finalized','Terminal')`.

Invariants:
- Exactly one `parent_message_id` per row. NULL only for the very first message of a branch.
- `state` mutates within the enum; transitions validated (see AF §8.1).

### 3.3 `branches`

| Column | Type | Notes |
|--------|------|-------|
| id | INTEGER PK | time-sortable 64-bit branch id |
| conversation_id | INTEGER NOT NULL | FK |
| root_message_id | INTEGER NOT NULL | where this branch diverged |
| name | TEXT NULL | user-given or auto "Branch #n" |
| created_at | TEXT NOT NULL |
| is_default | INTEGER NOT NULL DEFAULT 0 |

Invariants:
- Per conversation, exactly ONE branch row with `is_default=1`. Enforced by partial unique index: `CREATE UNIQUE INDEX uniq_default_branch ON branches(conversation_id) WHERE is_default=1`.
- A branch has at least one message rooting it; empty branches deleted on cleanup.

### 3.4 `tool_audit_log`

| Column | Type | Notes |
|--------|------|-------|
| id | INTEGER PK | time-sortable 64-bit id |
| message_id | INTEGER NULL | parent LLM message |
| tool_name | TEXT NOT NULL | one of validator's allow-list |
| tool_args | TEXT NOT NULL | JSON (after schema validator) |
| tool_result | BLOB NULL | success (raw bytes) |
| error_code | INTEGER NULL | 3xx range |
| error_message | TEXT NULL | sanitized |
| duration_ms | INTEGER NOT NULL | perf signal |
| fingerprint | TEXT NOT NULL | sort-canonical JSON SHA-256 |
| created_at | TEXT NOT NULL |

Index:
- `idx_tool_audit_message` `(message_id)`.

Privacy:
- `tool_args` may include SAF tokens. **Never** persisted in plaintext format; always occurrences are recorded as JSON-encoded `saf://` opaque tokens (the canonical path is logged separately in `saf_tokens`).
- `tool_result` redaction: file/image results truncated at MAX (1 MB) **before** insert.

### 3.5 `chunks` (RAG)

| Column | Type | Notes |
|--------|------|-------|
| id | INTEGER PK | time-sortable 64-bit chunk id |
| source_id | INTEGER NOT NULL | FK saf_tokens(id) |
| seq | INTEGER NOT NULL | ordinal within source file |
| content | TEXT NOT NULL | the chunk text |
| byte_offset | INTEGER NOT NULL | start position in source |
| byte_length | INTEGER NOT NULL | |
| content_hash | TEXT NOT NULL | SHA-256 hex of `content` (lower-case) |
| created_at | TEXT NOT NULL |

Indexes:
- `idx_chunks_source` `(source_id, seq)`.
- `idx_chunks_hash` `(content_hash)` (deduplication).

Privacy:
- A chunk is derived from SAF-acquired data only. It is never network-derived.

### 3.6 `saf_tokens` (Persistent SAF grants, TRD §5.4)

| Column | Type | Notes |
|--------|------|-------|
| id | INTEGER PK | time-sortable 64-bit id |
| saf_token | TEXT NOT NULL UNIQUE | opaque `saf://` URI |
| resolved_path | TEXT NOT NULL | canonical /data/data/com.mukei.app/cache/user-files/<uuid> |
| display_name | TEXT NULL | original file/dir name for UI |
| mime_type | TEXT NULL | resolved MIME |
| last_used_at | TEXT NOT NULL | updated on every read |
| granted_at | TEXT NOT NULL |
| revoked | INTEGER NOT NULL DEFAULT 0 | Soft delete; revoke leaves audit trail |

Indexes:
- `uniq_saf_token` UNIQUE `(saf_token)`.
- `idx_saf_revoked` `(revoked)` partial WHERE revoked=0.

Privacy:
- `saf_tokens` is NOT a path leakage: the resolved_path is internal / data/data path; spec forbids ever logging it.

### 3.7 `model_state`

| Column | Type | Notes |
|--------|------|-------|
| id | INTEGER PK | always 1 (single row config) |
| model_path | TEXT NOT NULL | absolute path under `files/` |
| model_size_bytes | INTEGER NOT NULL |
| model_sha256 | TEXT NOT NULL | hex / lower-case |
| model_url | TEXT NOT NULL | origin URL |
| context_size | INTEGER NOT NULL | n_ctx |
| gpu_layers | INTEGER NOT NULL | 0 = CPU-only |
| loaded_at | TEXT NULL | set when LLamaLoad succeeded |
| last_used_at | TEXT NULL | |

Invariants:
- `id = 1` (singleton).
- Exactly one row at all times. Enforced by `CHECK (id = 1)`.

### 3.8 `recovery_state` (Crash counter, TRD §36.1)

| Column | Type | Notes |
|--------|------|-------|
| id | INTEGER PK | always 1 |
| crash_counter | INTEGER NOT NULL DEFAULT 0 |
| last_crash_at | TEXT NULL |
| last_crash_class | TEXT NULL | `OOM|FFI|JVM|RUST_PANIC|UNKNOWN` |
| first_seen_version | TEXT NOT NULL | "0.7.x" |

Invariants:
- `id = 1`.
- Atomic rename on every write (CRASH-SAFE RENAMING strategy — see §6.4).

### 3.9 `app_settings`

A loose key-value store for non-secret UI prefs.

| Column | Type | Notes |
|--------|------|-------|
| key | TEXT PK | e.g. `theme`, `temperature_default`, `max_tokens_default`, `network_online`, `telemetry_enabled` |
| value | TEXT NOT NULL | JSON or scalar |
| updated_at | TEXT NOT NULL |

Invariants:
- `telemetry_enabled` MUST be `false` on insert (release-build invariant). Hot path validates this.

### 3.10 `migrations_applied` (TRD §6.1)

| Column | Type | Notes |
|--------|------|-------|
| version | INTEGER PK | matches `<VNUM>__name.sql` from `migrations/` |
| name | TEXT NOT NULL | human-readable |
| applied_at | TEXT NOT NULL | ISO-8601 UTC |

Index:
- The PK is the natural ordering; no extra index needed.

### 3.11 `lifecycle_state` (Optional AF §15)

| Column | Type | Notes |
|--------|------|-------|
| event | TEXT PK | e.g. `last_pause_at`, `last_resume_at`, `last_trim_level` |
| value | TEXT NOT NULL | serialized JSON |
| recorded_at | TEXT NOT NULL |

Used for crash-window context (e.g. crash right after a TRIM_MEMORY_COMPLETE).

---

## 4. Vector Store (usearch HNSW)

### 4.1 Why On-Disk (Not in SQLite)

HNSW requires random-access mmap on a contiguous file; SQLite's BLOB strategy causes fragmentation. usearch owns its own file format, kept alongside SQLCipher.

### 4.2 File Layout (TRD §4.2)

```
hnsw.bin     # canonical snapshot
hnsw.bin.tmp # writer builds here; fsync; rename
hnsw.bin.lock# flock('$hnsw.bin') ensures single writer
```

### 4.3 Schema (Embedded in HNSW header)

| Offset | Bytes | Field | Notes |
|--------|-------|-------|-------|
| 0 | 8 | `MAGIC = b"MUKEIVEC"` | 8-byte constant |
| 8 | 8 | version (u64 LE) | bumped per format change |
| 16 | 8 | dim (u32 LE) | 384 |
| 24 | 8 | count (u64 LE) | number of vectors |
| 32 | 8 | reserved | 0 |

Vectors follow as `count * dim * sizeof(f32)` little-endian bytes.

### 4.4 Index ↔ SQL Sync

| SQLite event | HNSW action |
|--------------|-------------|
| `chunks` row insert | `usearch::Index::add(<chunk_id, vec>)` |
| `chunks` row delete | `usearch::Index::remove(<chunk_id>)` |
| HNSW schema mismatch on boot | user prompted: rebuild or skip |

### 4.5 Failure Recovery

If `hnsw.bin` fails to load or checksum mismatch:
- Trigger `RagRebuildPrompt` (UXB §4.6).
- Or drop index and re-derive from `chunks`.

---

## 5. Migrations System

### 5.1 Directory

```
rust/migrations/
├── V001__schema.sql
├── V002__add_branches.sql
├── V003__add_tool_audit_log.sql
├── V004__add_lifecycle_state.sql
├── V005__add_saf_tokens_persistable.sql
└── ...
```

### 5.2 Loader

```
expected_version = MAX(version) on disk
db_version = PRAGMA user_version

if db_version < expected_version:
    for migration in (db_version + 1)..=expected_version:
        run(migration)  -- in transaction
        INSERT INTO migrations_applied (version, name, applied_at)
    PRAGMA user_version = expected_version
```

(TRD §6.1; PRD REQ-DB-04.)

### 5.3 Rules

- Migrations are append-only. **Never** edit a `Vxxx__*.sql` once committed.
- Migrations MUST be idempotent or run-once. The runtime guards by checking `migrations_applied`.
- Transactional: each one rolls forward; a failure reverts and aborts boot (SafeModeDB).
- Hot-reload is not supported — boot only.

### 5.4 Test

`test_migrations_tracked_after_run`: assert `PRAGMA user_version == MAX(version)` AND `migrations_applied` rows match.

---

## 6. Key, Blob, and Side-Storage Layout

### 6.1 `db_key.enc` (TRD §12.3)

```
struct WrappedKey { iv: [u8;12], ct: Vec<u8> }
fs::write(db_key.enc, &bincode::serialize(&wrapped_key)?)
```

- 12-byte IV (GCM nonce).
- CT length = 32 (raw key) + 16 (GCM tag) = 48 bytes minimum.
- Total file size: 60 bytes.

### 6.2 `brave_key.enc` (TRD §12.4)

Same shape but holds the Brave API key, opt-in (`config.toml.brave_key_blob`).

### 6.3 `.part` and `.meta` (TRD §5.3)

```
files/models/<id>.gguf.part     # in-progress bytes
files/models/<id>.gguf.meta     # JSON sidecar
```

`.meta` schema:

| Field | Type | Notes |
|-------|------|-------|
| expected_sha256 | TEXT | hex lower-case |
| total_bytes | INTEGER | content-length from HTTP HEAD |
| received_bytes | INTEGER | updated atomically |
| etag | TEXT NULL | for HTTP 304 |
| url | TEXT NOT NULL | origin |
| started_at | TEXT NOT NULL |

Atomic update uses `mv <meta>.tmp <meta>` after fsync.

### 6.4 Crash-Safe Renaming

All on-disk files in `files/` follow the rule:

```
write to `<path>.tmp`
fsync file
atomic rename(tmp, final_path)
fsync parent dir
```

This applies to `db_key.enc`, `brave_key.enc`, `hnsw.bin`, `.meta`, `crash.dump`, etc. TRD §36.1, §5.3.

---

## 7. Configuration Schema (`config.toml`)

> **Authoritative source**: `rust/crates/mukei-core/src/config/mod.rs`
> + the on-disk seed `rust/migrations/000_default_config.toml`.
> The schema documented here is the strict-validator surface enforced
> at boot.

### 7.1 Location

`files/config.toml` on Android (app-private storage); `~/.config/mukei/config.toml`
or an explicit override on desktop. Loaded at boot via
`MukeiConfig::load_and_validate(&Path)` (TRD §12.5). The validator is
**strict**: any top-level key not on `MukeiConfig::known_keys()` is
rejected with `MukeiError::ConfigUnknownField` and boot refuses to
start.

### 7.2 Strict TOML schema

All required (non-`#[serde(default)]`) fields must be present. The
listed keys are the canonical names; nested tables use the standard
TOML `[table]` / `[[array_of_tables]]` syntax.

```toml
models_dir         = "/var/mukei/models"
vectors_dir        = "/var/mukei/vectors"
database_path      = "/var/mukei/db/mukei.db"
saf_tokens_db      = "/var/mukei/db/saf_tokens.db"
crashes_dir        = "/var/mukei/crashes"
logs_dir           = "/var/mukei/logs"

gpu_layers         = 32     # i32, ≥ 0 (0 = CPU-only)
n_ctx              = 4096   # u32, range [256, 32768]
n_threads          = 4      # u32, range [1, 32]

[max_blocking]
max_blocking_threads_android = 6   # §2.2 — bounded Android pool
max_blocking_threads_desktop = 8   # desktop / CI
tool_slots                   = 2   # TOOL_BLOCKING_SLOTS

[watchdog]
max_iterations     = 8       # ≥ 1 (REQ-AGT-04)
max_token_budget   = 8192    # u64
max_wall_seconds   = 600     # u64

[storage]
sqlcipher_kdf_iter        = 256000
wal_autocheckpoint_pages  = 1000

[saf]
persist_grants_to_db = true
prompt_on_first_use  = true

[agent]
max_failures_per_tool       = 5     # threshold per architect review GH #14
recovered_history_window    = 12
repeat_output_window        = 2     # default if omitted
repeat_output_backoff_secs  = 10    # default if omitted
max_concurrent_tools        = 4     # tokio::spawn semaphore (GH #13)

[defaults]
prompt_card_auto_send = false
thermal_autopause     = true
first_run_completed   = false

[search]                     # optional; defaults below (GH #34)
brave_timeout_secs   = 3
tavily_timeout_secs  = 5
max_parallel_engines = 2
enable_cache         = true

[[wrapped_secrets]]          # optional, zero or more entries
slot       = "brave_api_key"
blob_path  = "/var/mukei/secrets/brave_key.enc"
created    = 2026-06-29T00:00:00Z
```

### 7.3 Validator semantics

`MukeiConfig::load_and_validate` runs a **two-pass** check:

1. **`validate_toml_keys`** — every root key MUST be in
   `MukeiConfig::known_keys()`. Unknown root keys yield
   `ConfigUnknownField(<key>)` and boot halts. Unknown nested keys
   are caught by `serde`'s `deny_unknown_fields` posture (every
   struct in the schema is strict).
2. **`logical_validate`** — typed range checks:
   - `gpu_layers ≥ 0`
   - `n_ctx ∈ [256, 32768]`
   - `n_threads ∈ [1, 32]`
   - `watchdog.max_iterations ≥ 1`

Failure is surfaced as `MukeiError::ConfigInvalid { field, reason }`;
the bridge crate renders both fields verbatim in the QML error
dialog so a first-run misconfig is human-readable.

The agent runtime wires `AgentCfg` → `ToolExecutionPolicy` via the
`From<&AgentCfg>` impl so the on-disk settings are not cosmetic
(Issue #13 fix). Adding a new field to `AgentCfg` requires updating
the conversion AND the `config_round_trips_into_policy` regression
test.

### 7.4 Defaults & forward compatibility

Three fields use `#[serde(default = …)]` so v0.7.4 configs that
predate them still load:

- `repeat_output_window` defaults to `2`
- `repeat_output_backoff_secs` defaults to `10`
- `max_concurrent_tools` defaults to `4`
- `[search]` is entirely defaulted (Brave 3 s / Tavily 5 s / parallel 2 / cache on)
- `[[wrapped_secrets]]` is defaulted to an empty list

No other field is defaulted — the strict-config posture is
deliberate so silent regressions cannot accumulate.

### 7.4 i18n String Storage 🌐 (NEW in v0.7.4)

> **🛡️ BUGFIX v0.7.4 — Localisation Source-of-Truth.** AF §6.5 references `i18n/web_search.en.json` (Brave-key toast strings) but v0.7.2 did not define a schema for that file. Without a formal contract, future toasts/banners drift into hard-coded strings in QML, breaking REQ-I18N-01 (every user-visible string is localisable) and making OEM region-pack overrides impossible.

**Storage model.** i18n is **flat-file**, *not* SQLite. The reason: locale switches must be atomic and survive DB corruption / Safe Mode boot. Strings live under:

```
/data/data/com.mukei.app/files/i18n/
    web_search.en.json
    web_search.hi.json
    web_search.fr.json
    …
    chat.en.json
    chat.hi.json
    …
```

**One file per (feature × locale).** Filenames are `{feature}.{bcp47_locale}.json`. Loaded lazily on first reference; cached in `OnceLock<HashMap<&'static str, String>>` per (feature, locale) pair in `rust/src/i18n/mod.rs`.

**JSON shape (strict, validated at boot):**

```json
{
  "$schema_version": 1,
  "locale": "en",
  "feature": "web_search",
  "strings": {
    "brave_key_missing_toast": "Brave API key missing — using DuckDuckGo only.",
    "brave_key_paste_invalid":  "Doesn't look like a Brave API key. Keys are 20–64 alphanumeric characters, with `-` or `_` allowed.",
    "brave_key_test_ok":        "Key works. Save?",
    "brave_key_test_rejected":  "Brave rejected this key (HTTP {http_status}). Double-check the dashboard.",
    "brave_key_test_rate_limit":"Brave rate-limited the test (HTTP 429). The key itself is fine — try Save anyway.",
    "brave_key_test_network":   "Couldn't reach Brave. Check connectivity. Save anyway?"
  }
}
```

**Schema rules (validator: `i18n::validate_file`):**

- `$schema_version` MUST equal `1`. Future-incompatible changes bump this AND ship a migration step.
- `locale` MUST be a valid **BCP-47** tag (regex `^[a-z]{2}(-[A-Z]{2})?$`).
- `feature` MUST equal the filename feature prefix (mismatch → boot halts to Safe Mode).
- `strings` is a `{string_key: string}` map; all values are UTF-8 strings; no nested objects, no arrays — keeps the lookup O(1) and lint-checkable.
- Placeholders use `{name}` syntax (NOT `printf`-style `%s`) so a missing placeholder is caught by the validator (`Regex(r"\{[a-z_]+\}")`).
- Unknown top-level keys are rejected.

**Fallback chain (lookup order):**

1. `i18n/{feature}.{user_locale}.json`
2. `i18n/{feature}.en.json` (mandatory — ship-blocker if absent)
3. The literal `string_key` itself (debug builds only; release boot halts).

**Encryption boundary.** i18n files are NOT encrypted (§8.2) — they are not secrets and are bundled in the APK assets, extracted on first launch (TRD §33.2.1). User-installed overrides land in `files/i18n/` and take precedence over APK assets (so OEM region packs work without an app update).

**FFI surface.** Rust exposes `i18n::t(feature: &str, key: &str, vars: &[(&str, &str)]) -> String`. QML reaches this via the agent bridge as `mukeiAgent.t("web_search", "brave_key_missing_toast")`. QML MUST NOT read the JSON files directly (REQ-UI-05 — keep filesystem I/O off the UI thread).

**Acceptance tests:**

- `test_i18n_validator_rejects_unknown_key`
- `test_i18n_validator_requires_schema_version_1`
- `test_i18n_fallback_to_english_when_locale_absent`
- `test_i18n_release_build_panics_on_missing_string_key`
- `test_i18n_placeholder_balance` — every `{name}` in EN must also exist in every translated file.

---

## 8. Encryption Boundaries

### 8.1 Encrypted At Rest

| Resource | Algorithm | Key |
|----------|-----------|-----|
| `mukei.db` (all tables) | SQLCipher 4 AES-256-CBC | 32 random |
| `db_key.enc` | AES-GCM, no padding | Keystore alias |
| `brave_key.enc` | AES-GCM | Keystore alias |
| `crash.dump` | binary-truncated, keys zeroized even before write | n/a |

### 8.2 NOT Encrypted (By Design)

| Resource | Why |
|----------|-----|
| `config.toml` | UI prefs only; non-secret (REQ-CON-01) |
| `hnsw.bin` | vectors derived from local files only; no secret |
| `.gguf` model files | public weights; SHA-256 verifies integrity |
| `i18n/*.json` | localisation strings, non-secret, OEM-overridable (§7.4) |
| Telemetry: never created | release-build invariant |

### 8.3 What Is NEVER Written To Disk

- Raw 32-byte DB key (lives in `Zeroizing` cursor only).
- User pastes that haven't yet been committed to a `messages` row (lives in QML).
- Token chunks during streaming until first-token commit.

### 8.4 Boundary Cross-Check

A pre-commit test (`test_no_plaintext_secret_in_repo`) greps the entire repo for any line containing `"key"` *and* `0x` *and* longer than 32 hex chars.

---

## 9. Indexes & Query Plans

### 9.1 Hot-Path Query Plans

| Query | Plan | Cost |
|-------|------|------|
| List conversations | `idx_conversations_updated_at DESC` | O(log n) per page |
| Get branch tail | `idx_messages_branch (conv, branch, id)` ORDER BY id DESC LIMIT 20 | O(log n) |
| Search children of `parent_id` | `idx_messages_parent` | O(1) per query |
| RAG top-k | usearch HNSW, k=8 | O(log n) |
| Tool audit replay by message | `idx_tool_audit_message` | O(log n) |

### 9.2 Cold / Maintenance Queries

- `VACUUM` (manual, after SafeMode reset only).
- `REINDEX` after schema migration.
- `pragma wal_checkpoint(TRUNCATE)` on graceful shutdown.

---

## 10. Concurrency Model

### 10.1 Writer Rule

> Exactly ONE writer at a time, period.

Writers:

- Rust background indexer (RAG embedder).
- QML/Flow action handlers (message lifecycle).

Locking strategy:

- A `Mutex<DbConn>` over the rusqlite connection.
- All writes go through a single async-st Tokio task (`db_writer`).
- QML never writes directly; it enqueues intent messages on an MPSC channel.

### 10.2 Lock Ordering

```
LockA: db_writer         (global)
LockB: hnsw.bin (file)   (single writer via flock)
LockC: flash lockfile     (prevent concurrent boot)
```

Acquired in `LockC → LockA → LockB` order. Reverse order forbidden.

### 10.3 Read Concurrency

Reads run from Tokio tasks using `tokio::sync::RwLock`. WAL allows concurrent readers.

### 10.4 FFI Generation Guard

(TRD §1.3.1, PRD REQ-ARCH-05.)

`generation: u64` on the Source QObject pins the token's owner. Late tokens = dropped at FFI edge.

### 10.5 FailureTracker Concurrency

A `FailureTrace` is `Send + Sync`. It does not contain locks but does have a `Vec<CallRecord>` updated atomically per attempt (REQ-AGT-04).

---

## 11. Backup / Restore / Wipe

### 11.1 Backup Policy (Local-Only)

- No cloud backup. No Google's Auto-Backup. SQLCipher keys prevent it anyway.
- Optional user-initiated export to SAF: full DB export as encrypted `.db` blob (user supplies passphrase or uses keystore-rewrap).

### 11.2 Restore

- User picks a `.db.enc` from SAF; verify SHA256; place under `files/`; boot tries it; PRAGMA key is re-required.

### 11.3 Wipe

- `SafeMode → Reset All Data` triggers SafeDataWipe flow.
- Atomic-delete of `files/*` and re-create empty `files/`.
- Crash counter = 0.

(REQ-LIFE-02.)

---

## 12. Retention, Eviction, Vacuum

### 12.1 Cache Quota

`cache_max_bytes` in config caps cache size. Eviction is least-recently-used first.

### 12.2 Conversation Soft-Delete

`conversations.archived = 1` instead of hard delete. Hard-delete runs every 30 days OR when user explicitly clears.

### 12.3 Vacuum Cycle

- Background-after-reset only.
- WAL checkpoint at graceful shutdown.

### 12.4 Orphan Cleanup

- Branches with zero messages and `is_default=0` removed on next cleanup.
- RAG chunks deleted when source file is revoked.

---

## 13. FFI Surface Schema

### 13.1 ABI-Exported Functions (CXX-Qt + c-FFI)

```
bool mukei_initialize(const char* config_path);
uint64_t mukei_send_message(char* user_input, void* context_ptr, void* guard, TokenCallback callback);
bool mukei_abort(uint64_t generation);
void mukei_pause(void);
void mukei_resume(void);
MukeiStatus mukei_get_status(void);
```

### 13.2 ABI Layout

```rust
#[repr(C)]
pub struct CallbackGuard { generation: u64 }     // 8 bytes
#[repr(C)]
pub struct MukeiStatus { state: u8, err: u16 }   // 4 bytes (rounded up)
```

(Tested by `test_callbackguard_layout_is_abi_stable` — TRD §11.1.)

### 13.3 Channel Names

| CXX-Qt channel | Direction | Type |
|----------------|-----------|------|
| `chunkGenerated` | Rust → QML | QString (50 ms diff-batch) |
| `streamFinalized` | Rust → QML | QString (status JSON) |
| `toolCallDetected` | Rust → QML | QString (tool-call JSON) |
| `errorOccurred` | Rust → QML | QString (error JSON) |
| `progressChanged` | Rust → QML | QString (progress/status JSON) |

(TRD §2.6.)

### 13.4 Lifecycle Pins

| Channel | Generation pinned? |
|---------|--------------------|
| `chunkGenerated` | yes (REQ-ARCH-05) |
| `streamFinalized` | yes |
| `toolCallDetected` | no (use-after-race still safe because QML reads final state only) |
| `errorOccurred` | no |
| `progressChanged` | yes (cross-cuts QML) |

---

## 14. On-Disk Layout Map

```
/data/data/com.mukei.app/
│
├── files/                                  # all main storage
│   ├── mukei.db                            # SQLCipher-encrypted, WAL-mode
│   ├── mukei.db-wal, mukei.db-shm
│   ├── db_key.enc                          # AES-GCM keystore-wrapped
│   ├── brave_key.enc                       # optional, same scheme
│   ├── hnsw.bin                            # usearch HNSW snapshot
│   ├── hnsw.bin.tmp                        # in-progress write
│   ├── crash.dump                          # last panic (sanitized)
│   ├── models/<id>.gguf                    # current
│   ├── models/<id>.gguf.part               # in-progress download
│   ├── models/<id>.gguf.meta               # sidecar (json)
│   └── config.toml                         # non-secret prefs
│
├── cache/                                  # may be reclaimed by Android
│   ├── user-files/<uuid>/                  # SAF-rendered copies
│   └── models/                             # legacy pre-v0.7.2 (now hot)
│
└── no_backup/                              # excluded from cloud auto-backup
    └── (empty by default — zero retention risk)
```

---

## 15. Privacy-Boundary Map

| Boundary | Direction | Mechanism |
|----------|-----------|-----------|
| UI ↔ Rust | bidirectional | FFI with generation guard |
| Tool → LLM | inbound | XML wrapper (`<external_data trust="untrusted">`) |
| RAG → LLM | inbound | XML wrapper |
| Web → LLM | inbound (HTTP only) | XML wrapper |
| DB disk | outbound | SQLCipher encrypts |
| Crash dump | outbound | keys zeroized first |
| Telemetry | outbound | hard-disabled in release builds |
| `config.toml` | outbound | only non-secret fields |

---

## 16. Type Codes / Hash Format

| Use | Algorithm | Hex? |
|-----|-----------|------|
| chunk content hash | SHA-256 | yes, lower-case |
| model file SHA (TRD §5.3) | SHA-256 | yes, lower-case |
| token generator seed | xoshiro256** | never exported |
| failure fingerprint | sort_canonical_json + SHA-256 | yes, lower-case |

Format invariant: hashes always 64 hex chars (lower-case). Tests check this in TRD §11.1.

---

## 17. Test & Regression Hooks

| Test | Asserts |
|------|---------|
| `test_no_direct_schema_edit` | every `CREATE TABLE` lives inside `migrations/V*.sql`. |
| `test_no_plaintext_secret_in_repo` | grep guard |
| `test_migrations_tracked_after_run` | `user_version` matches |
| `test_hnsw_header_invariant` | first 8 bytes = `MUKEIVEC` |
| `test_schema_version_on_conversation` | non-zero after migration pass |
| `test_msg_state_transitions_legal` | all enum transitions |
| `test_failure_fingerprint_is_canonical` | different key orders → same fingerprint |
| `test_no_raw_key_in_crash_dump` | dump does not contain 64 hex bytes |
| `test_kdf_iter` | PRAGMA kdf_iter = 256000 |
| `test_purge_after_reset` | `files/` cleared |

---

## 18. Revision History

| Date | Version | Author | Change |
|------|---------|--------|--------|
| 2026-06-19 | 1.0 | AI-Architect | First pass, cross-locked against PRD v0.7.2 + TRD v0.7.2. Full table-by-table specs with invariants, indexes, migrations, encryption layout, FFI ABI. |
| 2026-06-19 | 1.1 | AI-Architect | **v0.7.4 hardening:** added §7.4 (i18n String Storage — flat-file `i18n/{feature}.{locale}.json` schema, fallback chain, FFI surface, OEM override path, acceptance tests); §8.2 lists `i18n/*.json` as non-encrypted by design. No tables, columns, encryption, or FFI contracts removed or weakened. |
| 2026-06-20 | 1.2 | AI-Architect | **v0.7.5 — Convergence & Contract-Alignment Pass.** Header, document ID, status block, and companion links all re-pointed to the v0.7.5 graph (PRD v0.7.5 / TRD v0.7.5 / UXB v2.1 / AF v1.2). No table, column, index, migration, encryption boundary, or FFI contract was added, modified, or weakened in this revision — it is a strict truth-synchronisation pass for the backend schema document. |
