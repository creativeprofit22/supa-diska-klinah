//! The only network sink in the application (ADR 0003, decision 6).
//!
//! * HTTPS only (TLS 1.2+), port 443, GET only.
//! * Hosts and paths come from the closed [`Endpoint`] enum, never from input.
//! * Redirects, cookies and automatic authentication are disabled.
//! * Every request needs a [`NetworkCapability`] minted from an enabled
//!   opt-in flag, and the capability's purpose must match the endpoint.
//! * Timeouts and response sizes are bounded.
//!
//! `scripts/check-architecture.mjs` rejects WinHTTP or HTTP clients anywhere else.

use protection_core::{NetworkCapability, NetworkPurpose};
use windows::Win32::Networking::WinHttp::{
    INTERNET_DEFAULT_HTTPS_PORT, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
    WINHTTP_DISABLE_AUTHENTICATION, WINHTTP_DISABLE_COOKIES, WINHTTP_FLAG_SECURE,
    WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_2, WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_3,
    WINHTTP_OPTION_DISABLE_FEATURE, WINHTTP_OPTION_REDIRECT_POLICY,
    WINHTTP_OPTION_REDIRECT_POLICY_NEVER, WINHTTP_OPTION_SECURE_PROTOCOLS,
    WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_STATUS_CODE, WinHttpCloseHandle, WinHttpConnect,
    WinHttpOpen, WinHttpOpenRequest, WinHttpQueryHeaders, WinHttpReadData, WinHttpReceiveResponse,
    WinHttpSendRequest, WinHttpSetOption, WinHttpSetTimeouts,
};
use windows::core::{PCWSTR, w};

/// Rule packs are published on a dedicated branch served directly by GitHub's
/// raw host. Release-asset URLs are not used because they always redirect.
pub const RULE_PACK_HOST: &str = "raw.githubusercontent.com";
pub const RULE_PACK_PATH: &str = "/creativeprofit22/supa-diska-klinah/rule-packs/pack.json";
pub const RULE_SIG_PATH: &str = "/creativeprofit22/supa-diska-klinah/rule-packs/pack.sig";
pub const PWNED_PASSWORDS_HOST: &str = "api.pwnedpasswords.com";

const RESOLVE_TIMEOUT_MS: i32 = 10_000;
const CONNECT_TIMEOUT_MS: i32 = 10_000;
const SEND_TIMEOUT_MS: i32 = 10_000;
const RECEIVE_TIMEOUT_MS: i32 = 20_000;

/// Every place the application may contact.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Endpoint {
    RulePack,
    RuleSignature,
    /// First five uppercase hex characters of a SHA-1.
    PasswordRange([u8; 5]),
}

impl Endpoint {
    pub fn purpose(&self) -> NetworkPurpose {
        match self {
            Self::RulePack | Self::RuleSignature => NetworkPurpose::RuleDownload,
            Self::PasswordRange(_) => NetworkPurpose::PasswordBreachCheck,
        }
    }

    pub fn host(&self) -> &'static str {
        match self {
            Self::RulePack | Self::RuleSignature => RULE_PACK_HOST,
            Self::PasswordRange(_) => PWNED_PASSWORDS_HOST,
        }
    }

    pub fn path(&self) -> Result<String, NetError> {
        match self {
            Self::RulePack => Ok(RULE_PACK_PATH.into()),
            Self::RuleSignature => Ok(RULE_SIG_PATH.into()),
            Self::PasswordRange(prefix) => {
                if !prefix
                    .iter()
                    .all(|b| matches!(b, b'0'..=b'9' | b'A'..=b'F'))
                {
                    return Err(NetError::InvalidRequest);
                }
                Ok(format!(
                    "/range/{}",
                    std::str::from_utf8(prefix).map_err(|_| NetError::InvalidRequest)?
                ))
            }
        }
    }

    /// Extra request headers. Padding makes every range response a similar
    /// size, so an observer cannot infer the prefix from response length.
    pub fn headers(&self) -> &'static str {
        match self {
            Self::PasswordRange(_) => "Add-Padding: true\r\n",
            _ => "",
        }
    }

    pub fn max_bytes(&self) -> usize {
        match self {
            Self::RulePack => protection_core::MAX_PACK_BYTES,
            Self::RuleSignature => 256,
            Self::PasswordRange(_) => 2 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum NetError {
    /// The matching opt-in flag is off.
    NotPermitted,
    InvalidRequest,
    Unreachable,
    Status(u32),
    TooLarge,
}

impl std::fmt::Display for NetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotPermitted => f.write_str("this network feature is turned off"),
            Self::InvalidRequest => f.write_str("invalid network request"),
            Self::Unreachable => f.write_str("the service could not be reached"),
            Self::Status(code) => write!(f, "the service answered with HTTP {code}"),
            Self::TooLarge => f.write_str("the response was larger than allowed"),
        }
    }
}

/// Performs one HTTPS GET. Implemented by WinHTTP in production and by
/// counting fakes in tests.
pub trait Transport: Send + Sync {
    fn get(
        &self,
        host: &'static str,
        path: &str,
        headers: &'static str,
        max_bytes: usize,
    ) -> Result<Vec<u8>, NetError>;
}

/// Capability-checked front door to a transport.
pub struct NetClient<T: Transport> {
    transport: T,
}

impl<T: Transport> NetClient<T> {
    pub fn new(transport: T) -> Self {
        Self { transport }
    }

    pub fn transport(&self) -> &T {
        &self.transport
    }

    pub fn fetch(
        &self,
        capability: &NetworkCapability,
        endpoint: Endpoint,
    ) -> Result<Vec<u8>, NetError> {
        if capability.purpose() != endpoint.purpose() {
            return Err(NetError::NotPermitted);
        }
        let path = endpoint.path()?;
        self.transport.get(
            endpoint.host(),
            &path,
            endpoint.headers(),
            endpoint.max_bytes(),
        )
    }
}

/// WinHTTP transport.
#[derive(Default)]
pub struct SystemTransport;

struct Handle(*mut core::ffi::c_void);
impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: each handle is closed exactly once, children before parents
            // (declaration order in `get` guarantees reverse-drop order).
            let _ = unsafe { WinHttpCloseHandle(self.0) };
        }
    }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

impl Transport for SystemTransport {
    fn get(
        &self,
        host: &'static str,
        path: &str,
        headers: &'static str,
        max_bytes: usize,
    ) -> Result<Vec<u8>, NetError> {
        if !path.starts_with('/') || !path.bytes().all(|b| b.is_ascii_graphic()) {
            return Err(NetError::InvalidRequest);
        }
        let host_w = wide(host);
        let path_w = wide(path);
        // SAFETY: all strings are NUL-terminated and outlive the calls; every
        // handle is owned by a `Handle` guard; buffers are sized as passed.
        unsafe {
            let session = Handle(WinHttpOpen(
                w!("SupaDiskaKlinah-Protection/0.1"),
                WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
                PCWSTR::null(),
                PCWSTR::null(),
                0,
            ));
            if session.0.is_null() {
                return Err(NetError::Unreachable);
            }
            WinHttpSetTimeouts(
                session.0,
                RESOLVE_TIMEOUT_MS,
                CONNECT_TIMEOUT_MS,
                SEND_TIMEOUT_MS,
                RECEIVE_TIMEOUT_MS,
            )
            .map_err(|_| NetError::Unreachable)?;
            let redirect = WINHTTP_OPTION_REDIRECT_POLICY_NEVER.to_le_bytes();
            WinHttpSetOption(
                Some(session.0),
                WINHTTP_OPTION_REDIRECT_POLICY,
                Some(&redirect),
            )
            .map_err(|_| NetError::Unreachable)?;
            let modern = (WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_2
                | WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_3)
                .to_le_bytes();
            if WinHttpSetOption(
                Some(session.0),
                WINHTTP_OPTION_SECURE_PROTOCOLS,
                Some(&modern),
            )
            .is_err()
            {
                // Older Windows 10 builds lack TLS 1.3 in WinHTTP.
                let tls12 = WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_2.to_le_bytes();
                WinHttpSetOption(
                    Some(session.0),
                    WINHTTP_OPTION_SECURE_PROTOCOLS,
                    Some(&tls12),
                )
                .map_err(|_| NetError::Unreachable)?;
            }
            let connect = Handle(WinHttpConnect(
                session.0,
                PCWSTR(host_w.as_ptr()),
                INTERNET_DEFAULT_HTTPS_PORT,
                0,
            ));
            if connect.0.is_null() {
                return Err(NetError::Unreachable);
            }
            let request = Handle(WinHttpOpenRequest(
                connect.0,
                w!("GET"),
                PCWSTR(path_w.as_ptr()),
                PCWSTR::null(),
                PCWSTR::null(),
                std::ptr::null(),
                WINHTTP_FLAG_SECURE,
            ));
            if request.0.is_null() {
                return Err(NetError::Unreachable);
            }
            let disable = (WINHTTP_DISABLE_COOKIES | WINHTTP_DISABLE_AUTHENTICATION).to_le_bytes();
            WinHttpSetOption(
                Some(request.0),
                WINHTTP_OPTION_DISABLE_FEATURE,
                Some(&disable),
            )
            .map_err(|_| NetError::Unreachable)?;
            let header_w: Vec<u16> = headers.encode_utf16().collect();
            let header_arg = (!header_w.is_empty()).then_some(header_w.as_slice());
            WinHttpSendRequest(request.0, header_arg, None, 0, 0, 0)
                .map_err(|_| NetError::Unreachable)?;
            WinHttpReceiveResponse(request.0, std::ptr::null_mut())
                .map_err(|_| NetError::Unreachable)?;

            let mut status = 0_u32;
            let mut size = size_of::<u32>() as u32;
            WinHttpQueryHeaders(
                request.0,
                WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
                PCWSTR::null(),
                Some((&raw mut status).cast()),
                &mut size,
                std::ptr::null_mut(),
            )
            .map_err(|_| NetError::Unreachable)?;
            if status != 200 {
                return Err(NetError::Status(status));
            }

            let mut body = Vec::new();
            let mut chunk = vec![0_u8; 16 * 1024];
            loop {
                let mut read = 0_u32;
                WinHttpReadData(
                    request.0,
                    chunk.as_mut_ptr().cast(),
                    chunk.len() as u32,
                    &mut read,
                )
                .map_err(|_| NetError::Unreachable)?;
                if read == 0 {
                    break;
                }
                if body.len() + read as usize > max_bytes {
                    return Err(NetError::TooLarge);
                }
                body.extend_from_slice(&chunk[..read as usize]);
            }
            Ok(body)
        }
    }
}

#[cfg(test)]
pub(crate) mod fake {
    use super::*;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Records every request and replays scripted responses.
    #[derive(Default)]
    pub(crate) struct CountingTransport {
        pub calls: AtomicUsize,
        pub requests: Mutex<Vec<(String, String, String)>>,
        pub responses: Mutex<Vec<Result<Vec<u8>, NetError>>>,
    }

    impl CountingTransport {
        pub(crate) fn with(responses: Vec<Result<Vec<u8>, NetError>>) -> Self {
            let mut responses = responses;
            responses.reverse();
            Self {
                responses: Mutex::new(responses),
                ..Self::default()
            }
        }
        pub(crate) fn count(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    impl Transport for CountingTransport {
        fn get(
            &self,
            host: &'static str,
            path: &str,
            headers: &'static str,
            max_bytes: usize,
        ) -> Result<Vec<u8>, NetError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.requests
                .lock()
                .unwrap()
                .push((host.into(), path.into(), headers.into()));
            let response = self
                .responses
                .lock()
                .unwrap()
                .pop()
                .unwrap_or(Err(NetError::Unreachable));
            response.and_then(|body| {
                if body.len() > max_bytes {
                    Err(NetError::TooLarge)
                } else {
                    Ok(body)
                }
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake::CountingTransport;
    use super::*;
    use protection_core::ProtectionNetworkPolicy;

    #[test]
    fn capability_purpose_must_match_endpoint() {
        let client = NetClient::new(CountingTransport::default());
        let policy = ProtectionNetworkPolicy {
            rule_download: true,
            password_breach_check: false,
        };
        let cap = policy.capability(NetworkPurpose::RuleDownload).unwrap();
        assert_eq!(
            client.fetch(&cap, Endpoint::PasswordRange(*b"ABCDE")),
            Err(NetError::NotPermitted)
        );
        assert_eq!(client.transport().count(), 0);
    }

    #[test]
    fn endpoints_are_fixed_https_hosts_with_validated_paths() {
        assert_eq!(Endpoint::RulePack.host(), "raw.githubusercontent.com");
        assert_eq!(
            Endpoint::PasswordRange(*b"0A1B2").path().unwrap(),
            "/range/0A1B2"
        );
        assert_eq!(
            Endpoint::PasswordRange(*b"../..").path(),
            Err(NetError::InvalidRequest)
        );
        assert_eq!(
            Endpoint::PasswordRange(*b"abcde").path(),
            Err(NetError::InvalidRequest)
        );
        assert_eq!(
            Endpoint::PasswordRange(*b"ABCDE").headers(),
            "Add-Padding: true\r\n"
        );
    }

    #[test]
    fn winhttp_transport_rejects_non_path_input_before_any_io() {
        let transport = SystemTransport;
        assert_eq!(
            transport.get(RULE_PACK_HOST, "https://evil.example/", "", 10),
            Err(NetError::InvalidRequest)
        );
        assert_eq!(
            transport.get(RULE_PACK_HOST, "/a b", "", 10),
            Err(NetError::InvalidRequest)
        );
    }
}
