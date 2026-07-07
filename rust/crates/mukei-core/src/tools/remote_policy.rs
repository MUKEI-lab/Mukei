//! Remote feature privacy policy.

use std::str::FromStr;

use crate::error::{MukeiError, Result};

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum RemoteFeaturePolicy {
    #[default]
    LocalOnly,
    AskBeforeRemote,
    RemoteAllowed,
    EnterpriseDisabled,
}

impl RemoteFeaturePolicy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::LocalOnly => "local_only",
            Self::AskBeforeRemote => "ask_before_remote",
            Self::RemoteAllowed => "remote_allowed",
            Self::EnterpriseDisabled => "enterprise_disabled",
        }
    }

    pub fn ensure_remote_allowed(self, feature: &'static str) -> Result<()> {
        if matches!(self, Self::RemoteAllowed) {
            Ok(())
        } else {
            Err(MukeiError::RemoteFeatureDisabled {
                feature,
                policy: self.as_str().to_string(),
            })
        }
    }
}

impl FromStr for RemoteFeaturePolicy {
    type Err = MukeiError;

    fn from_str(value: &str) -> Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "local_only" | "local-only" | "local" => Ok(Self::LocalOnly),
            "ask_before_remote" | "ask-before-remote" | "ask" => Ok(Self::AskBeforeRemote),
            "remote_allowed" | "remote-allowed" | "remote" | "allowed" => Ok(Self::RemoteAllowed),
            "enterprise_disabled" | "enterprise-disabled" | "disabled" => {
                Ok(Self::EnterpriseDisabled)
            }
            _ => Err(MukeiError::ConfigInvalid {
                field: "remote_feature_policy".into(),
                reason:
                    "expected local_only, ask_before_remote, remote_allowed, or enterprise_disabled"
                        .into(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_policy_is_local_only() {
        assert_eq!(
            RemoteFeaturePolicy::default(),
            RemoteFeaturePolicy::LocalOnly
        );
        let err = RemoteFeaturePolicy::default()
            .ensure_remote_allowed("web_search")
            .unwrap_err();
        assert_eq!(err.error_code(), "ERR_REMOTE_DISABLED");
    }

    #[test]
    fn parses_policy_aliases() {
        assert_eq!(
            "remote_allowed".parse::<RemoteFeaturePolicy>().unwrap(),
            RemoteFeaturePolicy::RemoteAllowed
        );
        assert_eq!(
            "local".parse::<RemoteFeaturePolicy>().unwrap(),
            RemoteFeaturePolicy::LocalOnly
        );
        assert!("surprise".parse::<RemoteFeaturePolicy>().is_err());
    }
}
