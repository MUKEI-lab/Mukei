# Mukei v0.1 Sol merge notes

Base: `Mukei_v0.9.zip`
Selective restore source: `mukei_v0.8.zip`

## Restored / repaired

- Restored `SettingsRepository`, typed preference validation, and `secret_refs` support.
- Restored working `update_setting()` persistence instead of the v0.9 no-op.
- Preserved canonical V008 settings migration and moved schema metadata/RAG tombstones to V009.
- Added a compatibility repair for databases created by the short-lived v0.9 V008 migration lineage.
- Restored opaque model download destination tokens so app-private absolute paths do not cross bridge events.
- Restored synchronous API-key/privacy-policy updates to close the stale-policy race window.
- Preserved v0.9 improvements: clean source packaging, CI workflow, RetryPolicy foundation, valid TOML defaults, and UiError redaction.

## Verification performed in this environment

- Archive/file-level diff and source-level consistency checks.
- TOML parse check for the shipped default config.
- SQLite execution of embedded migration SQL on a fresh database.
- SQLite compatibility simulation for the legacy v0.9 V008 schema lineage.
- No Rust compilation was performed because `cargo`/`rustc` are unavailable in this environment.
