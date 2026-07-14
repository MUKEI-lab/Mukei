//! Build/runtime provenance exposed to diagnostics without conflating product,
//! protocol, schema, compiler profile, feature flags, or environment mode.

use serde::{Deserialize, Serialize};

use mukei_core::config::{HardeningMode, RuntimeEnvironmentMode};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct RuntimeProvenanceSnapshot {
    pub(crate) schema_version: u32,
    pub(crate) product_version: String,
    pub(crate) protocol_version: u32,
    pub(crate) database_schema_version: u32,
    pub(crate) build_identifier: Option<String>,
    pub(crate) compiler_profile: String,
    pub(crate) runtime_environment_mode: RuntimeEnvironmentMode,
    pub(crate) hardening_mode: HardeningMode,
    pub(crate) feature_flags: Vec<String>,
}

pub(crate) fn runtime_environment_mode() -> RuntimeEnvironmentMode {
    #[cfg(feature = "runtime_production")]
    {
        RuntimeEnvironmentMode::Production
    }
    #[cfg(all(not(feature = "runtime_production"), feature = "runtime_test"))]
    {
        RuntimeEnvironmentMode::Test
    }
    #[cfg(all(not(feature = "runtime_production"), not(feature = "runtime_test")))]
    {
        RuntimeEnvironmentMode::Development
    }
}

pub(crate) fn hardening_mode() -> HardeningMode {
    if cfg!(feature = "runtime_hardening") {
        HardeningMode::Hardened
    } else {
        HardeningMode::Standard
    }
}

pub(crate) fn snapshot() -> RuntimeProvenanceSnapshot {
    let mut feature_flags = Vec::new();
    for (name, enabled) in [
        ("sqlcipher", cfg!(feature = "sqlcipher")),
        ("rusqlite", cfg!(feature = "rusqlite")),
        ("android_keystore", cfg!(feature = "android_keystore")),
        ("network", cfg!(feature = "network")),
        ("llama_cpp", cfg!(feature = "llama_cpp")),
        ("candle", cfg!(feature = "candle")),
        ("usearch_hnsw", cfg!(feature = "usearch_hnsw")),
        ("diagnostics_export", cfg!(feature = "diagnostics_export")),
        ("runtime_hardening", cfg!(feature = "runtime_hardening")),
    ] {
        if enabled {
            feature_flags.push(name.to_string());
        }
    }

    #[cfg(feature = "rusqlite")]
    let database_schema_version = mukei_core::storage::Migrator::embedded()
        .latest_version()
        .unwrap_or(0);
    #[cfg(not(feature = "rusqlite"))]
    let database_schema_version = 0;

    RuntimeProvenanceSnapshot {
        schema_version: 1,
        product_version: env!("CARGO_PKG_VERSION").to_string(),
        protocol_version: mukei_core::ui_contract::UI_CONTRACT_VERSION,
        database_schema_version,
        build_identifier: option_env!("MUKEI_BUILD_ID")
            .or(option_env!("GIT_COMMIT"))
            .map(str::to_string),
        compiler_profile: if cfg!(debug_assertions) {
            "debug".to_string()
        } else {
            "release".to_string()
        },
        runtime_environment_mode: runtime_environment_mode(),
        hardening_mode: hardening_mode(),
        feature_flags,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sol03_provenance_keeps_product_protocol_and_schema_versions_distinct() {
        let value = snapshot();
        assert!(!value.product_version.is_empty());
        assert!(value.protocol_version > 0);
        #[cfg(feature = "rusqlite")]
        assert!(value.database_schema_version > 0);
        assert_ne!(value.product_version, value.protocol_version.to_string());
    }

    #[test]
    fn sol03_runtime_provenance_snapshot_is_internally_consistent() {
        let value = snapshot();
        assert_eq!(value.schema_version, 1);
        assert_eq!(
            value.hardening_mode == HardeningMode::Hardened,
            value
                .feature_flags
                .iter()
                .any(|flag| flag == "runtime_hardening")
        );
        assert_eq!(value.runtime_environment_mode, runtime_environment_mode());
    }
    #[test]
    fn sol03_compiler_profile_and_runtime_hardening_feature_names_are_not_conflated() {
        const WORKSPACE_MANIFEST: &str = include_str!("../../../Cargo.toml");
        const BRIDGE_MANIFEST: &str = include_str!("../Cargo.toml");
        assert!(WORKSPACE_MANIFEST.contains("[profile.release-hardening]"));
        assert!(BRIDGE_MANIFEST.contains("runtime_hardening"));
        assert!(!BRIDGE_MANIFEST.contains("release_hardening   ="));
    }
}
