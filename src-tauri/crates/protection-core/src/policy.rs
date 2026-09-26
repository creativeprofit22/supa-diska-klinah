use serde::{Deserialize, Serialize};

/// Network opt-ins. Every capability defaults to off (ADR 0003, decision 6).
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProtectionNetworkPolicy {
    pub rule_download: bool,
    pub password_breach_check: bool,
}

/// The purposes for which the app may contact the network.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NetworkPurpose {
    RuleDownload,
    PasswordBreachCheck,
    /// Checking for and downloading app updates.
    UpdateCheck,
}

/// Proof that the user enabled a specific network capability.
///
/// The private field means a token can only be created by
/// [`ProtectionNetworkPolicy::capability`], so the single network sink can
/// require one without trusting its caller.
#[derive(Debug)]
pub struct NetworkCapability {
    purpose: NetworkPurpose,
    _sealed: (),
}

impl NetworkCapability {
    pub fn purpose(&self) -> NetworkPurpose {
        self.purpose
    }
}

impl ProtectionNetworkPolicy {
    pub fn allows(&self, purpose: NetworkPurpose) -> bool {
        match purpose {
            NetworkPurpose::RuleDownload => self.rule_download,
            NetworkPurpose::PasswordBreachCheck => self.password_breach_check,
            NetworkPurpose::UpdateCheck => false,
        }
    }

    /// Mint a capability only when the matching flag is enabled.
    pub fn capability(&self, purpose: NetworkPurpose) -> Option<NetworkCapability> {
        self.allows(purpose).then_some(NetworkCapability {
            purpose,
            _sealed: (),
        })
    }
}

/// The app-update opt-in. Kept separate from protection's policy so turning
/// on rule downloads never enables update checks, or the reverse.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct UpdateCheckPolicy {
    pub enabled: bool,
}

impl UpdateCheckPolicy {
    /// Mint the update capability only when the user turned update checks on.
    pub fn capability(&self) -> Option<NetworkCapability> {
        self.enabled.then_some(NetworkCapability {
            purpose: NetworkPurpose::UpdateCheck,
            _sealed: (),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_deny_every_purpose() {
        let policy = ProtectionNetworkPolicy::default();
        assert!(policy.capability(NetworkPurpose::RuleDownload).is_none());
        assert!(
            policy
                .capability(NetworkPurpose::PasswordBreachCheck)
                .is_none()
        );
    }

    #[test]
    fn each_flag_mints_only_its_own_capability() {
        let policy = ProtectionNetworkPolicy {
            rule_download: true,
            password_breach_check: false,
        };
        assert_eq!(
            policy
                .capability(NetworkPurpose::RuleDownload)
                .unwrap()
                .purpose(),
            NetworkPurpose::RuleDownload
        );
        assert!(
            policy
                .capability(NetworkPurpose::PasswordBreachCheck)
                .is_none()
        );
    }

    #[test]
    fn update_capability_is_separate_and_off_by_default() {
        assert!(UpdateCheckPolicy::default().capability().is_none());
        let all_protection = ProtectionNetworkPolicy {
            rule_download: true,
            password_breach_check: true,
        };
        assert!(
            all_protection
                .capability(NetworkPurpose::UpdateCheck)
                .is_none()
        );
        assert_eq!(
            UpdateCheckPolicy { enabled: true }
                .capability()
                .unwrap()
                .purpose(),
            NetworkPurpose::UpdateCheck
        );
    }

    #[test]
    fn policy_rejects_unknown_fields() {
        assert!(
            serde_json::from_str::<ProtectionNetworkPolicy>(
                r#"{"ruleDownload":false,"passwordBreachCheck":false,"telemetry":true}"#
            )
            .is_err()
        );
    }
}
