//! Bounded, no-follow, cancellable file scanning with typed evidence.
//!
//! Directories are walked iteratively. Reparse points (junctions, symlinks,
//! mount points, cloud placeholders) are reported and never followed. Each
//! file is opened without following a final reparse point, hashed and matched
//! in one streaming pass up to a per-file byte cap.

use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use protection_core::{
    CompiledPack, Evidence, FileFacts, SYSTEM_BINARY_NAMES, SignerStatus, UnavailableReason,
    evaluate_file, has_executable_extension,
};
use serde::Serialize;
use windows_sys::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_OPEN_REPARSE_POINT, FILE_FLAG_SEQUENTIAL_SCAN,
    FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
};

use super::authenticode::SignerCache;
use super::locations::KnownLocations;

const CHUNK: usize = 64 * 1024;
const MAX_FINDINGS: usize = 5_000;

#[derive(Clone, Copy, Debug)]
pub struct ScanLimits {
    pub max_files: u64,
    pub max_depth: usize,
    pub max_file_bytes: u64,
}

impl ScanLimits {
    pub const QUICK: Self = Self {
        max_files: 50_000,
        max_depth: 6,
        max_file_bytes: 256 * 1024 * 1024,
    };
    pub const FOLDER: Self = Self {
        max_files: 500_000,
        max_depth: 64,
        max_file_bytes: 256 * 1024 * 1024,
    };
}

/// A root to scan. Files are scanned directly; directories are walked.
#[derive(Clone, Debug)]
pub struct ScanTarget {
    pub path: PathBuf,
}

/// Shared progress and cancellation.
#[derive(Default)]
pub struct ScanControl {
    pub cancelled: AtomicBool,
    pub files_scanned: AtomicU64,
    pub bytes_hashed: AtomicU64,
}

/// One finding with its native path. The path stays in the backend; the UI
/// receives it for display only and refers back by opaque ID.
#[derive(Clone, Debug)]
pub struct ScannedFinding {
    pub path: PathBuf,
    pub root: PathBuf,
    pub sha256: Option<String>,
    pub size: Option<u64>,
    pub evidence: Evidence,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanSummary {
    pub files_scanned: u64,
    pub deterministic: u64,
    pub heuristic: u64,
    pub unavailable: u64,
    pub reparse_points_skipped: u64,
    pub allowlisted: u64,
    pub truncated: bool,
    pub cancelled: bool,
    pub pack_sequence: u64,
}

pub struct ScanOutcome {
    pub summary: ScanSummary,
    pub findings: Vec<ScannedFinding>,
}

pub struct Scanner<'a> {
    pub pack: &'a CompiledPack,
    pub signers: &'a SignerCache,
    pub known: &'a KnownLocations,
    /// SHA-256 values whose heuristic findings the user dismissed.
    /// Deterministic matches are never suppressed.
    pub allowlist: &'a HashSet<String>,
    pub limits: ScanLimits,
    pub control: Arc<ScanControl>,
}

fn unavailable_for(error: &std::io::Error) -> UnavailableReason {
    match error.raw_os_error() {
        Some(5) => UnavailableReason::AccessDenied,
        Some(32) | Some(33) => UnavailableReason::InUse,
        Some(2) | Some(3) => UnavailableReason::NotFound,
        _ => match error.kind() {
            std::io::ErrorKind::PermissionDenied => UnavailableReason::AccessDenied,
            std::io::ErrorKind::NotFound => UnavailableReason::NotFound,
            _ => UnavailableReason::ReadFailed,
        },
    }
}

fn is_reparse(metadata: &fs::Metadata) -> bool {
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

/// Open without following a final reparse point, allowing other writers.
pub(crate) fn open_no_follow(path: &Path) -> std::io::Result<File> {
    OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_SEQUENTIAL_SCAN)
        .open(path)
}

impl Scanner<'_> {
    pub fn run(&self, targets: &[ScanTarget]) -> ScanOutcome {
        let mut summary = ScanSummary {
            pack_sequence: self.pack.sequence(),
            ..ScanSummary::default()
        };
        let mut findings = Vec::new();
        let mut seen_files = HashSet::new();
        for target in targets {
            self.walk(target, &mut summary, &mut findings, &mut seen_files);
            if summary.cancelled || summary.truncated {
                break;
            }
        }
        ScanOutcome { summary, findings }
    }

    fn push(
        &self,
        summary: &mut ScanSummary,
        findings: &mut Vec<ScannedFinding>,
        finding: ScannedFinding,
    ) {
        match finding.evidence {
            Evidence::Deterministic { .. } => summary.deterministic += 1,
            Evidence::Heuristic { .. } => summary.heuristic += 1,
            Evidence::Unavailable { .. } => summary.unavailable += 1,
            Evidence::External { .. } => {}
        }
        if findings.len() < MAX_FINDINGS {
            findings.push(finding);
        } else {
            summary.truncated = true;
        }
    }

    fn walk(
        &self,
        target: &ScanTarget,
        summary: &mut ScanSummary,
        findings: &mut Vec<ScannedFinding>,
        seen_files: &mut HashSet<PathBuf>,
    ) {
        let root = target.path.clone();
        let mut stack = vec![(root.clone(), 0_usize)];
        while let Some((path, depth)) = stack.pop() {
            if self.control.cancelled.load(Ordering::Relaxed) {
                summary.cancelled = true;
                return;
            }
            if summary.files_scanned >= self.limits.max_files {
                summary.truncated = true;
                return;
            }
            let metadata = match fs::symlink_metadata(&path) {
                Ok(metadata) => metadata,
                Err(error) => {
                    if error.kind() != std::io::ErrorKind::NotFound {
                        self.push(
                            summary,
                            findings,
                            ScannedFinding {
                                path: path.clone(),
                                root: root.clone(),
                                sha256: None,
                                size: None,
                                evidence: Evidence::Unavailable {
                                    reason: unavailable_for(&error),
                                },
                            },
                        );
                    }
                    continue;
                }
            };
            if is_reparse(&metadata) {
                summary.reparse_points_skipped += 1;
                continue;
            }
            if metadata.is_dir() {
                if depth >= self.limits.max_depth {
                    continue;
                }
                match fs::read_dir(&path) {
                    Ok(entries) => {
                        for entry in entries.flatten() {
                            stack.push((entry.path(), depth + 1));
                        }
                    }
                    Err(error) => self.push(
                        summary,
                        findings,
                        ScannedFinding {
                            path: path.clone(),
                            root: root.clone(),
                            sha256: None,
                            size: None,
                            evidence: Evidence::Unavailable {
                                reason: unavailable_for(&error),
                            },
                        },
                    ),
                }
            } else if metadata.is_file() {
                let key = PathBuf::from(path.as_os_str().to_string_lossy().to_lowercase());
                if !seen_files.insert(key) {
                    continue;
                }
                summary.files_scanned += 1;
                self.control.files_scanned.fetch_add(1, Ordering::Relaxed);
                for finding in self.scan_file(&path, &root, metadata.len()) {
                    if matches!(finding.evidence, Evidence::Heuristic { .. })
                        && finding
                            .sha256
                            .as_ref()
                            .is_some_and(|h| self.allowlist.contains(h))
                    {
                        summary.allowlisted += 1;
                        continue;
                    }
                    self.push(summary, findings, finding);
                }
            }
        }
    }

    /// Scan one regular file and return all findings for it.
    pub fn scan_file(&self, path: &Path, root: &Path, size_hint: u64) -> Vec<ScannedFinding> {
        let make = |sha256: Option<String>, size: Option<u64>, evidence: Evidence| ScannedFinding {
            path: path.to_path_buf(),
            root: root.to_path_buf(),
            sha256,
            size,
            evidence,
        };
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut out: Vec<ScannedFinding> = self
            .pack
            .match_name(&name)
            .into_iter()
            .map(|hit| make(None, Some(size_hint), hit.evidence(self.pack.sequence())))
            .collect();

        let mut file = match open_no_follow(path) {
            Ok(file) => file,
            Err(error) => {
                out.push(make(
                    None,
                    Some(size_hint),
                    Evidence::Unavailable {
                        reason: unavailable_for(&error),
                    },
                ));
                return out;
            }
        };
        // The handle refers to the entry itself; refuse if it became a reparse point.
        match file.metadata() {
            Ok(metadata) if is_reparse(&metadata) || !metadata.is_file() => {
                out.push(make(
                    None,
                    None,
                    Evidence::Unavailable {
                        reason: UnavailableReason::ReparsePoint,
                    },
                ));
                return out;
            }
            Err(error) => {
                out.push(make(
                    None,
                    None,
                    Evidence::Unavailable {
                        reason: unavailable_for(&error),
                    },
                ));
                return out;
            }
            Ok(_) => {}
        }

        let mut header = [0_u8; 64];
        let mut header_len = 0;
        let mut digest = None;
        let mut size = None;
        let mut hits = Vec::new();
        if size_hint > self.limits.max_file_bytes {
            header_len = file.read(&mut header).unwrap_or(0);
            out.push(make(
                None,
                Some(size_hint),
                Evidence::Unavailable {
                    reason: UnavailableReason::TooLarge,
                },
            ));
        } else {
            let mut matcher = self.pack.matcher();
            let mut buffer = vec![0_u8; CHUNK];
            let mut total = 0_u64;
            loop {
                if self.control.cancelled.load(Ordering::Relaxed) {
                    out.push(make(
                        None,
                        None,
                        Evidence::Unavailable {
                            reason: UnavailableReason::Cancelled,
                        },
                    ));
                    return out;
                }
                match file.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(read) => {
                        if header_len < header.len() {
                            let take = (header.len() - header_len).min(read);
                            header[header_len..header_len + take].copy_from_slice(&buffer[..take]);
                            header_len += take;
                        }
                        total += read as u64;
                        if total > self.limits.max_file_bytes {
                            // The file grew while being read.
                            out.push(make(
                                None,
                                Some(total),
                                Evidence::Unavailable {
                                    reason: UnavailableReason::TooLarge,
                                },
                            ));
                            return out;
                        }
                        matcher.update(&buffer[..read]);
                        self.control
                            .bytes_hashed
                            .fetch_add(read as u64, Ordering::Relaxed);
                    }
                    Err(error) => {
                        out.push(make(
                            None,
                            Some(total),
                            Evidence::Unavailable {
                                reason: unavailable_for(&error),
                            },
                        ));
                        return out;
                    }
                }
            }
            let verdict = matcher.finish();
            hits = verdict.hits;
            digest = Some(verdict.sha256);
            size = Some(verdict.size);
        }
        drop(file);

        for hit in hits {
            out.push(make(
                digest.clone(),
                size,
                hit.evidence(self.pack.sequence()),
            ));
        }
        for finding in &mut out {
            if finding.sha256.is_none() {
                finding.sha256 = digest.clone();
            }
        }

        let location = self.known.classify(path);
        let lower = name.to_lowercase();
        let is_mz = header[..header_len].starts_with(b"MZ");
        let needs_signer = (is_mz || has_executable_extension(&lower))
            && (location.is_user_writable_risky() || SYSTEM_BINARY_NAMES.contains(&lower.as_str()));
        let signer = if needs_signer {
            self.signers.verify(path)
        } else {
            SignerStatus::NotApplicable
        };
        for evidence in evaluate_file(&FileFacts {
            file_name: &name,
            location,
            header: &header[..header_len],
            signer: &signer,
        }) {
            out.push(make(digest.clone(), size, evidence));
        }
        out
    }
}

/// Quick scope: startup folders, Temp, Downloads and running process images.
pub fn quick_targets(
    known: &KnownLocations,
    process_images: impl IntoIterator<Item = PathBuf>,
) -> Vec<ScanTarget> {
    let mut targets: Vec<ScanTarget> = known
        .autostart
        .iter()
        .chain(known.temp.iter())
        .chain(known.downloads.iter())
        .filter(|path| path.is_dir())
        .map(|path| ScanTarget { path: path.clone() })
        .collect();
    let mut seen = HashSet::new();
    for image in process_images {
        let key = image.to_string_lossy().to_lowercase();
        if seen.insert(key) && image.is_file() {
            targets.push(ScanTarget { path: image });
        }
    }
    targets
}

#[cfg(test)]
mod tests;
