//! Matching engine for verified rule packs.
//!
//! Files are streamed once: every chunk updates a SHA-256 and the first
//! `head_limit` bytes are kept for byte-pattern matching. Byte patterns cannot
//! anchor beyond [`MAX_PATTERN_OFFSET`], so memory per file stays bounded.

use std::collections::HashMap;

use aho_corasick::{AhoCorasick, AhoCorasickBuilder, MatchKind};
use sha2::{Digest, Sha256};

use crate::evidence::{Evidence, MatchMethod, Severity, from_hex, to_hex};
use crate::pack::{MAX_PATTERN_OFFSET, RuleMatch, VerifiedPack};

#[derive(Clone, Debug)]
struct RuleInfo {
    id: String,
    name: String,
    severity: Severity,
}

#[derive(Clone, Debug)]
struct PatternMeta {
    rule: usize,
    offset_min: u64,
    offset_max: u64,
    max_file_size: Option<u64>,
}

/// A compiled, immutable matcher for one verified pack.
#[derive(Clone, Debug)]
pub struct CompiledPack {
    sequence: u64,
    rules: Vec<RuleInfo>,
    hashes: HashMap<[u8; 32], Vec<usize>>,
    names: HashMap<String, Vec<usize>>,
    patterns: Option<AhoCorasick>,
    pattern_meta: Vec<PatternMeta>,
    head_limit: usize,
}

/// One rule that matched a file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuleHit {
    pub rule_id: String,
    pub rule_name: String,
    pub method: MatchMethod,
    pub severity: Severity,
}

impl RuleHit {
    /// SHA-256 and byte matches are deterministic; file-name rules are heuristic.
    pub fn evidence(&self, pack_sequence: u64) -> Evidence {
        match self.method {
            MatchMethod::Sha256 | MatchMethod::Bytes => Evidence::Deterministic {
                rule_id: self.rule_id.clone(),
                rule_name: self.rule_name.clone(),
                pack_sequence,
                method: self.method,
                severity: self.severity,
            },
            MatchMethod::FileName => Evidence::Heuristic {
                heuristic_id: format!("rule:{}", self.rule_id),
                reason: format!("File name matches signed rule \"{}\"", self.rule_name),
                false_positive_note:
                    "Names are easy to reuse; a legitimate file can share this name.".into(),
                severity: self.severity,
            },
        }
    }
}

/// Result of streaming one file through the matcher.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileVerdict {
    pub sha256: String,
    pub size: u64,
    pub hits: Vec<RuleHit>,
}

impl CompiledPack {
    pub fn compile(verified: &VerifiedPack) -> Self {
        let pack = verified.pack();
        let mut rules = Vec::with_capacity(pack.rules.len());
        let mut hashes: HashMap<[u8; 32], Vec<usize>> = HashMap::new();
        let mut names: HashMap<String, Vec<usize>> = HashMap::new();
        let mut pattern_bytes = Vec::new();
        let mut pattern_meta = Vec::new();
        let mut head_limit = 0_usize;
        for (index, rule) in pack.rules.iter().enumerate() {
            rules.push(RuleInfo {
                id: rule.id.clone(),
                name: rule.name.clone(),
                severity: rule.severity,
            });
            // Validation already guaranteed well-formed hex and bounds.
            match &rule.matcher {
                RuleMatch::Sha256 { sha256 } => {
                    if let Some(Ok(digest)) = from_hex(sha256).map(<[u8; 32]>::try_from) {
                        hashes.entry(digest).or_default().push(index);
                    }
                }
                RuleMatch::Bytes {
                    pattern,
                    offset_min,
                    offset_max,
                    max_file_size,
                } => {
                    if let Some(bytes) = from_hex(pattern) {
                        let end = (*offset_max).min(MAX_PATTERN_OFFSET) as usize + bytes.len();
                        head_limit = head_limit.max(end);
                        pattern_bytes.push(bytes);
                        pattern_meta.push(PatternMeta {
                            rule: index,
                            offset_min: *offset_min,
                            offset_max: *offset_max,
                            max_file_size: *max_file_size,
                        });
                    }
                }
                RuleMatch::FileName { name } => {
                    names.entry(name.to_lowercase()).or_default().push(index);
                }
            }
        }
        let patterns = (!pattern_bytes.is_empty()).then(|| {
            AhoCorasickBuilder::new()
                .match_kind(MatchKind::Standard)
                .build(&pattern_bytes)
                .expect("bounded patterns always compile")
        });
        Self {
            sequence: pack.sequence,
            rules,
            hashes,
            names,
            patterns,
            pattern_meta,
            head_limit,
        }
    }

    pub fn sequence(&self) -> u64 {
        self.sequence
    }

    pub fn rule_count(&self) -> usize {
        self.rules.len()
    }

    fn hit(&self, index: usize, method: MatchMethod) -> RuleHit {
        let rule = &self.rules[index];
        RuleHit {
            rule_id: rule.id.clone(),
            rule_name: rule.name.clone(),
            method,
            severity: rule.severity,
        }
    }

    /// File-name rules, compared case-insensitively against the final component.
    pub fn match_name(&self, file_name: &str) -> Vec<RuleHit> {
        self.names
            .get(&file_name.to_lowercase())
            .into_iter()
            .flatten()
            .map(|index| self.hit(*index, MatchMethod::FileName))
            .collect()
    }

    /// Start streaming one file.
    pub fn matcher(&self) -> FileMatcher<'_> {
        FileMatcher {
            pack: self,
            hasher: Sha256::new(),
            head: Vec::new(),
            size: 0,
        }
    }

    /// Match a known digest without reading the file again.
    pub fn match_digest(&self, digest: &[u8; 32]) -> Vec<RuleHit> {
        self.hashes
            .get(digest)
            .into_iter()
            .flatten()
            .map(|index| self.hit(*index, MatchMethod::Sha256))
            .collect()
    }
}

/// Streaming state for one file. Feed chunks in order, then call `finish`.
pub struct FileMatcher<'a> {
    pack: &'a CompiledPack,
    hasher: Sha256,
    head: Vec<u8>,
    size: u64,
}

impl FileMatcher<'_> {
    pub fn update(&mut self, chunk: &[u8]) {
        self.hasher.update(chunk);
        self.size += chunk.len() as u64;
        let room = self.pack.head_limit.saturating_sub(self.head.len());
        if room > 0 {
            self.head.extend_from_slice(&chunk[..chunk.len().min(room)]);
        }
    }

    pub fn finish(self) -> FileVerdict {
        let digest: [u8; 32] = self.hasher.finalize().into();
        let mut hits = self.pack.match_digest(&digest);
        if let Some(patterns) = &self.pack.patterns {
            let mut seen = vec![false; self.pack.pattern_meta.len()];
            for found in patterns.find_overlapping_iter(&self.head) {
                let pattern = found.pattern().as_usize();
                if seen[pattern] {
                    continue;
                }
                let meta = &self.pack.pattern_meta[pattern];
                let start = found.start() as u64;
                let within_offset = (meta.offset_min..=meta.offset_max).contains(&start);
                let within_size = meta.max_file_size.is_none_or(|max| self.size <= max);
                if within_offset && within_size {
                    seen[pattern] = true;
                    hits.push(self.pack.hit(meta.rule, MatchMethod::Bytes));
                }
            }
        }
        hits.dedup_by(|a, b| a.rule_id == b.rule_id);
        FileVerdict {
            sha256: to_hex(&digest),
            size: self.size,
            hits,
        }
    }
}
