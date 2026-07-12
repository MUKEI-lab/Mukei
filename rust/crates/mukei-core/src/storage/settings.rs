//! Settings persistence with secret separation.
//!
//! `preferences` stores only registered, non-secret user preferences.
//! `secret_refs` stores opaque secure-store handles; plaintext API keys
//! must stay in Android Keystore/secure storage and runtime memory.

use crate::error::{MukeiError, Result};

use super::pool::{DatabasePool, PooledConnectionExt};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PreferenceValue {
    Bool(bool),
    Integer(i64),
    String(String),
}

impl PreferenceValue {
    fn value_type(&self) -> &'static str {
        match self {
            Self::Bool(_) => "bool",
            Self::Integer(_) => "integer",
            Self::String(_) => "string",
        }
    }

    fn to_json(&self) -> String {
        match self {
            Self::Bool(value) => value.to_string(),
            Self::Integer(value) => value.to_string(),
            Self::String(value) => serde_json::to_string(value).unwrap_or_else(|_| "\"\"".into()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct PreferenceRecord {
    pub key: String,
    pub value_json: String,
    pub value_type: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SecretRefRecord {
    pub slot: String,
    pub provider: String,
    pub storage_key: String,
}

pub struct SettingsRepository;

impl SettingsRepository {
    pub fn validate_preference(key: &str, value: &PreferenceValue) -> Result<()> {
        if looks_secret(key) {
            return Err(MukeiError::PermissionDenied);
        }
        match (key, value) {
            ("font_scale_percent", PreferenceValue::Integer(value))
                if !(85..=200).contains(value) =>
            {
                Err(MukeiError::ToolArgumentInvalid {
                    field: "font_scale_percent",
                    reason: "expected a value between 85 and 200".into(),
                })
            }
            ("temperature_milli", PreferenceValue::Integer(value))
                if !(0..=2000).contains(value) =>
            {
                Err(MukeiError::ToolArgumentInvalid {
                    field: "temperature_milli",
                    reason: "expected a value between 0 and 2000".into(),
                })
            }
            ("max_tokens_default", PreferenceValue::Integer(value))
                if !(64..=32768).contains(value) =>
            {
                Err(MukeiError::ToolArgumentInvalid {
                    field: "max_tokens_default",
                    reason: "expected a value between 64 and 32768".into(),
                })
            }
            ("top_p_milli", PreferenceValue::Integer(value))
                if !(1..=1000).contains(value) =>
            {
                Err(MukeiError::ToolArgumentInvalid {
                    field: "top_p_milli",
                    reason: "expected a value between 1 and 1000".into(),
                })
            }
            ("prompt_card_auto_send", PreferenceValue::Bool(_))
            | ("thermal_autopause", PreferenceValue::Bool(_))
            | ("first_run_completed", PreferenceValue::Bool(_))
            | ("search.enable_cache", PreferenceValue::Bool(_))
            | ("reduce_motion", PreferenceValue::Bool(_))
            | ("high_contrast", PreferenceValue::Bool(_))
            | ("search.max_parallel_engines", PreferenceValue::Integer(_))
            | ("search.brave_timeout_secs", PreferenceValue::Integer(_))
            | ("search.tavily_timeout_secs", PreferenceValue::Integer(_))
            | ("font_scale_percent", PreferenceValue::Integer(_))
            | ("temperature_milli", PreferenceValue::Integer(_))
            | ("max_tokens_default", PreferenceValue::Integer(_))
            | ("top_p_milli", PreferenceValue::Integer(_)) => Ok(()),
            ("theme_mode", PreferenceValue::String(value)) => {
                match value.as_str() {
                    "dolce_vita" | "espresso" | "taupe" => Ok(()),
                    _ => Err(MukeiError::ToolArgumentInvalid {
                        field: "theme_mode",
                        reason: "unsupported theme mode".into(),
                    }),
                }
            }
            ("remote_feature_policy", PreferenceValue::String(value)) => value
                .parse::<crate::tools::RemoteFeaturePolicy>()
                .map(|_| ()),
            _ => Err(MukeiError::ToolArgumentInvalid {
                field: "setting",
                reason: format!("unsupported setting key or value type: {key}"),
            }),
        }
    }

    pub async fn upsert_preference(
        pool: &DatabasePool,
        key: impl Into<String>,
        value: PreferenceValue,
    ) -> Result<()> {
        let key = key.into();
        Self::validate_preference(&key, &value)?;
        let value_json = value.to_json();
        let value_type = value.value_type().to_string();
        pool.with_conn(move |c| {
            c.execute(
                "INSERT INTO preferences (key, value_json, value_type, updated_at) \
                 VALUES (?1, ?2, ?3, strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) \
                 ON CONFLICT(key) DO UPDATE SET \
                   value_json = excluded.value_json, \
                   value_type = excluded.value_type, \
                   updated_at = excluded.updated_at",
                rusqlite::params![key, value_json, value_type],
            )?;
            Ok::<_, super::pool::DbError>(())
        })
        .await?;
        Ok(())
    }

    pub async fn get_preference(
        pool: &DatabasePool,
        key: impl Into<String>,
    ) -> Result<Option<PreferenceRecord>> {
        let key = key.into();
        pool.with_conn(move |c| {
            let mut stmt = c.prepare(
                "SELECT key, value_json, value_type, updated_at FROM preferences WHERE key = ?1",
            )?;
            let mut rows = stmt.query([key])?;
            if let Some(row) = rows.next()? {
                Ok::<_, super::pool::DbError>(Some(PreferenceRecord {
                    key: row.get(0)?,
                    value_json: row.get(1)?,
                    value_type: row.get(2)?,
                    updated_at: row.get(3)?,
                }))
            } else {
                Ok(None)
            }
        })
        .await
    }

    pub async fn list_preferences(pool: &DatabasePool) -> Result<Vec<PreferenceRecord>> {
        pool.with_conn(|c| {
            let mut stmt = c.prepare(
                "SELECT key, value_json, value_type, updated_at FROM preferences ORDER BY key",
            )?;
            let rows = stmt
                .query_map([], |row| {
                    Ok(PreferenceRecord {
                        key: row.get(0)?,
                        value_json: row.get(1)?,
                        value_type: row.get(2)?,
                        updated_at: row.get(3)?,
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok::<_, super::pool::DbError>(rows)
        })
        .await
    }

    pub async fn upsert_secret_ref(pool: &DatabasePool, record: SecretRefRecord) -> Result<()> {
        if record.slot.trim().is_empty()
            || record.provider.trim().is_empty()
            || record.storage_key.trim().is_empty()
        {
            return Err(MukeiError::ConfigInvalid {
                field: "secret_ref".into(),
                reason: "slot, provider, and storage_key are required".into(),
            });
        }
        pool.with_conn(move |c| {
            c.execute(
                "INSERT INTO secret_refs (slot, provider, storage_key, created_at, updated_at) \
                 VALUES (?1, ?2, ?3, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) \
                 ON CONFLICT(slot) DO UPDATE SET \
                   provider = excluded.provider, \
                   storage_key = excluded.storage_key, \
                   updated_at = excluded.updated_at",
                rusqlite::params![record.slot, record.provider, record.storage_key],
            )?;
            Ok::<_, super::pool::DbError>(())
        })
        .await?;
        Ok(())
    }
}

fn looks_secret(key: &str) -> bool {
    let lowered = key.to_ascii_lowercase();
    lowered.contains("secret")
        || lowered.contains("token")
        || lowered.contains("api_key")
        || lowered.contains("apikey")
        || lowered.contains("password")
        || lowered.contains("cipher")
        || lowered.contains("key_hex")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Migrator;

    async fn migrated_pool() -> DatabasePool {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("settings.db");
        let pool = DatabasePool::open(&db_path).unwrap();
        Migrator::embedded().apply_pending(&pool).await.unwrap();
        std::mem::forget(dir);
        pool
    }

    #[test]
    fn rejects_secret_looking_preference_keys() {
        let err = SettingsRepository::validate_preference(
            "provider.api_key",
            &PreferenceValue::String("nope".into()),
        )
        .unwrap_err();
        assert!(matches!(err, MukeiError::PermissionDenied));
    }

    #[test]
    fn rejects_unknown_preference_keys() {
        let err = SettingsRepository::validate_preference("surprise", &PreferenceValue::Bool(true))
            .unwrap_err();
        assert!(matches!(err, MukeiError::ToolArgumentInvalid { .. }));
    }

    #[test]
    fn validates_remote_feature_policy_before_persisting() {
        assert!(SettingsRepository::validate_preference(
            "remote_feature_policy",
            &PreferenceValue::String("remote_allowed".into()),
        )
        .is_ok());
        assert!(SettingsRepository::validate_preference(
            "remote_feature_policy",
            &PreferenceValue::String("send_everything".into()),
        )
        .is_err());
    }

    #[tokio::test]
    async fn upsert_preference_round_trips_without_secret_table() {
        let pool = migrated_pool().await;
        SettingsRepository::upsert_preference(
            &pool,
            "thermal_autopause",
            PreferenceValue::Bool(false),
        )
        .await
        .unwrap();

        let stored = SettingsRepository::get_preference(&pool, "thermal_autopause")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.key, "thermal_autopause");
        assert_eq!(stored.value_type, "bool");
        assert_eq!(stored.value_json, "false");
    }

    #[test]
    fn validates_ui_preference_ranges_and_theme_names() {
        assert!(SettingsRepository::validate_preference(
            "font_scale_percent",
            &PreferenceValue::Integer(200),
        )
        .is_ok());
        assert!(SettingsRepository::validate_preference(
            "font_scale_percent",
            &PreferenceValue::Integer(201),
        )
        .is_err());
        assert!(SettingsRepository::validate_preference(
            "theme_mode",
            &PreferenceValue::String("espresso".into()),
        )
        .is_ok());
        assert!(SettingsRepository::validate_preference(
            "theme_mode",
            &PreferenceValue::String("neon".into()),
        )
        .is_err());
    }

    #[tokio::test]
    async fn list_preferences_returns_persisted_ui_projection_values() {
        let pool = migrated_pool().await;
        SettingsRepository::upsert_preference(
            &pool,
            "theme_mode",
            PreferenceValue::String("taupe".into()),
        )
        .await
        .unwrap();
        SettingsRepository::upsert_preference(
            &pool,
            "reduce_motion",
            PreferenceValue::Bool(true),
        )
        .await
        .unwrap();

        let rows = SettingsRepository::list_preferences(&pool).await.unwrap();
        assert!(rows.iter().any(|row| {
            row.key == "theme_mode" && row.value_json == "\"taupe\""
        }));
        assert!(rows
            .iter()
            .any(|row| row.key == "reduce_motion" && row.value_json == "true"));
    }

    #[tokio::test]
    async fn secret_refs_store_only_opaque_handles() {
        let pool = migrated_pool().await;
        SettingsRepository::upsert_secret_ref(
            &pool,
            SecretRefRecord {
                slot: "brave_api_key".into(),
                provider: "android_keystore".into(),
                storage_key: "alias/mukei/brave".into(),
            },
        )
        .await
        .unwrap();

        let count: i64 = pool
            .with_conn(|c| {
                let count = c.query_row("SELECT COUNT(*) FROM secret_refs", [], |row| {
                    row.get::<_, i64>(0)
                })?;
                Ok::<_, super::super::pool::DbError>(count)
            })
            .await
            .unwrap();
        assert_eq!(count, 1);
    }
}
