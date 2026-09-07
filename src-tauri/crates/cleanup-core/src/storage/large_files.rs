use super::{CandidateEligibility, StorageError};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FileSort {
    Size,
    Modified,
    Path,
}
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FileCategory {
    Any,
    Documents,
    Images,
    Audio,
    Video,
    Archives,
    Other,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FileFilter {
    pub minimum_bytes: u64,
    pub maximum_bytes: Option<u64>,
    pub extensions: Vec<String>,
    pub category: FileCategory,
    pub sort: FileSort,
    pub descending: bool,
}
impl FileFilter {
    pub fn validate(&self) -> Result<(), StorageError> {
        if self
            .maximum_bytes
            .is_some_and(|max| max < self.minimum_bytes)
            || self.extensions.len() > 64
            || self.extensions.iter().any(|ext| {
                ext.is_empty()
                    || ext.len() > 32
                    || !ext
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
            })
        {
            return Err(StorageError::InvalidRequest);
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FileRecord {
    pub record_id: String,
    pub display_path: String,
    pub logical_bytes: u64,
    pub allocated_bytes: Option<u64>,
    pub modified_unix_seconds: Option<u64>,
    pub eligibility: CandidateEligibility,
    // Personal files are never default-selected; there is intentionally no such flag.
}

impl Default for FileFilter {
    fn default() -> Self {
        Self {
            minimum_bytes: 10 * 1024 * 1024,
            maximum_bytes: None,
            extensions: vec![],
            category: FileCategory::Any,
            sort: FileSort::Size,
            descending: true,
        }
    }
}
pub fn extension(path: &std::path::Path) -> String {
    path.extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase()
}
pub fn category(extension: &str) -> FileCategory {
    match extension {
        "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "txt" | "rtf" | "odt"
        | "csv" => FileCategory::Documents,
        "jpg" | "jpeg" | "png" | "gif" | "bmp" | "webp" | "svg" | "tif" | "tiff" | "heic" => {
            FileCategory::Images
        }
        "mp3" | "wav" | "flac" | "aac" | "ogg" | "m4a" | "wma" => FileCategory::Audio,
        "mp4" | "mkv" | "avi" | "mov" | "wmv" | "webm" | "m4v" => FileCategory::Video,
        "zip" | "7z" | "rar" | "gz" | "tar" | "bz2" | "xz" => FileCategory::Archives,
        _ => FileCategory::Other,
    }
}
impl FileFilter {
    pub fn matches(&self, path: &std::path::Path, metadata: &crate::EntryMetadata) -> bool {
        let ext = extension(path);
        metadata.kind == crate::EntryKind::File
            && metadata.size >= self.minimum_bytes
            && self.maximum_bytes.is_none_or(|max| metadata.size <= max)
            && (self.extensions.is_empty() || self.extensions.contains(&ext))
            && (self.category == FileCategory::Any || self.category == category(&ext))
    }
}
/// Retains at most `limit` matching directory entries (hard-link paths stay visible).
/// A further match stops traversal and marks partial; sorting is over the observed
/// prefix, never advertised as the global largest N on a truncated scan.
pub struct LargeFiles {
    filter: FileFilter,
    limit: usize,
    files: Vec<(FileRecord, Option<super::ObservedEntry>)>,
}
impl LargeFiles {
    pub fn new(filter: FileFilter, limit: usize) -> Result<Self, StorageError> {
        filter.validate()?;
        if !(1..=super::MAX_RECORDS).contains(&limit) {
            return Err(StorageError::InvalidRequest);
        }
        Ok(Self {
            filter,
            limit,
            files: vec![],
        })
    }
    pub fn observe(
        &mut self,
        event: super::walk::WalkEvent<'_>,
        allocated: Option<u64>,
    ) -> super::walk::WalkControl {
        if let super::walk::WalkEvent::Entry { path, metadata, .. } = event
            && self.filter.matches(path, metadata)
        {
            if self.files.len() >= self.limit {
                return super::walk::WalkControl::Stop;
            }
            let modified = metadata
                .modified
                .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok());
            let evidence = metadata
                .identity
                .zip(modified.and_then(|d| u64::try_from(d.as_nanos()).ok()))
                .map(|(identity, modified_unix_nanos)| super::ObservedEntry {
                    canonical_path: path.into(),
                    identity,
                    kind: metadata.kind,
                    logical_bytes: metadata.size,
                    allocated_bytes: allocated,
                    modified_unix_nanos,
                });
            self.files.push((
                FileRecord {
                    record_id: String::new(),
                    display_path: path.to_string_lossy().into_owned(),
                    logical_bytes: metadata.size,
                    allocated_bytes: allocated,
                    modified_unix_seconds: modified.map(|d| d.as_secs()),
                    eligibility: CandidateEligibility::ReadOnly,
                },
                evidence,
            ));
        }
        super::walk::WalkControl::Continue
    }
    pub fn finish(mut self) -> Vec<(FileRecord, Option<super::ObservedEntry>)> {
        let filter = self.filter;
        self.files.sort_by(|(a, _), (b, _)| {
            // Missing dates always last in either direction. Path is a stable
            // case-folded key with exact path tie-breaks, never a random ID.
            let paths = || {
                a.display_path
                    .to_lowercase()
                    .cmp(&b.display_path.to_lowercase())
                    .then_with(|| a.display_path.cmp(&b.display_path))
            };
            if filter.sort == FileSort::Modified {
                match (a.modified_unix_seconds, b.modified_unix_seconds) {
                    (None, Some(_)) => return std::cmp::Ordering::Greater,
                    (Some(_), None) => return std::cmp::Ordering::Less,
                    _ => {}
                }
            }
            let order = match filter.sort {
                FileSort::Size => a.logical_bytes.cmp(&b.logical_bytes),
                FileSort::Modified => a.modified_unix_seconds.cmp(&b.modified_unix_seconds),
                FileSort::Path => paths(),
            };
            (if filter.descending {
                order.reverse()
            } else {
                order
            })
            .then_with(paths)
        });
        self.files
    }
}
