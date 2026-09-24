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
    fn policy_rejects_unknown_fields() {
        assert!(
            serde_json::from_str::<ProtectionNetworkPolicy>(
                r#"{"ruleDownload":false,"passwordBreachCheck":false,"telemetry":true}"#
            )
            .is_err()
        );
    }
}
