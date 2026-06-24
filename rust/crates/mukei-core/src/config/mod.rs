//! `mukei_core::config` — TRD §12.5.
//!
//! **Strict** config schema validator. The boot path calls
//! [`MukeiConfig::load_and_validate`] and refuses to start if any field
//! is wrong (REQ-CON-04 / §11.2). The QML side gets an
//! `MukeiError::ConfigInvalid` whose `field`+`reason` are rendered in
//! the error dialog so a first-run misconfig is human-readable.

use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{MukeiError, Result};

/// On-disk representation. **Strict** — no [`#[serde(default)]`] — every
/// required field must be present.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct MukeiConfig {
    pub models_dir:       PathBuf,
    pub vectors_dir:      PathBuf,
    pub database_path:    PathBuf,
    pub saf_tokens_db:    PathBuf,
    pub crashes_dir:      PathBuf,
    pub logs_dir:         PathBuf,
    pub max_blocking:     BlockingPoolCfg,
    pub gpu_layers:       i32,
    pub n_ctx:            u32,
    pub n_threads:        u32,
    pub watchdog:         WatchdogCfg,
    pub storage:          StorageCfg,
    pub saf:              SafCfg,
    pub agent:            AgentCfg,
    pub defaults:         DefaultsCfg,
    /// List of known third-party API-key slots whose values are
    /// Keystore-wrapped ciphertext (REQ-SEC-20 / §12.4).
    #[serde(default)]
    pub wrapped_secrets:  Vec<WrappedSecretRef>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct BlockingPoolCfg {
    pub max_blocking_threads_android: usize,
    pub max_blocking_threads_desktop: usize,
    pub tool_slots: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct WatchdogCfg {
    pub max_iterations: usize,
    pub max_token_budget: u64,
    pub max_wall_seconds: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct StorageCfg {
    pub sqlcipher_kdf_iter: u32,
    pub wal_autocheckpoint_pages: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SafCfg {
    pub persist_grants_to_db: bool,
    pub prompt_on_first_use: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct AgentCfg {
    /// Consecutive failures on the same `(tool, fingerprint)` pair
    /// before the abuse blocker fires. Default 5 (raised from 2 per
    /// the v0.7.5 audit — the legacy value was too brittle for transient
    /// network errors).
    pub max_failures_per_tool: u32,
    /// Number of recent messages replayed into the context window when
    /// the recovery_state table indicates an OS-kill resume.
    pub recovered_history_window: u32,
    /// Number of consecutive byte-identical tool outputs that trigger
    /// the no-progress / backoff envelope.
    #[serde(default = "AgentCfg::default_repeat_output_window")]
    pub repeat_output_window: u32,
    /// Advisory backoff (in seconds) inserted into the no-progress
    /// envelope handed back to the LLM.
    #[serde(default = "AgentCfg::default_repeat_output_backoff_secs")]
    pub repeat_output_backoff_secs: u32,
    /// Architect review GH #13: cap on the number of `tokio::spawn` tool
    /// tasks alive at once (PRD REQ-CON-02). Default 4. Defaulted on
    /// missing field so v0.7.5 configs that pre-date this knob still
    /// load — the strict-config validator only fires on UNKNOWN fields,
    /// not missing-with-default fields.
    #[serde(default = "AgentCfg::default_max_concurrent_tools")]
    pub max_concurrent_tools: u32,
}

impl AgentCfg {
    /// Default value for `repeat_output_window` when the field is omitted
    /// from `config.toml` (kept for forward compatibility with v0.7.4
    /// configs that pre-date the policy field).
    pub fn default_repeat_output_window() -> u32 { 2 }
    /// Default value for `repeat_output_backoff_secs`.
    pub fn default_repeat_output_backoff_secs() -> u32 { 10 }
    /// Default value for `max_concurrent_tools` (architect review GH #13).
    pub fn default_max_concurrent_tools() -> u32 { 4 }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct DefaultsCfg {
    pub prompt_card_auto_send: bool,
    pub thermal_autopause: bool,
    pub first_run_completed: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct WrappedSecretRef {
    pub slot: String,        // e.g. "brave_api_key"
    pub blob_path: PathBuf,  // e.g. ~/.mukei/secrets/brave_key.enc
    pub created: chrono::DateTime<chrono::Utc>,
}

/// Lenient first-pass deserialiser. Used internally — the public API
/// is [`MukeiConfig::load_and_validate`] which pokes the keys not in
/// [`Self::KNOWN_KEYS`] through a tighter filter.
mod raw {
    use serde::Deserialize;

    #[derive(Deserialize)]
    pub struct RawRoot {
        #[serde(flatten)]
        pub fields: std::collections::BTreeMap<String, toml::Value>,
    }
}

impl MukeiConfig {
    /// Strictly known top-level keys. Anything else is `ConfigUnknownField`.
    pub fn known_keys() -> &'static [&'static str] {
        &[
            "models_dir",
            "vectors_dir",
            "database_path",
            "saf_tokens_db",
            "crashes_dir",
            "logs_dir",
            "max_blocking",
            "gpu_layers",
            "n_ctx",
            "n_threads",
            "watchdog",
            "storage",
            "saf",
            "agent",
            "defaults",
            "wrapped_secrets",
        ]
    }

    /// Load + validate. Strict. The bridge crate's `boot()` calls this.
    pub fn load_and_validate(path: &Path) -> Result<Self> {
        let bytes = fs::read(path).map_err(|e| {
            MukeiError::SafeStorageUnavailable(format!(
                "config.toml read failed: {e} (path={})",
                path.display()
            ))
        })?;
        Self::validate_toml_keys(&bytes)?;
        let cfg: MukeiConfig = toml::from_str(std::str::from_utf8(&bytes).map_err(|e| MukeiError::ConfigInvalid {
            field: "root".into(),
            reason: e.to_string(),
        })?).map_err(|e| {
            // tom-parse-error prints "field X" — surface that to the UI verbatim.
            let msg = e.to_string();
            MukeiError::ConfigInvalid {
                field: msg.clone(),
                reason: msg,
            }
        })?;
        Self::logical_validate(&cfg)?;
        Ok(cfg)
    }

    fn validate_toml_keys(bytes: &[u8]) -> Result<()> {
        let raw: raw::RawRoot = toml::from_str(std::str::from_utf8(bytes).map_err(|e| MukeiError::ConfigInvalid {
            field: "root".into(),
            reason: e.to_string(),
        })?).map_err(|e| {
            MukeiError::ConfigInvalid {
                field: "root".into(),
                reason: e.to_string(),
            }
        })?;
        let known: BTreeSet<&'static str> = Self::known_keys().iter().copied().collect();
        for k in raw.fields.keys() {
            if !known.contains(k.as_str()) {
                return Err(MukeiError::ConfigUnknownField(k.clone()));
            }
        }
        Ok(())
    }

    fn logical_validate(cfg: &MukeiConfig) -> Result<()> {
        if cfg.gpu_layers < 0 {
            return Err(MukeiError::ConfigInvalid {
                field: "gpu_layers".into(),
                reason: "must be ≥ 0 (0 = CPU-only)".into(),
            });
        }
        if cfg.n_ctx < 256 || cfg.n_ctx > 32768 {
            return Err(MukeiError::ConfigInvalid {
                field: "n_ctx".into(),
                reason: "out of range [256, 32768]".into(),
            });
        }
        if cfg.n_threads == 0 || cfg.n_threads > 32 {
            return Err(MukeiError::ConfigInvalid {
                field: "n_threads".into(),
                reason: "must be in [1, 32]".into(),
            });
        }
        if cfg.watchdog.max_iterations == 0 {
            return Err(MukeiError::ConfigInvalid {
                field: "watchdog.max_iterations".into(),
                reason: "must be ≥ 1 (REQ-AGT-04)".into(),
            });
        }
        Ok(())
    }
}

/// Helper used by tests + the bridge crate to write a default
/// `mukei.toml` to disk on first run.
pub fn write_default(path: &Path) -> io::Result<()> {
    let toml_text = include_str!("../../../../migrations/000_default_config.toml");
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, toml_text)
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID_TOML: &str = r#"
models_dir        = "/var/mukei/models"
vectors_dir       = "/var/mukei/vectors"
database_path     = "/var/mukei/db/mukei.db"
saf_tokens_db     = "/var/mukei/db/saf_tokens.db"
crashes_dir       = "/var/mukei/crashes"
logs_dir          = "/var/mukei/logs"

gpu_layers        = 32
n_ctx             = 4096
n_threads         = 4

[max_blocking]
max_blocking_threads_android = 6
max_blocking_threads_desktop = 8
tool_slots                   = 2

[watchdog]
max_iterations     = 8
max_token_budget   = 8192
max_wall_seconds   = 600

[storage]
sqlcipher_kdf_iter          = 256000
wal_autocheckpoint_pages    = 1000

[saf]
persist_grants_to_db = true
prompt_on_first_use  = true

[agent]
max_failures_per_tool       = 5
recovered_history_window    = 12
repeat_output_window        = 2
repeat_output_backoff_secs  = 10

[defaults]
prompt_card_auto_send = false
thermal_autopause      = true
first_run_completed    = false
"#;

    #[test]
    fn valid_toml_passes() {
        let cfg: MukeiConfig = toml::from_str(VALID_TOML).expect("hard-coded test must compile");
        MukeiConfig::logical_validate(&cfg).expect("hard-coded test must validate");
    }

    #[test]
    fn rejects_unknown_field() {
        // Insert the unknown key at the root (before any [table] section)
        // so the TOML parser sees it as a top-level key rather than a
        // member of the last table.
        let cfg_text = format!("mystery_key = 1\n{VALID_TOML}");
        let err = MukeiConfig::load_and_validate_from_str(&cfg_text).unwrap_err();
        assert!(matches!(err, MukeiError::ConfigUnknownField(_)));
    }

    #[test]
    fn rejects_zero_max_iterations() {
        let cfg_text = VALID_TOML.replace("max_iterations     = 8", "max_iterations     = 0");
        let err = MukeiConfig::load_and_validate_from_str(&cfg_text).unwrap_err();
        assert!(matches!(err, MukeiError::ConfigInvalid { .. }));
    }

    #[test]
    fn rejects_out_of_range_n_ctx() {
        let cfg_text = VALID_TOML.replace("n_ctx             = 4096", "n_ctx             = 64");
        let err = MukeiConfig::load_and_validate_from_str(&cfg_text).unwrap_err();
        assert!(matches!(err, MukeiError::ConfigInvalid { .. }));
    }
}

// ---------------------------------------------------------------------
// Test-only helper — split out so `load_and_validate` keeps its strict
// `&Path` signature.
// ---------------------------------------------------------------------
#[cfg(test)]
impl MukeiConfig {
    fn load_and_validate_from_str(s: &str) -> Result<Self> {
        Self::validate_toml_keys(s.as_bytes())?;
        let cfg: MukeiConfig = toml::from_str(s).map_err(|e| MukeiError::ConfigInvalid {
            field: "root".into(),
            reason: e.to_string(),
        })?;
        Self::logical_validate(&cfg)?;
        Ok(cfg)
    }
}
