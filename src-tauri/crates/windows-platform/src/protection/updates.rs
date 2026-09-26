//! Opt-in rule-pack download, built on the same verified install path as a
//! manual import. Any failure leaves the active pack untouched, and offline
//! scanning keeps working.

use protection_core::{NetworkPurpose, ProtectionNetworkPolicy};

use super::net::{Endpoint, NetClient, NetError, Transport};
use super::rules_store::{RulesError, RulesStatus, RulesStore};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DownloadError {
    Net(NetError),
    Rules(RulesError),
}

impl std::fmt::Display for DownloadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Net(error) => write!(f, "{error}"),
            Self::Rules(error) => write!(f, "{error}"),
        }
    }
}

/// Unverified bytes of a downloaded pack; only `install_rule_pack` may use them.
#[derive(Debug)]
pub struct FetchedPack {
    pack: Vec<u8>,
    signature: Vec<u8>,
}

/// Network step. Needs no rules lock, so a slow or unreachable host does not
/// block the overview, imports or scans. The capability is checked first, so
/// nothing is sent while the policy is off.
pub fn fetch_rule_pack<T: Transport>(
    policy: &ProtectionNetworkPolicy,
    client: &NetClient<T>,
) -> Result<FetchedPack, DownloadError> {
    let capability = policy
        .capability(NetworkPurpose::RuleDownload)
        .ok_or(DownloadError::Net(NetError::NotPermitted))?;
    if !super::rules_store::external_packs_allowed() {
        return Err(DownloadError::Rules(RulesError::Disabled));
    }
    let pack = client
        .fetch(&capability, Endpoint::RulePack)
        .map_err(DownloadError::Net)?;
    let signature = client
        .fetch(&capability, Endpoint::RuleSignature)
        .map_err(DownloadError::Net)?;
    Ok(FetchedPack { pack, signature })
}

/// Install step: the store verifies the signature before parsing, and any
/// failure leaves the active pack in place.
pub fn install_rule_pack(
    fetched: &FetchedPack,
    store: &mut RulesStore,
) -> Result<RulesStatus, DownloadError> {
    store
        .install(&fetched.pack, &fetched.signature)
        .map_err(DownloadError::Rules)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protection::net::fake::CountingTransport;
    use crate::protection::rules_store::RulesSource;
    use crate::protection::test_support::*;
    use protection_core::PackError;

    fn store(label: &str) -> (std::path::PathBuf, RulesStore) {
        let root = temp_dir(label);
        let (pack, sig) = signed_pack(1, "baseline");
        let store = RulesStore::open_with(root.clone(), test_verifier(), pack, sig).unwrap();
        (root, store)
    }

    fn scans_eicar(store: &RulesStore) -> bool {
        let mut matcher = store.active().matcher();
        matcher.update(&eicar());
        !matcher.finish().hits.is_empty()
    }

    const ON: ProtectionNetworkPolicy = ProtectionNetworkPolicy {
        rule_download: true,
        password_breach_check: false,
    };

    fn download_rule_pack(
        policy: &ProtectionNetworkPolicy,
        client: &NetClient<CountingTransport>,
        store: &mut RulesStore,
    ) -> Result<RulesStatus, DownloadError> {
        install_rule_pack(&fetch_rule_pack(policy, client)?, store)
    }

    #[test]
    fn policy_off_makes_zero_network_calls() {
        let (root, mut store) = store("dl-off");
        let client = NetClient::new(CountingTransport::default());
        let off = ProtectionNetworkPolicy::default();
        assert_eq!(
            download_rule_pack(&off, &client, &mut store),
            Err(DownloadError::Net(NetError::NotPermitted))
        );
        // Enabling only the breach check does not enable downloads.
        let other = ProtectionNetworkPolicy {
            rule_download: false,
            password_breach_check: true,
        };
        assert!(download_rule_pack(&other, &client, &mut store).is_err());
        assert_eq!(client.transport().count(), 0);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn transport_failure_keeps_current_pack_and_offline_scanning() {
        let (root, mut store) = store("dl-fail");
        let before = store.status().clone();
        for failure in [
            NetError::Unreachable,
            NetError::Status(404),
            NetError::TooLarge,
        ] {
            let client = NetClient::new(CountingTransport::with(vec![Err(failure)]));
            assert_eq!(
                download_rule_pack(&ON, &client, &mut store),
                Err(DownloadError::Net(failure))
            );
            assert_eq!(
                client.transport().count(),
                1,
                "no signature fetch after a failed pack fetch"
            );
        }
        assert_eq!(store.status(), &before);
        assert!(scans_eicar(&store));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn downloaded_pack_with_bad_signature_is_rejected() {
        let (root, mut store) = store("dl-badsig");
        let (pack, _) = signed_pack(5, "remote");
        let (_, wrong_sig) = signed_pack_with(99, 5, "remote");
        let client = NetClient::new(CountingTransport::with(vec![Ok(pack), Ok(wrong_sig)]));
        assert_eq!(
            download_rule_pack(&ON, &client, &mut store),
            Err(DownloadError::Rules(RulesError::Pack(
                PackError::BadSignature
            )))
        );
        assert_eq!(store.status().source, RulesSource::EmbeddedBaseline);
        assert!(scans_eicar(&store));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn downloaded_pack_installs_and_requests_only_fixed_endpoints() {
        let (root, mut store) = store("dl-ok");
        let (pack, sig) = signed_pack(5, "remote");
        let client = NetClient::new(CountingTransport::with(vec![Ok(pack), Ok(sig)]));
        let status = download_rule_pack(&ON, &client, &mut store).unwrap();
        assert_eq!(
            (status.source, status.sequence),
            (RulesSource::Installed, 5)
        );
        let requests = client.transport().requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 2);
        assert!(requests.iter().all(|(host, _, headers)| host == "raw.githubusercontent.com" && headers.is_empty()));
        let _ = std::fs::remove_dir_all(root);
    }
}
