//! Local, signer-aware heuristics. Every heuristic has a stable ID and a
//! documented false-positive note. Heuristics never produce deterministic
//! evidence and can be suppressed per file by a SHA-256 allowlist.

use serde::{Deserialize, Serialize};

use crate::evidence::{Evidence, Severity, UnavailableReason};

/// Where a file lives, classified by the platform adapter.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum LocationClass {
    SystemRoot,
    ProgramFiles,
    Autostart,
    Temp,
    Downloads,
    RoamingAppData,
    OtherUserWritable,
    Other,
}

impl LocationClass {
    pub fn is_user_writable_risky(self) -> bool {
        matches!(
            self,
            Self::Autostart
                | Self::Temp
                | Self::Downloads
                | Self::RoamingAppData
                | Self::OtherUserWritable
        )
    }
}

/// Authenticode result, evaluated offline by the platform adapter.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    tag = "state",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SignerStatus {
    Valid {
        subject: String,
        microsoft: bool,
    },
    Unsigned,
    Invalid,
    /// Could not be evaluated (not a PE file, access denied, catalog only…).
    Unavailable,
    /// Not checked because the file is not an executable type.
    NotApplicable,
}

impl SignerStatus {
    pub fn is_microsoft(&self) -> bool {
        matches!(
            self,
            Self::Valid {
                microsoft: true,
                ..
            }
        )
    }
}

/// Facts about one file that heuristics evaluate.
#[derive(Clone, Debug)]
pub struct FileFacts<'a> {
    pub file_name: &'a str,
    pub location: LocationClass,
    /// The first bytes of the file (at least 2 for the MZ check).
    pub header: &'a [u8],
    pub signer: &'a SignerStatus,
}

/// Facts about one running process.
#[derive(Clone, Debug)]
pub struct ProcessFacts<'a> {
    pub image_name: &'a str,
    pub image_location: LocationClass,
    pub image_deleted: bool,
    pub signer: &'a SignerStatus,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HeuristicInfo {
    pub id: &'static str,
    pub title: &'static str,
    pub false_positive_note: &'static str,
}

pub const H_SYSTEM_NAME: HeuristicInfo = HeuristicInfo {
    id: "H001-system-name-outside-windows",
    title: "Windows system binary name outside the Windows folder",
    false_positive_note: "Installers, backups and virtual machines can hold copies of system binaries; a valid Microsoft signature lowers the severity.",
};
pub const H_DOUBLE_EXTENSION: HeuristicInfo = HeuristicInfo {
    id: "H002-double-extension",
    title: "Document-like double extension on an executable",
    false_positive_note: "Some tools name archives or generated files this way; check the signer and origin.",
};
pub const H_BIDI_NAME: HeuristicInfo = HeuristicInfo {
    id: "H003-bidi-control-in-name",
    title: "Bidirectional control character in file name",
    false_positive_note: "Right-to-left language names can legitimately contain direction marks.",
};
pub const H_MZ_MISMATCH: HeuristicInfo = HeuristicInfo {
    id: "H004-executable-content-wrong-extension",
    title: "Executable (MZ) content under a non-executable extension",
    false_positive_note: "Some applications store plugins or resources as PE files with custom extensions.",
};
pub const H_UNSIGNED_RISKY: HeuristicInfo = HeuristicInfo {
    id: "H005-unsigned-executable-risky-location",
    title: "Unsigned or invalidly signed executable in an autostart or download location",
    false_positive_note: "Many legitimate open-source and portable tools are unsigned.",
};
pub const H_PROCESS_IMAGE: HeuristicInfo = HeuristicInfo {
    id: "H006-process-image-deleted-or-user-writable",
    title: "Running process image deleted or in a user-writable folder",
    false_positive_note: "Updaters replace their own images, and per-user installs (browsers, chat apps) run from AppData.",
};

pub const HEURISTIC_CATALOG: [HeuristicInfo; 6] = [
    H_SYSTEM_NAME,
    H_DOUBLE_EXTENSION,
    H_BIDI_NAME,
    H_MZ_MISMATCH,
    H_UNSIGNED_RISKY,
    H_PROCESS_IMAGE,
];

/// System binary names that only belong in `%SystemRoot%` (from Kudu's list).
pub const SYSTEM_BINARY_NAMES: [&str; 14] = [
    "svchost.exe",
    "csrss.exe",
    "lsass.exe",
    "winlogon.exe",
    "services.exe",
    "smss.exe",
    "wininit.exe",
    "spoolsv.exe",
    "taskhostw.exe",
    "dwm.exe",
    "conhost.exe",
    "rundll32.exe",
    "dllhost.exe",
    "explorer.exe",
];

pub const EXECUTABLE_EXTENSIONS: [&str; 15] = [
    "exe", "dll", "sys", "scr", "com", "cpl", "ocx", "efi", "drv", "mui", "node", "pyd", "winmd",
    "ax", "tlb",
];

const SCRIPT_OR_EXEC_EXTENSIONS: [&str; 10] = [
    "exe", "scr", "com", "bat", "cmd", "js", "vbs", "ps1", "lnk", "pif",
];

const DOCUMENT_EXTENSIONS: [&str; 16] = [
    "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "txt", "rtf", "jpg", "jpeg", "png", "gif",
    "mp3", "mp4", "zip",
];

const BIDI_CONTROLS: [char; 9] = [
    '\u{202A}', '\u{202B}', '\u{202C}', '\u{202D}', '\u{202E}', '\u{2066}', '\u{2067}', '\u{2068}',
    '\u{2069}',
];

fn extension(name: &str) -> Option<String> {
    let (_, ext) = name.rsplit_once('.')?;
    Some(ext.to_ascii_lowercase())
}

pub fn has_executable_extension(name: &str) -> bool {
    extension(name).is_some_and(|ext| EXECUTABLE_EXTENSIONS.contains(&ext.as_str()))
}

fn evidence(info: HeuristicInfo, reason: String, severity: Severity) -> Evidence {
    Evidence::Heuristic {
        heuristic_id: info.id.into(),
        reason,
        false_positive_note: info.false_positive_note.into(),
        severity,
    }
}

/// Evaluate every file heuristic. Returns heuristic evidence, plus
/// `Unavailable` when a heuristic needed a signer result it could not get.
pub fn evaluate_file(facts: &FileFacts<'_>) -> Vec<Evidence> {
    let mut out = Vec::new();
    let lower = facts.file_name.to_lowercase();
    let is_mz = facts.header.starts_with(b"MZ");
    let exec_ext = has_executable_extension(&lower);

    if SYSTEM_BINARY_NAMES.contains(&lower.as_str()) && facts.location != LocationClass::SystemRoot
    {
        let severity = if facts.signer.is_microsoft() {
            Severity::Low
        } else {
            Severity::High
        };
        let signer = if facts.signer.is_microsoft() {
            "valid Microsoft signature"
        } else {
            "no valid Microsoft signature"
        };
        out.push(evidence(
            H_SYSTEM_NAME,
            format!(
                "{} is a Windows system name found outside the Windows folder ({signer})",
                facts.file_name
            ),
            severity,
        ));
    }

    let parts: Vec<&str> = lower.split('.').collect();
    if parts.len() >= 3 {
        let last = parts[parts.len() - 1];
        let penultimate = parts[parts.len() - 2];
        if SCRIPT_OR_EXEC_EXTENSIONS.contains(&last) && DOCUMENT_EXTENSIONS.contains(&penultimate) {
            out.push(evidence(
                H_DOUBLE_EXTENSION,
                format!("Name ends in .{penultimate}.{last}, which hides the real type"),
                Severity::Medium,
            ));
        }
    }

    if facts.file_name.chars().any(|c| BIDI_CONTROLS.contains(&c)) {
        out.push(evidence(
            H_BIDI_NAME,
            "Name contains a bidirectional control character that can reverse how it displays"
                .into(),
            Severity::Medium,
        ));
    }

    if is_mz && !exec_ext {
        out.push(evidence(
            H_MZ_MISMATCH,
            "File starts with an executable (MZ) header but its extension is not executable".into(),
            Severity::Medium,
        ));
    }

    if (exec_ext || is_mz) && facts.location.is_user_writable_risky() {
        match facts.signer {
            SignerStatus::Unsigned => out.push(evidence(
                H_UNSIGNED_RISKY,
                "Unsigned executable in an autostart, Temp, Downloads or roaming AppData folder"
                    .into(),
                Severity::Low,
            )),
            SignerStatus::Invalid => out.push(evidence(
                H_UNSIGNED_RISKY,
                "Executable with an invalid or untrusted signature in a risky folder".into(),
                Severity::Medium,
            )),
            SignerStatus::Unavailable => out.push(Evidence::Unavailable {
                reason: UnavailableReason::SignerNotCheckable,
            }),
            SignerStatus::Valid { .. } | SignerStatus::NotApplicable => {}
        }
    }
    out
}

/// Evaluate process heuristics.
pub fn evaluate_process(facts: &ProcessFacts<'_>) -> Vec<Evidence> {
    let mut out = Vec::new();
    let lower = facts.image_name.to_lowercase();
    if SYSTEM_BINARY_NAMES.contains(&lower.as_str())
        && facts.image_location != LocationClass::SystemRoot
        && !facts.image_deleted
    {
        let severity = if facts.signer.is_microsoft() {
            Severity::Low
        } else {
            Severity::High
        };
        out.push(evidence(
            H_SYSTEM_NAME,
            format!(
                "Process {} runs from outside the Windows folder",
                facts.image_name
            ),
            severity,
        ));
    }
    if facts.image_deleted {
        out.push(evidence(
            H_PROCESS_IMAGE,
            "The process image file no longer exists on disk".into(),
            Severity::Medium,
        ));
    } else if facts.image_location.is_user_writable_risky()
        && matches!(facts.signer, SignerStatus::Unsigned | SignerStatus::Invalid)
    {
        out.push(evidence(
            H_PROCESS_IMAGE,
            "Unsigned process running from a user-writable folder".into(),
            Severity::Low,
        ));
    }
    out
}
