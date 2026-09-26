//! The only network sink in the application (ADR 0003, decision 6).
//!
//! * HTTPS only (TLS 1.2+), port 443, GET only.
//! * Hosts and paths come from the closed [`Endpoint`] enum, never from input.
//! * Redirects, cookies and automatic authentication are disabled. The one
//!   exception is the app-update installer download: GitHub release assets
//!   always redirect, so that endpoint alone may follow exactly one redirect,
//!   HTTPS only, to an exact host allowlist ([`RELEASE_ASSET_HOSTS`]).
//! * Every request needs a [`NetworkCapability`] minted from an enabled
//!   opt-in flag, and the capability's purpose must match the endpoint.
//! * Timeouts and response sizes are bounded.
//!
//! `scripts/check-architecture.mjs` rejects WinHTTP or HTTP clients anywhere else.

use protection_core::{AppVersion, NetworkCapability, NetworkPurpose};
use windows::Win32::Networking::WinHttp::{
    INTERNET_DEFAULT_HTTPS_PORT, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
    WINHTTP_DISABLE_AUTHENTICATION, WINHTTP_DISABLE_COOKIES, WINHTTP_FLAG_SECURE,
    WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_2, WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_3,
    WINHTTP_OPTION_DISABLE_FEATURE, WINHTTP_OPTION_REDIRECT_POLICY,
    WINHTTP_OPTION_REDIRECT_POLICY_NEVER, WINHTTP_OPTION_SECURE_PROTOCOLS,
    WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_LOCATION, WINHTTP_QUERY_STATUS_CODE,
    WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest, WinHttpQueryHeaders,
    WinHttpReadData, WinHttpReceiveResponse, WinHttpSendRequest, WinHttpSetOption,
    WinHttpSetTimeouts,
};
use windows::core::{PCWSTR, w};

/// Rule packs are published on a dedicated branch served directly by GitHub's
/// raw host. Release-asset URLs are not used because they always redirect.
pub const RULE_PACK_HOST: &str = "raw.githubusercontent.com";
pub const RULE_PACK_PATH: &str = "/creativeprofit22/supa-diska-klinah/rule-packs/pack.json";
pub const RULE_SIG_PATH: &str = "/creativeprofit22/supa-diska-klinah/rule-packs/pack.sig";
pub const PWNED_PASSWORDS_HOST: &str = "api.pwnedpasswords.com";
/// The signed update manifest lives on a dedicated branch, served without redirects.
pub const UPDATE_MANIFEST_HOST: &str = "raw.githubusercontent.com";
pub const UPDATE_MANIFEST_PATH: &str = "/creativeprofit22/supa-diska-klinah/updates/update.json";
pub const UPDATE_SIG_PATH: &str = "/creativeprofit22/supa-diska-klinah/updates/update.json.sig";
pub const UPDATE_MANIFEST_MAX_BYTES: usize = 16 * 1024;
/// Installers are downloaded from the release page, which redirects once to GitHub's asset CDN.
pub const RELEASE_HOST: &str = "github.com";
pub const RELEASE_DOWNLOAD_PREFIX: &str = "/creativeprofit22/supa-diska-klinah/releases/download/";
/// The only hosts a release download may redirect to.
pub const RELEASE_ASSET_HOSTS: &[&str] = &[
    "release-assets.githubusercontent.com",
    "objects.githubusercontent.com",
];
/// Upper bound for any installer, whatever the manifest claims.
pub const MAX_INSTALLER_BYTES: u64 = 512 * 1024 * 1024;
/// Longest redirect target accepted (GitHub's signed asset URLs are ~1 KB).
const MAX_REDIRECT_CHARS: usize = 4_096;

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
    UpdateManifest,
    UpdateSignature,
    /// The NSIS installer for one release; the name is derived from the version.
    UpdateInstaller(AppVersion),
}

impl Endpoint {
    pub fn purpose(&self) -> NetworkPurpose {
        match self {
            Self::RulePack | Self::RuleSignature => NetworkPurpose::RuleDownload,
            Self::PasswordRange(_) => NetworkPurpose::PasswordBreachCheck,
            Self::UpdateManifest | Self::UpdateSignature | Self::UpdateInstaller(_) => {
                NetworkPurpose::UpdateCheck
            }
        }
    }

    pub fn host(&self) -> &'static str {
        match self {
            Self::RulePack | Self::RuleSignature => RULE_PACK_HOST,
            Self::PasswordRange(_) => PWNED_PASSWORDS_HOST,
            Self::UpdateManifest | Self::UpdateSignature => UPDATE_MANIFEST_HOST,
            Self::UpdateInstaller(_) => RELEASE_HOST,
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
            Self::UpdateManifest => Ok(UPDATE_MANIFEST_PATH.into()),
            Self::UpdateSignature => Ok(UPDATE_SIG_PATH.into()),
            Self::UpdateInstaller(version) => Ok(format!(
                "{RELEASE_DOWNLOAD_PREFIX}v{version}/{}",
                version.installer_name()
            )),
        }
    }

    /// Hosts this endpoint may be redirected to (at most once). Empty means
    /// redirects are refused, which is every endpoint except the installer.
    pub fn redirect_hosts(&self) -> &'static [&'static str] {
        match self {
            Self::UpdateInstaller(_) => RELEASE_ASSET_HOSTS,
            _ => &[],
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
            Self::UpdateManifest => UPDATE_MANIFEST_MAX_BYTES,
            Self::UpdateSignature => 256,
            // Streamed to disk through `NetClient::download` with the manifest's size.
            Self::UpdateInstaller(_) => 0,
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
    /// A redirect pointed outside the allowlist, was not HTTPS, or chained.
    BadRedirect,
    /// Writing the downloaded bytes locally failed.
    Storage,
}

impl std::fmt::Display for NetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotPermitted => f.write_str("this network feature is turned off"),
            Self::InvalidRequest => f.write_str("invalid network request"),
            Self::Unreachable => f.write_str("the service could not be reached"),
            Self::Status(code) => write!(f, "the service answered with HTTP {code}"),
            Self::TooLarge => f.write_str("the response was larger than allowed"),
            Self::BadRedirect => f.write_str("the download was redirected somewhere unexpected"),
            Self::Storage => f.write_str("the download could not be saved"),
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

    /// Streams one HTTPS GET into `sink`, following at most one redirect to
    /// an exact host in `redirect_hosts`. Returns the number of bytes written.
    /// Transports that do not support downloads refuse.
    fn download(
        &self,
        _host: &'static str,
        _path: &str,
        _redirect_hosts: &'static [&'static str],
        _max_bytes: u64,
        _sink: &mut dyn std::io::Write,
    ) -> Result<u64, NetError> {
        Err(NetError::NotPermitted)
    }
}

/// Validates a redirect `Location`: absolute `https://` URL, host exactly in
/// the allowlist (no port, no user info), graphic ASCII path. Returns the
/// allowlisted host and the path-and-query to request.
pub fn parse_redirect(
    location: &str,
    allowed: &'static [&'static str],
) -> Result<(&'static str, String), NetError> {
    if location.len() > MAX_REDIRECT_CHARS || !location.bytes().all(|b| b.is_ascii_graphic()) {
        return Err(NetError::BadRedirect);
    }
    let rest = location
        .strip_prefix("https://")
        .ok_or(NetError::BadRedirect)?;
    let split = rest.find('/').ok_or(NetError::BadRedirect)?;
    let (authority, path) = rest.split_at(split);
    let host = allowed
        .iter()
        .find(|candidate| candidate.eq_ignore_ascii_case(authority))
        .ok_or(NetError::BadRedirect)?;
    if path.starts_with("//") || path.contains('#') || path.contains('\\') {
        return Err(NetError::BadRedirect);
    }
    Ok((host, path.to_owned()))
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
        if !endpoint.redirect_hosts().is_empty() {
            // Redirecting endpoints are streamed with `download`.
            return Err(NetError::InvalidRequest);
        }
        let path = endpoint.path()?;
        self.transport.get(
            endpoint.host(),
            &path,
            endpoint.headers(),
            endpoint.max_bytes(),
        )
    }

    /// Streams a large download (the update installer) to `sink`, capped at
    /// `max_bytes`. Only endpoints with a redirect allowlist may be downloaded.
    pub fn download(
        &self,
        capability: &NetworkCapability,
        endpoint: Endpoint,
        max_bytes: u64,
        sink: &mut dyn std::io::Write,
    ) -> Result<u64, NetError> {
        if capability.purpose() != endpoint.purpose() {
            return Err(NetError::NotPermitted);
        }
        if endpoint.redirect_hosts().is_empty() || max_bytes == 0 || max_bytes > MAX_INSTALLER_BYTES
        {
            return Err(NetError::InvalidRequest);
        }
        let path = endpoint.path()?;
        self.transport.download(
            endpoint.host(),
            &path,
            endpoint.redirect_hosts(),
            max_bytes,
            sink,
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
            // (`Exchange` field order guarantees children drop before parents).
            let _ = unsafe { WinHttpCloseHandle(self.0) };
        }
    }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// One open WinHTTP request. Fields drop in declaration order: request,
/// then connection, then session (children before parents).
struct Exchange {
    request: Handle,
    _connect: Handle,
    _session: Handle,
    status: u32,
}

fn valid_path(path: &str) -> bool {
    path.starts_with('/') && path.bytes().all(|b| b.is_ascii_graphic())
}

fn is_redirect(status: u32) -> bool {
    matches!(status, 301 | 302 | 303 | 307 | 308)
}

impl SystemTransport {
    /// Sends one GET with redirects disabled and returns the open exchange.
    fn exchange(&self, host: &str, path: &str, headers: &str) -> Result<Exchange, NetError> {
        if !valid_path(path) {
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
            Ok(Exchange {
                request,
                _connect: connect,
                _session: session,
                status,
            })
        }
    }

    fn location(exchange: &Exchange) -> Result<String, NetError> {
        let mut buffer = vec![0_u16; MAX_REDIRECT_CHARS + 1];
        let mut size = (buffer.len() * size_of::<u16>()) as u32;
        // SAFETY: `size` is the buffer's byte length; WinHTTP writes at most that.
        unsafe {
            WinHttpQueryHeaders(
                exchange.request.0,
                WINHTTP_QUERY_LOCATION,
                PCWSTR::null(),
                Some(buffer.as_mut_ptr().cast()),
                &mut size,
                std::ptr::null_mut(),
            )
        }
        .map_err(|_| NetError::BadRedirect)?;
        let units = (size as usize / size_of::<u16>()).min(buffer.len());
        String::from_utf16(&buffer[..units]).map_err(|_| NetError::BadRedirect)
    }

    /// Streams the body to `sink`, failing as soon as more than `max_bytes` arrive.
    fn read_body(
        exchange: &Exchange,
        max_bytes: u64,
        sink: &mut dyn std::io::Write,
    ) -> Result<u64, NetError> {
        let mut total = 0_u64;
        let mut chunk = vec![0_u8; 64 * 1024];
        loop {
            let mut read = 0_u32;
            // SAFETY: the chunk is valid for `chunk.len()` bytes.
            unsafe {
                WinHttpReadData(
                    exchange.request.0,
                    chunk.as_mut_ptr().cast(),
                    chunk.len() as u32,
                    &mut read,
                )
            }
            .map_err(|_| NetError::Unreachable)?;
            if read == 0 {
                return Ok(total);
            }
            total += u64::from(read);
            if total > max_bytes {
                return Err(NetError::TooLarge);
            }
            sink.write_all(&chunk[..read as usize])
                .map_err(|_| NetError::Storage)?;
        }
    }
}

impl Transport for SystemTransport {
    fn get(
        &self,
        host: &'static str,
        path: &str,
        headers: &'static str,
        max_bytes: usize,
    ) -> Result<Vec<u8>, NetError> {
        let exchange = self.exchange(host, path, headers)?;
        if exchange.status != 200 {
            return Err(NetError::Status(exchange.status));
        }
        let mut body = Vec::new();
        Self::read_body(&exchange, max_bytes as u64, &mut body)?;
        Ok(body)
    }

    fn download(
        &self,
        host: &'static str,
        path: &str,
        redirect_hosts: &'static [&'static str],
        max_bytes: u64,
        sink: &mut dyn std::io::Write,
    ) -> Result<u64, NetError> {
        let first = self.exchange(host, path, "")?;
        let exchange = if is_redirect(first.status) {
            let (next_host, next_path) = parse_redirect(&Self::location(&first)?, redirect_hosts)?;
            drop(first);
            let second = self.exchange(next_host, &next_path, "")?;
            // Exactly one hop is allowed.
            if is_redirect(second.status) {
                return Err(NetError::BadRedirect);
            }
            second
        } else {
            first
        };
        if exchange.status != 200 {
            return Err(NetError::Status(exchange.status));
        }
        Self::read_body(&exchange, max_bytes, sink)
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
        /// When set, `download` writes this many bytes, then drops the connection.
        pub interrupt_after: Mutex<Option<usize>>,
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

        fn download(
            &self,
            host: &'static str,
            path: &str,
            _redirect_hosts: &'static [&'static str],
            max_bytes: u64,
            sink: &mut dyn std::io::Write,
        ) -> Result<u64, NetError> {
            let body = self.get(host, path, "", usize::MAX)?;
            if body.len() as u64 > max_bytes {
                return Err(NetError::TooLarge);
            }
            if let Some(cut) = *self.interrupt_after.lock().unwrap() {
                sink.write_all(&body[..cut.min(body.len())])
                    .map_err(|_| NetError::Storage)?;
                return Err(NetError::Unreachable);
            }
            sink.write_all(&body).map_err(|_| NetError::Storage)?;
            Ok(body.len() as u64)
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

    fn version(text: &str) -> AppVersion {
        AppVersion::parse(text).unwrap()
    }

    #[test]
    fn update_endpoints_are_fixed_and_need_the_update_capability() {
        assert_eq!(Endpoint::UpdateManifest.host(), "raw.githubusercontent.com");
        assert_eq!(
            Endpoint::UpdateManifest.path().unwrap(),
            "/creativeprofit22/supa-diska-klinah/updates/update.json"
        );
        assert_eq!(
            Endpoint::UpdateInstaller(version("1.2.3")).path().unwrap(),
            "/creativeprofit22/supa-diska-klinah/releases/download/v1.2.3/Supa-Diska-Klinah_1.2.3_x64-setup.exe"
        );
        assert_eq!(
            Endpoint::UpdateInstaller(version("1.2.3")).host(),
            "github.com"
        );
        assert!(Endpoint::UpdateManifest.redirect_hosts().is_empty());
        assert!(Endpoint::RulePack.redirect_hosts().is_empty());

        let client = NetClient::new(CountingTransport::default());
        let rules = ProtectionNetworkPolicy {
            rule_download: true,
            password_breach_check: true,
        };
        let cap = rules.capability(NetworkPurpose::RuleDownload).unwrap();
        assert_eq!(
            client.fetch(&cap, Endpoint::UpdateManifest),
            Err(NetError::NotPermitted)
        );
        let update = protection_core::UpdateCheckPolicy { enabled: true }
            .capability()
            .unwrap();
        assert_eq!(
            client.fetch(&update, Endpoint::RulePack),
            Err(NetError::NotPermitted)
        );
        assert_eq!(
            client.fetch(&update, Endpoint::UpdateInstaller(version("1.0.0"))),
            Err(NetError::InvalidRequest)
        );
        let mut sink = Vec::new();
        assert_eq!(
            client.download(&update, Endpoint::UpdateManifest, 10, &mut sink),
            Err(NetError::InvalidRequest)
        );
        assert_eq!(
            client.download(
                &update,
                Endpoint::UpdateInstaller(version("1.0.0")),
                MAX_INSTALLER_BYTES + 1,
                &mut sink
            ),
            Err(NetError::InvalidRequest)
        );
        assert_eq!(client.transport().count(), 0);
    }

    #[test]
    fn redirects_are_limited_to_exact_https_asset_hosts() {
        let ok = parse_redirect(
            "https://release-assets.githubusercontent.com/github-production-release-asset/1/abc?sp=r&sig=x%2B",
            RELEASE_ASSET_HOSTS,
        )
        .unwrap();
        assert_eq!(ok.0, "release-assets.githubusercontent.com");
        assert!(ok.1.starts_with("/github-production-release-asset/1/abc?"));
        assert_eq!(
            parse_redirect(
                "https://objects.githubusercontent.com/x",
                RELEASE_ASSET_HOSTS
            )
            .unwrap()
            .0,
            "objects.githubusercontent.com"
        );
        for bad in [
            "http://release-assets.githubusercontent.com/x",
            "https://evil.example/x",
            "https://release-assets.githubusercontent.com.evil.example/x",
            "https://release-assets.githubusercontent.com:8443/x",
            "https://user@release-assets.githubusercontent.com/x",
            "https://release-assets.githubusercontent.com",
            "https://objects.githubusercontent.com//evil.example/x",
            "//release-assets.githubusercontent.com/x",
            "/relative/path",
            "https://release-assets.githubusercontent.com/a b",
            "https://release-assets.githubusercontent.com/a#frag",
        ] {
            assert_eq!(
                parse_redirect(bad, RELEASE_ASSET_HOSTS),
                Err(NetError::BadRedirect),
                "{bad}"
            );
        }
        let long = format!(
            "https://objects.githubusercontent.com/{}",
            "a".repeat(MAX_REDIRECT_CHARS)
        );
        assert_eq!(
            parse_redirect(&long, RELEASE_ASSET_HOSTS),
            Err(NetError::BadRedirect)
        );
        assert_eq!(
            parse_redirect("https://objects.githubusercontent.com/x", &[]),
            Err(NetError::BadRedirect)
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
