//! Hosts file review, tamper check, and reversible line edits.
//!
//! The only file this module touches is `GetSystemDirectoryW()` +
//! `\drivers\etc\hosts`. Content is handled as bytes split on `\n`; every
//! untouched line (including its `\r`) and the final-newline state round-trip
//! byte-for-byte whatever the encoding. Edits only ever add or strip the
//! [`DISABLED_PREFIX`] at the start of a line, so line indices never shift.

pub mod elevated;
#[cfg(test)]
mod tests;

use std::{
    ffi::OsString,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    net::IpAddr,
    os::windows::{
        ffi::{OsStrExt, OsStringExt},
        fs::MetadataExt,
    },
    path::{Path, PathBuf},
    ptr,
    time::{SystemTime, UNIX_EPOCH},
};

use cleanup_core::system_change::{
    HostsLineAction, HostsLineOp, ImpactSummary, PriorState, RestartRequirement, RiskLevel,
    Sha256Digest, SystemChange, UnsupportedReason,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use windows_sys::Win32::{
    Foundation::ERROR_ACCESS_DENIED,
    Storage::FileSystem::{
        FILE_ATTRIBUTE_REPARSE_POINT, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
        MoveFileExW,
    },
    System::{Com::CoTaskMemFree, SystemInformation::GetSystemDirectoryW},
    UI::Shell::{FOLDERID_ProgramData, SHGetKnownFolderPath},
};

use crate::system_change::{AdapterError, SystemAdapter};

pub const MAX_HOSTS_BYTES: usize = 1024 * 1024;
pub const MAX_REPORT_LINES: usize = 10_000;
pub const MAX_TEXT_CHARS: usize = 512;
pub const MAX_FINDINGS: usize = 1_000;
pub const KEEP_BACKUPS: usize = 20;
/// Prefix this application adds to comment out a mapping.
pub const DISABLED_PREFIX: &[u8] = b"#sdk-disabled# ";

const UTF8_BOM: &[u8] = b"\xEF\xBB\xBF";
const MAX_HOSTNAMES_PER_LINE: usize = 64;
const MAX_BACKUP_DIR_ENTRIES: usize = 10_000;

/// Domains whose redirection to a real address is a strong tamper signal.
/// Subdomains match too.
pub const SENSITIVE_DOMAINS: &[&str] = &[
    "microsoft.com",
    "windowsupdate.com",
    "update.microsoft.com",
    "download.windowsupdate.com",
    "live.com",
    "login.microsoftonline.com",
    "google.com",
    "facebook.com",
    "paypal.com",
    "apple.com",
    "amazon.com",
    "avast.com",
    "avg.com",
    "kaspersky.com",
    "eset.com",
    "norton.com",
    "mcafee.com",
    "malwarebytes.com",
    "bitdefender.com",
    "sophos.com",
];

/// Security and update domains whose blocking (loopback / unspecified) is a
/// tamper signal. Subdomains match too.
pub const SECURITY_DOMAINS: &[&str] = &[
    "windowsupdate.com",
    "update.microsoft.com",
    "download.windowsupdate.com",
    "avast.com",
    "avg.com",
    "kaspersky.com",
    "eset.com",
    "norton.com",
    "mcafee.com",
    "malwarebytes.com",
    "bitdefender.com",
    "sophos.com",
];

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum HostsError {
    SystemDirectory,
    NotFound,
    Denied,
    TooLarge,
    Io,
    StateChanged,
}

impl From<HostsError> for AdapterError {
    fn from(error: HostsError) -> Self {
        match error {
            HostsError::SystemDirectory => Self::Unsupported(UnsupportedReason::ApiUnavailable),
            HostsError::NotFound => Self::Unsupported(UnsupportedReason::NotPresent),
            HostsError::Denied => Self::Denied,
            HostsError::TooLarge | HostsError::Io | HostsError::StateChanged => Self::Failed,
        }
    }
}

fn io_error(error: io::Error) -> HostsError {
    if error.raw_os_error() == Some(ERROR_ACCESS_DENIED as i32)
        || error.kind() == io::ErrorKind::PermissionDenied
    {
        HostsError::Denied
    } else if error.kind() == io::ErrorKind::NotFound {
        HostsError::NotFound
    } else {
        HostsError::Io
    }
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum HostsLineKind {
    Blank,
    Comment,
    Mapping,
    AppDisabled,
    Invalid,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum HostsLine {
    Blank,
    Comment,
    Mapping { ip: IpAddr, hostnames: Vec<String> },
    AppDisabled { ip: IpAddr, hostnames: Vec<String> },
    Invalid,
}

impl HostsLine {
    fn kind(&self) -> HostsLineKind {
        match self {
            Self::Blank => HostsLineKind::Blank,
            Self::Comment => HostsLineKind::Comment,
            Self::Mapping { .. } => HostsLineKind::Mapping,
            Self::AppDisabled { .. } => HostsLineKind::AppDisabled,
            Self::Invalid => HostsLineKind::Invalid,
        }
    }
}

/// Byte-exact view of a hosts file: lines without their `\n` (a `\r` stays
/// in the line) plus whether the file ended with `\n`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct HostsDocument {
    lines: Vec<Vec<u8>>,
    trailing_newline: bool,
}

impl HostsDocument {
    pub(crate) fn parse(bytes: &[u8]) -> Self {
        if bytes.is_empty() {
            return Self {
                lines: Vec::new(),
                trailing_newline: false,
            };
        }
        let mut lines: Vec<Vec<u8>> = bytes
            .split(|byte| *byte == b'\n')
            .map(<[u8]>::to_vec)
            .collect();
        let trailing_newline = bytes.ends_with(b"\n");
        if trailing_newline {
            lines.pop();
        }
        Self {
            lines,
            trailing_newline,
        }
    }

    pub(crate) fn to_bytes(&self) -> Vec<u8> {
        let mut out = self.lines.join(&b'\n');
        if self.trailing_newline {
            out.push(b'\n');
        }
        out
    }

    pub(crate) fn line_count(&self) -> usize {
        self.lines.len()
    }

    pub(crate) fn classify(&self, index: usize) -> Option<HostsLine> {
        self.lines.get(index).map(|raw| classify_line(index, raw))
    }
}

/// Byte offset past a UTF-8 BOM on the first line.
fn body_offset(index: usize, raw: &[u8]) -> usize {
    if index == 0 && raw.starts_with(UTF8_BOM) {
        UTF8_BOM.len()
    } else {
        0
    }
}

fn classify_line(index: usize, raw: &[u8]) -> HostsLine {
    let body = &raw[body_offset(index, raw)..];
    if let Some(rest) = body.strip_prefix(DISABLED_PREFIX) {
        return match parse_mapping(rest) {
            Some((ip, hostnames)) => HostsLine::AppDisabled { ip, hostnames },
            None => HostsLine::Comment,
        };
    }
    let trimmed = body.trim_ascii();
    if trimmed.is_empty() {
        return HostsLine::Blank;
    }
    if trimmed.starts_with(b"#") {
        return HostsLine::Comment;
    }
    match parse_mapping(body) {
        Some((ip, hostnames)) => HostsLine::Mapping { ip, hostnames },
        None => HostsLine::Invalid,
    }
}

fn parse_mapping(bytes: &[u8]) -> Option<(IpAddr, Vec<String>)> {
    let content = bytes.split(|byte| *byte == b'#').next()?;
    let text = std::str::from_utf8(content).ok()?;
    let mut tokens = text.split_ascii_whitespace();
    let ip: IpAddr = tokens.next()?.parse().ok()?;
    let hostnames: Vec<String> = tokens.map(str::to_ascii_lowercase).collect();
    if hostnames.is_empty()
        || hostnames.len() > MAX_HOSTNAMES_PER_LINE
        || !hostnames.iter().all(|name| valid_hostname(name))
    {
        return None;
    }
    Some((ip, hostnames))
}

fn valid_hostname(name: &str) -> bool {
    (1..=253).contains(&name.len())
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_'))
        && name.bytes().any(|byte| byte != b'.')
}

fn matches_domain(host: &str, domains: &[&str]) -> bool {
    let host = host.trim_end_matches('.');
    domains.iter().any(|domain| {
        host == *domain
            || (host.len() > domain.len()
                && host.ends_with(domain)
                && host.as_bytes()[host.len() - domain.len() - 1] == b'.')
    })
}

// ---------------------------------------------------------------------------
// Findings and report
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FindingSeverity {
    Low,
    High,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FindingKind {
    Redirect,
    SecurityBlocked,
    CustomMapping,
    Invalid,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostsFinding {
    pub line: u32,
    pub severity: FindingSeverity,
    pub kind: FindingKind,
    pub detail: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostsLineView {
    pub index: u32,
    pub kind: HostsLineKind,
    pub text: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostsReport {
    pub path: String,
    pub sha256: String,
    pub size_bytes: u64,
    pub total_lines: u32,
    pub lines_truncated: bool,
    pub lines: Vec<HostsLineView>,
    pub findings: Vec<HostsFinding>,
}

fn bounded(text: &str) -> String {
    text.chars().take(MAX_TEXT_CHARS).collect()
}

fn describe_hosts(hosts: &[&str], ip: &IpAddr) -> String {
    let shown: Vec<&str> = hosts.iter().take(3).copied().collect();
    let more = hosts.len().saturating_sub(shown.len());
    let suffix = if more > 0 {
        format!(" and {more} more")
    } else {
        String::new()
    };
    bounded(&format!("{}{suffix} -> {ip}", shown.join(", ")))
}

pub(crate) fn findings(document: &HostsDocument) -> Vec<HostsFinding> {
    let mut findings = Vec::new();
    for index in 0..document.line_count() {
        if findings.len() >= MAX_FINDINGS {
            break;
        }
        let line = index as u32;
        match document.classify(index) {
            Some(HostsLine::Mapping { ip, hostnames }) => {
                let blocking = ip.is_loopback() || ip.is_unspecified();
                let (mut redirect, mut blocked, mut custom) = (Vec::new(), Vec::new(), Vec::new());
                for host in &hostnames {
                    if blocking {
                        if matches_domain(host, SECURITY_DOMAINS) {
                            blocked.push(host.as_str());
                        }
                    } else if matches_domain(host, SENSITIVE_DOMAINS) {
                        redirect.push(host.as_str());
                    } else {
                        custom.push(host.as_str());
                    }
                }
                for (hosts, severity, kind) in [
                    (redirect, FindingSeverity::High, FindingKind::Redirect),
                    (blocked, FindingSeverity::High, FindingKind::SecurityBlocked),
                    (custom, FindingSeverity::Low, FindingKind::CustomMapping),
                ] {
                    if !hosts.is_empty() {
                        findings.push(HostsFinding {
                            line,
                            severity,
                            kind,
                            detail: describe_hosts(&hosts, &ip),
                        });
                    }
                }
            }
            Some(HostsLine::Invalid) => findings.push(HostsFinding {
                line,
                severity: FindingSeverity::Low,
                kind: FindingKind::Invalid,
                detail: "Line is not a valid hosts entry.".to_owned(),
            }),
            _ => {}
        }
    }
    findings.truncate(MAX_FINDINGS);
    findings
}

pub(crate) fn build_report(path: String, bytes: &[u8]) -> HostsReport {
    let document = HostsDocument::parse(bytes);
    let lines = document
        .lines
        .iter()
        .enumerate()
        .take(MAX_REPORT_LINES)
        .map(|(index, raw)| {
            let body = &raw[body_offset(index, raw)..];
            let body = body.strip_suffix(b"\r").unwrap_or(body);
            HostsLineView {
                index: index as u32,
                kind: classify_line(index, raw).kind(),
                text: bounded(&String::from_utf8_lossy(body)),
            }
        })
        .collect();
    HostsReport {
        path: bounded(&path),
        sha256: sha256_hex(bytes),
        size_bytes: bytes.len() as u64,
        total_lines: document.line_count() as u32,
        lines_truncated: document.line_count() > MAX_REPORT_LINES,
        lines,
        findings: findings(&document),
    }
}

/// Read-only report on the real hosts file.
pub fn hosts_report() -> Result<HostsReport, HostsError> {
    let store = HostsStore::system()?;
    let bytes = store.read()?;
    Ok(build_report(store.hosts.display().to_string(), &bytes))
}

// ---------------------------------------------------------------------------
// Line edits
// ---------------------------------------------------------------------------

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub(crate) fn sha256_digest(bytes: &[u8]) -> Result<Sha256Digest, AdapterError> {
    Sha256Digest::parse(sha256_hex(bytes)).map_err(|_| AdapterError::Failed)
}

/// `Ok(true)` when the op's line is already in its target state, `Ok(false)`
/// when it is in the source state, and `NotPresent` otherwise.
pub(crate) fn op_state(document: &HostsDocument, op: &HostsLineOp) -> Result<bool, AdapterError> {
    let not_present = AdapterError::Unsupported(UnsupportedReason::NotPresent);
    let line = document.classify(op.line as usize).ok_or(not_present)?;
    match (op.action, line) {
        (HostsLineAction::Disable, HostsLine::Mapping { .. })
        | (HostsLineAction::Restore, HostsLine::AppDisabled { .. }) => Ok(false),
        (HostsLineAction::Disable, HostsLine::AppDisabled { .. })
        | (HostsLineAction::Restore, HostsLine::Mapping { .. }) => Ok(true),
        _ => Err(not_present),
    }
}

fn validated_ops(line_ops: &[HostsLineOp]) -> Result<(), AdapterError> {
    SystemChange::EditHosts {
        line_ops: line_ops.to_vec(),
    }
    .validate()
    .map_err(|_| AdapterError::Failed)
}

/// Apply `line_ops` to `bytes`, re-validating every op. Lines already in
/// their target state are left as they are.
pub(crate) fn edit(bytes: &[u8], line_ops: &[HostsLineOp]) -> Result<Vec<u8>, AdapterError> {
    validated_ops(line_ops)?;
    let mut document = HostsDocument::parse(bytes);
    for op in line_ops {
        if op_state(&document, op)? {
            continue;
        }
        let index = op.line as usize;
        let raw = document
            .lines
            .get_mut(index)
            .ok_or(AdapterError::Unsupported(UnsupportedReason::NotPresent))?;
        let offset = body_offset(index, raw);
        match op.action {
            HostsLineAction::Disable => {
                raw.splice(offset..offset, DISABLED_PREFIX.iter().copied());
            }
            HostsLineAction::Restore => {
                raw.drain(offset..offset + DISABLED_PREFIX.len());
            }
        }
    }
    Ok(document.to_bytes())
}

// ---------------------------------------------------------------------------
// File access
// ---------------------------------------------------------------------------

pub trait HostsReader: Send + Sync {
    fn read(&self) -> Result<Vec<u8>, HostsError>;
}

pub trait HostsWriter {
    /// Replace the file with `contents` only if its current hash is still
    /// `expected`, keeping a backup of the current content.
    fn replace(&self, expected: &Sha256Digest, contents: &[u8]) -> Result<(), HostsError>;
}

/// Reads and writes the hosts file. The path always comes from
/// `GetSystemDirectoryW` outside tests.
pub struct HostsStore {
    hosts: PathBuf,
    backups: Option<PathBuf>,
}

impl HostsStore {
    pub fn system() -> Result<Self, HostsError> {
        Ok(Self {
            hosts: system_directory()?
                .join("drivers")
                .join("etc")
                .join("hosts"),
            backups: None,
        })
    }

    #[cfg(test)]
    pub(crate) fn at(hosts: PathBuf, backups: PathBuf) -> Self {
        Self {
            hosts,
            backups: Some(backups),
        }
    }

    fn backup_dir(&self) -> Result<PathBuf, HostsError> {
        if let Some(backups) = &self.backups {
            ensure_plain_dir(backups)?;
            return Ok(backups.clone());
        }
        let app = program_data()?.join("SupaDiskaKlinah");
        ensure_plain_dir(&app)?;
        let backups = app.join("hosts-backups");
        ensure_plain_dir(&backups)?;
        Ok(backups)
    }
}

impl HostsReader for HostsStore {
    fn read(&self) -> Result<Vec<u8>, HostsError> {
        let file = File::open(&self.hosts).map_err(io_error)?;
        let mut bytes = Vec::new();
        file.take(MAX_HOSTS_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(io_error)?;
        if bytes.len() > MAX_HOSTS_BYTES {
            return Err(HostsError::TooLarge);
        }
        Ok(bytes)
    }
}

impl HostsWriter for HostsStore {
    fn replace(&self, expected: &Sha256Digest, contents: &[u8]) -> Result<(), HostsError> {
        if contents.len() > MAX_HOSTS_BYTES + 64 * DISABLED_PREFIX.len() {
            return Err(HostsError::TooLarge);
        }
        let current = self.read()?;
        if sha256_hex(&current) != expected.as_str() {
            return Err(HostsError::StateChanged);
        }
        let backups = self.backup_dir()?;
        write_backup(&backups, &current, expected.as_str())?;
        prune_backups(&backups, KEEP_BACKUPS)?;

        let parent = self.hosts.parent().ok_or(HostsError::Io)?;
        let id = crate::system_change::random_id().map_err(|_| HostsError::Io)?;
        let temporary = parent.join(format!("hosts.sdk-{id}.tmp"));
        let written = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .and_then(|mut file| file.write_all(contents).and_then(|()| file.sync_all()))
            .map_err(io_error);
        let result = written
            .and_then(|()| {
                // Narrow the window between validation and replacement.
                if sha256_hex(&self.read()?) == expected.as_str() {
                    Ok(())
                } else {
                    Err(HostsError::StateChanged)
                }
            })
            .and_then(|()| move_replace(&temporary, &self.hosts).map_err(io_error));
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }
}

/// Reads the real hosts file, resolving its path on every call.
struct SystemHosts;

impl HostsReader for SystemHosts {
    fn read(&self) -> Result<Vec<u8>, HostsError> {
        HostsStore::system()?.read()
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn backup_name(secs: u64, sha: &str) -> String {
    format!("hosts-{secs}-{}.bak", &sha[..8.min(sha.len())])
}

/// Seconds from a name produced by [`backup_name`]; anything else is ignored.
fn parse_backup_name(name: &str) -> Option<u64> {
    let rest = name.strip_prefix("hosts-")?.strip_suffix(".bak")?;
    let (secs, hash) = rest.split_once('-')?;
    let valid = !secs.is_empty()
        && secs.len() <= 20
        && secs.bytes().all(|byte| byte.is_ascii_digit())
        && hash.len() == 8
        && hash
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
    if valid { secs.parse().ok() } else { None }
}

fn write_backup(directory: &Path, current: &[u8], sha: &str) -> Result<(), HostsError> {
    let path = directory.join(backup_name(unix_now(), sha));
    match OpenOptions::new().write(true).create_new(true).open(&path) {
        Ok(mut file) => file
            .write_all(current)
            .and_then(|()| file.sync_all())
            .map_err(io_error),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            // Same second and same hash prefix: accept only identical bytes.
            if fs::read(&path).map_err(io_error)? == current {
                Ok(())
            } else {
                Err(HostsError::Io)
            }
        }
        Err(error) => Err(io_error(error)),
    }
}

fn prune_backups(directory: &Path, keep: usize) -> Result<(), HostsError> {
    let mut backups: Vec<(u64, String)> = fs::read_dir(directory)
        .map_err(io_error)?
        .take(MAX_BACKUP_DIR_ENTRIES)
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            parse_backup_name(&name).map(|secs| (secs, name))
        })
        .collect();
    backups.sort_unstable_by(|left, right| right.cmp(left));
    for (_, name) in backups.into_iter().skip(keep) {
        fs::remove_file(directory.join(name)).map_err(io_error)?;
    }
    Ok(())
}

/// Create `path` if missing and refuse reparse points (junction/symlink
/// planting in a user-writable parent).
fn ensure_plain_dir(path: &Path) -> Result<(), HostsError> {
    match fs::create_dir(path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(io_error(error)),
    }
    let metadata = fs::symlink_metadata(path).map_err(io_error)?;
    if !metadata.is_dir() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(HostsError::Io);
    }
    Ok(())
}

fn move_replace(source: &Path, destination: &Path) -> io::Result<()> {
    let wide =
        |path: &Path| -> Vec<u16> { path.as_os_str().encode_wide().chain(Some(0)).collect() };
    let (source, destination) = (wide(source), wide(destination));
    // SAFETY: both are valid NUL-terminated UTF-16 strings alive for the call.
    if unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    } == 0
    {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn system_directory() -> Result<PathBuf, HostsError> {
    let mut buffer = [0u16; 32768];
    // SAFETY: the buffer is writable for `buffer.len()` UTF-16 units.
    let length = unsafe { GetSystemDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) } as usize;
    if length == 0 || length >= buffer.len() || buffer[..length].contains(&0) {
        return Err(HostsError::SystemDirectory);
    }
    let path = PathBuf::from(OsString::from_wide(&buffer[..length]));
    if !path.is_absolute() {
        return Err(HostsError::SystemDirectory);
    }
    Ok(path)
}

fn program_data() -> Result<PathBuf, HostsError> {
    let mut raw = ptr::null_mut();
    // SAFETY: Windows owns the returned NUL-terminated allocation until CoTaskMemFree below.
    let result =
        unsafe { SHGetKnownFolderPath(&FOLDERID_ProgramData, 0, ptr::null_mut(), &mut raw) };
    if result < 0 || raw.is_null() {
        if !raw.is_null() {
            // SAFETY: SHGetKnownFolderPath documents CoTaskMemFree even on failure.
            unsafe { CoTaskMemFree(raw.cast()) };
        }
        return Err(HostsError::Io);
    }
    let mut length = 0;
    // SAFETY: A successful SHGetKnownFolderPath call returns a valid NUL-terminated UTF-16 string.
    while unsafe { *raw.add(length) } != 0 {
        length += 1;
    }
    // SAFETY: `length` was established by scanning the valid allocation to its terminator.
    let path = PathBuf::from(OsString::from_wide(unsafe {
        std::slice::from_raw_parts(raw, length)
    }));
    // SAFETY: SHGetKnownFolderPath documents CoTaskMemFree as the matching deallocator.
    unsafe { CoTaskMemFree(raw.cast()) };
    if !path.is_absolute() {
        return Err(HostsError::Io);
    }
    Ok(path)
}

// ---------------------------------------------------------------------------
// Shared observe/apply
// ---------------------------------------------------------------------------

/// Hash the current file after checking every op against it.
pub(crate) fn observe_ops(
    reader: &dyn HostsReader,
    line_ops: &[HostsLineOp],
) -> Result<PriorState, AdapterError> {
    validated_ops(line_ops)?;
    let bytes = reader.read()?;
    let document = HostsDocument::parse(&bytes);
    for op in line_ops {
        op_state(&document, op)?;
    }
    Ok(PriorState::Hosts {
        sha256: sha256_digest(&bytes)?,
    })
}

/// Re-read, re-validate, and replace atomically with a backup.
pub(crate) fn apply_ops(
    reader: &dyn HostsReader,
    writer: &dyn HostsWriter,
    line_ops: &[HostsLineOp],
) -> Result<(), AdapterError> {
    let bytes = reader.read()?;
    let updated = edit(&bytes, line_ops)?;
    if updated == bytes {
        return Ok(());
    }
    writer
        .replace(&sha256_digest(&bytes)?, &updated)
        .map_err(AdapterError::from)
}

pub(crate) fn satisfied(
    reader: &dyn HostsReader,
    line_ops: &[HostsLineOp],
    state: &PriorState,
) -> bool {
    let PriorState::Hosts { sha256 } = state else {
        return false;
    };
    let Ok(bytes) = reader.read() else {
        return false;
    };
    if sha256_hex(&bytes) != sha256.as_str() {
        return false;
    }
    let document = HostsDocument::parse(&bytes);
    line_ops
        .iter()
        .all(|op| op_state(&document, op) == Ok(true))
}

// ---------------------------------------------------------------------------
// Adapter
// ---------------------------------------------------------------------------

pub struct HostsAdapter {
    reader: Box<dyn HostsReader>,
}

impl HostsAdapter {
    pub fn new() -> Self {
        Self {
            reader: Box::new(SystemHosts),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_reader(reader: impl HostsReader + 'static) -> Self {
        Self {
            reader: Box::new(reader),
        }
    }
}

impl Default for HostsAdapter {
    fn default() -> Self {
        Self::new()
    }
}

fn line_ops(change: &SystemChange) -> Result<&[HostsLineOp], AdapterError> {
    match change {
        SystemChange::EditHosts { line_ops } => Ok(line_ops),
        _ => Err(AdapterError::Failed),
    }
}

impl SystemAdapter for HostsAdapter {
    fn describe(&self, change: &SystemChange) -> Result<ImpactSummary, AdapterError> {
        let ops = line_ops(change)?;
        validated_ops(ops)?;
        let bytes = self.reader.read()?;
        let document = HostsDocument::parse(&bytes);
        let (mut disable, mut restore) = (0usize, 0usize);
        for op in ops {
            op_state(&document, op)?;
            match op.action {
                HostsLineAction::Disable => disable += 1,
                HostsLineAction::Restore => restore += 1,
            }
        }
        let mut parts = Vec::new();
        if disable > 0 {
            parts.push(format!(
                "comment out {disable} hosts mapping{}",
                if disable == 1 { "" } else { "s" }
            ));
        }
        if restore > 0 {
            parts.push(format!(
                "re-enable {restore} mapping{} this app disabled",
                if restore == 1 { "" } else { "s" }
            ));
        }
        Ok(ImpactSummary {
            component: "Hosts file".to_owned(),
            effect: format!(
                "Will {}. A backup of the hosts file is saved first; other lines are untouched.",
                parts.join(" and ")
            ),
            restart: RestartRequirement::None,
            risk: RiskLevel::Medium,
        })
    }

    fn observe(&self, change: &SystemChange) -> Result<PriorState, AdapterError> {
        observe_ops(self.reader.as_ref(), line_ops(change)?)
    }

    fn is_satisfied(&self, change: &SystemChange, state: &PriorState) -> bool {
        line_ops(change).is_ok_and(|ops| satisfied(self.reader.as_ref(), ops, state))
    }
}
