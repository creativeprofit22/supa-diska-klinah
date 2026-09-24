//! Opt-in Pwned Passwords k-anonymity check.
//!
//! The password is SHA-1 hashed locally with Windows CNG. Only the first five
//! hex characters of the hash are sent; the service returns every suffix in
//! that range (padded with decoys), and the match happens here. The password
//! and full hash live in zeroizing buffers and are never logged or stored.
//! E-mail breach lookups are unsupported (they need a paid key and send identity).

use protection_core::{Evidence, NetworkPurpose, ProtectionNetworkPolicy};
use serde::Serialize;
use windows_sys::Win32::Security::Cryptography::{BCRYPT_SHA1_ALG_HANDLE, BCryptHash};
use zeroize::Zeroizing;

use super::net::{Endpoint, NetClient, NetError, Transport};

pub const PROVIDER: &str = "Have I Been Pwned: Pwned Passwords";
pub const MAX_PASSWORD_BYTES: usize = 1024;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PasswordBreachResult {
    /// Times the password appears in the provider's corpus; 0 = not listed.
    pub occurrences: u64,
    pub evidence: Evidence,
}

/// SHA-1 via CNG into a zeroizing buffer, as uppercase hex.
fn sha1_upper_hex(input: &[u8]) -> Result<Zeroizing<Vec<u8>>, NetError> {
    let mut digest = Zeroizing::new([0_u8; 20]);
    // SAFETY: the pseudo-handle needs no open/close; buffers are sized as passed.
    let status = unsafe {
        BCryptHash(
            BCRYPT_SHA1_ALG_HANDLE,
            std::ptr::null(),
            0,
            input.as_ptr(),
            input.len() as u32,
            digest.as_mut_ptr(),
            digest.len() as u32,
        )
    };
    if status != 0 {
        return Err(NetError::InvalidRequest);
    }
    const DIGITS: &[u8; 16] = b"0123456789ABCDEF";
    let mut hex = Zeroizing::new(Vec::with_capacity(40));
    for byte in digest.iter() {
        hex.push(DIGITS[(byte >> 4) as usize]);
        hex.push(DIGITS[(byte & 0x0f) as usize]);
    }
    Ok(hex)
}

/// Find `suffix` (35 uppercase hex chars) in a range response.
fn occurrences(body: &[u8], suffix: &[u8]) -> Result<u64, NetError> {
    let text = std::str::from_utf8(body).map_err(|_| NetError::InvalidRequest)?;
    for line in text.lines() {
        let Some((candidate, count)) = line.trim().split_once(':') else {
            continue;
        };
        if candidate.len() == 35 && candidate.as_bytes().eq_ignore_ascii_case(suffix) {
            // Padding entries have a count of 0 and therefore read as "not listed".
            return Ok(count.trim().parse().unwrap_or(0));
        }
    }
    Ok(0)
}

pub fn check_password<T: Transport>(
    policy: &ProtectionNetworkPolicy,
    client: &NetClient<T>,
    password: Zeroizing<Vec<u8>>,
    observed_at: String,
) -> Result<PasswordBreachResult, NetError> {
    let capability = policy
        .capability(NetworkPurpose::PasswordBreachCheck)
        .ok_or(NetError::NotPermitted)?;
    if password.is_empty() || password.len() > MAX_PASSWORD_BYTES {
        return Err(NetError::InvalidRequest);
    }
    let hash = sha1_upper_hex(&password)?;
    drop(password);
    let prefix: [u8; 5] = hash[..5].try_into().map_err(|_| NetError::InvalidRequest)?;
    let body = client.fetch(&capability, Endpoint::PasswordRange(prefix))?;
    let count = occurrences(&body, &hash[5..])?;
    let detail = if count > 0 {
        format!("This password appears {count} times in known breach data. Do not use it.")
    } else {
        "This password was not found in the provider's breach data at this time.".into()
    };
    Ok(PasswordBreachResult {
        occurrences: count,
        evidence: Evidence::External {
            provider: PROVIDER.into(),
            observed_at,
            detail,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protection::net::fake::CountingTransport;

    const ON: ProtectionNetworkPolicy = ProtectionNetworkPolicy {
        rule_download: false,
        password_breach_check: true,
    };

    fn pw(text: &str) -> Zeroizing<Vec<u8>> {
        Zeroizing::new(text.as_bytes().to_vec())
    }

    #[test]
    fn cng_sha1_matches_known_vector() {
        // SHA-1("password") = 5BAA61E4C9B93F3F0682250B6CF8331B7EE68FD8
        assert_eq!(
            &*sha1_upper_hex(b"password").unwrap(),
            b"5BAA61E4C9B93F3F0682250B6CF8331B7EE68FD8"
        );
    }

    #[test]
    fn policy_off_sends_nothing() {
        let client = NetClient::new(CountingTransport::default());
        for policy in [
            ProtectionNetworkPolicy::default(),
            ProtectionNetworkPolicy {
                rule_download: true,
                password_breach_check: false,
            },
        ] {
            assert!(matches!(
                check_password(&policy, &client, pw("password"), "t".into()),
                Err(NetError::NotPermitted)
            ));
        }
        assert_eq!(client.transport().count(), 0);
    }

    #[test]
    fn only_the_five_character_prefix_leaves_the_device() {
        let body = b"1E4C9B93F3F0682250B6CF8331B7EE68FD8:9659365\r\n0018A45C4D1DEF81644B54AB7F969B88D65:0\r\n".to_vec();
        let client = NetClient::new(CountingTransport::with(vec![Ok(body)]));
        let result =
            check_password(&ON, &client, pw("password"), "2026-09-23T00:00:00Z".into()).unwrap();
        assert_eq!(result.occurrences, 9_659_365);
        assert!(
            matches!(result.evidence, Evidence::External { ref provider, .. } if provider == PROVIDER)
        );
        let requests = client.transport().requests.lock().unwrap().clone();
        assert_eq!(
            requests,
            vec![(
                "api.pwnedpasswords.com".into(),
                "/range/5BAA6".into(),
                "Add-Padding: true\r\n".into()
            )]
        );
    }

    #[test]
    fn padding_and_absent_suffixes_read_as_not_listed() {
        let body = b"1E4C9B93F3F0682250B6CF8331B7EE68FD8:0\r\n".to_vec();
        let client = NetClient::new(CountingTransport::with(vec![Ok(body), Ok(b"".to_vec())]));
        assert_eq!(
            check_password(&ON, &client, pw("password"), "t".into())
                .unwrap()
                .occurrences,
            0
        );
        assert_eq!(
            check_password(&ON, &client, pw("password"), "t".into())
                .unwrap()
                .occurrences,
            0
        );
    }

    #[test]
    fn network_failure_is_reported_not_treated_as_safe() {
        let client = NetClient::new(CountingTransport::with(vec![Err(NetError::Unreachable)]));
        assert!(matches!(
            check_password(&ON, &client, pw("password"), "t".into()),
            Err(NetError::Unreachable)
        ));
        assert!(matches!(
            check_password(&ON, &client, pw(""), "t".into()),
            Err(NetError::InvalidRequest)
        ));
    }
}
