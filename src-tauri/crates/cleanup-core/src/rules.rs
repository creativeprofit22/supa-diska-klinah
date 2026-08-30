use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fmt,
    io::Read,
    path::{Component, Path, PathBuf},
};

#[derive(Clone, Copy, Debug)]
pub struct CatalogLimits {
    pub max_bytes: usize,
    pub max_rules: usize,
    pub max_roots_per_rule: usize,
    pub max_names_per_field: usize,
    pub max_excluded_paths: usize,
    pub max_text_bytes: usize,
    pub max_depth: u16,
}

impl Default for CatalogLimits {
    fn default() -> Self {
        Self {
            max_bytes: 1_048_576,
            max_rules: 256,
            max_roots_per_rule: 16,
            max_names_per_field: 64,
            max_excluded_paths: 64,
            max_text_bytes: 512,
            max_depth: 64,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Lifecycle {
    Candidate,
    Verified,
    Stable,
    Deprecated,
    Disabled,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Risk {
    Safe,
    Recoverable,
    HighImpact,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ScannerKind {
    Direct,
    ProjectArtifacts,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TargetType {
    File,
    Directory,
    Either,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Provenance {
    pub source: String,
    pub verified_at: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuleRoot {
    pub binding: String,
    #[serde(default)]
    pub suffix: PathBuf,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Markers {
    #[serde(default)]
    pub all: Vec<String>,
    #[serde(default)]
    pub any: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CleanupRule {
    pub id: String,
    pub rule_version: u32,
    pub lifecycle: Lifecycle,
    pub risk: Risk,
    pub provenance: Provenance,
    pub default_selected: bool,
    pub scanner: ScannerKind,
    pub roots: Vec<RuleRoot>,
    #[serde(default)]
    pub markers: Markers,
    pub targets: Vec<String>,
    pub target_type: TargetType,
    pub root_depth: u16,
    #[serde(default)]
    pub project_depth: Option<u16>,
    #[serde(default)]
    pub target_depth: Option<u16>,
    #[serde(default)]
    pub minimum_age_seconds: u64,
    #[serde(default)]
    pub excluded_names: Vec<String>,
    #[serde(default)]
    pub excluded_paths: Vec<PathBuf>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuleCatalog {
    rules: Vec<CleanupRule>,
}

impl RuleCatalog {
    pub fn rules(&self) -> &[CleanupRule] {
        &self.rules
    }
    pub fn rule(&self, id: &str) -> Option<&CleanupRule> {
        self.rules.iter().find(|rule| rule.id == id)
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SourceCatalog {
    schema_version: u32,
    rules: Vec<CleanupRule>,
}

#[derive(Debug)]
pub enum CatalogError {
    TooLarge,
    Read(std::io::Error),
    Json(serde_json::Error),
    Invalid(String),
}

impl fmt::Display for CatalogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge => f.write_str("rule catalog exceeds its byte limit"),
            Self::Read(_) => f.write_str("rule catalog could not be read"),
            Self::Json(_) => f.write_str("rule catalog is not valid schema v1 JSON"),
            Self::Invalid(reason) => write!(f, "invalid rule catalog: {reason}"),
        }
    }
}
impl std::error::Error for CatalogError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Read(error) => Some(error),
            Self::Json(error) => Some(error),
            _ => None,
        }
    }
}

pub fn load_catalog(reader: impl Read, limits: CatalogLimits) -> Result<RuleCatalog, CatalogError> {
    let mut bytes = Vec::new();
    reader
        .take(limits.max_bytes as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(CatalogError::Read)?;
    if bytes.len() > limits.max_bytes {
        return Err(CatalogError::TooLarge);
    }
    let source: SourceCatalog = serde_json::from_slice(&bytes).map_err(CatalogError::Json)?;
    validate(source, limits)
}

fn validate(source: SourceCatalog, limits: CatalogLimits) -> Result<RuleCatalog, CatalogError> {
    if source.schema_version != 1 {
        return invalid("unsupported schemaVersion");
    }
    if source.rules.is_empty() || source.rules.len() > limits.max_rules {
        return invalid("rule count is outside limits");
    }
    let mut ids = HashSet::new();
    for rule in &source.rules {
        text(&rule.id, limits, "id")?;
        if !valid_id(&rule.id) || !ids.insert(rule.id.clone()) {
            return invalid("rule IDs must be unique lowercase identifiers");
        }
        if rule.rule_version == 0 {
            return invalid("ruleVersion must be positive");
        }
        if rule.default_selected
            && (matches!(
                rule.lifecycle,
                Lifecycle::Candidate | Lifecycle::Deprecated | Lifecycle::Disabled
            ) || rule.risk == Risk::HighImpact)
        {
            return invalid("unsafe lifecycle or risk cannot be selected by default");
        }
        text(&rule.provenance.source, limits, "provenance source")?;
        text(
            &rule.provenance.verified_at,
            limits,
            "provenance verifiedAt",
        )?;
        if rule.roots.is_empty() || rule.roots.len() > limits.max_roots_per_rule {
            return invalid("root count is outside limits");
        }
        let mut roots = HashSet::new();
        for root in &rule.roots {
            name(&root.binding, limits, "binding")?;
            relative(&root.suffix)?;
            let key = format!(
                "{}:{}",
                root.binding.to_lowercase(),
                normalize(&root.suffix)
            );
            if !roots.insert(key) {
                return invalid("duplicate normalized root");
            }
        }
        names(&rule.targets, limits, "targets", false)?;
        names(&rule.markers.all, limits, "markers.all", true)?;
        names(&rule.markers.any, limits, "markers.any", true)?;
        names(&rule.excluded_names, limits, "excludedNames", true)?;
        if rule.excluded_paths.len() > limits.max_excluded_paths {
            return invalid("too many excluded paths");
        }
        let mut paths = HashSet::new();
        for path in &rule.excluded_paths {
            relative(path)?;
            if path.as_os_str().is_empty() || !paths.insert(normalize(path)) {
                return invalid("excluded paths must be non-empty and unique");
            }
        }
        for depth in [Some(rule.root_depth), rule.project_depth, rule.target_depth]
            .into_iter()
            .flatten()
        {
            if depth > limits.max_depth {
                return invalid("depth exceeds limit");
            }
        }
        match rule.scanner {
            ScannerKind::Direct
                if !rule.markers.all.is_empty()
                    || !rule.markers.any.is_empty()
                    || rule.project_depth.is_some() =>
            {
                return invalid("direct scanner cannot define project fields");
            }
            ScannerKind::ProjectArtifacts
                if rule.markers.all.is_empty() && rule.markers.any.is_empty() =>
            {
                return invalid("projectArtifacts requires markers");
            }
            ScannerKind::ProjectArtifacts
                if rule.project_depth.is_none() || rule.target_depth.is_none() =>
            {
                return invalid("projectArtifacts requires projectDepth and targetDepth");
            }
            _ => {}
        }
    }
    Ok(RuleCatalog {
        rules: source.rules,
    })
}

fn text(value: &str, limits: CatalogLimits, field: &str) -> Result<(), CatalogError> {
    if value.is_empty()
        || value.len() > limits.max_text_bytes
        || value.chars().any(char::is_control)
    {
        return invalid(&format!("invalid {field}"));
    }
    Ok(())
}
fn name(value: &str, limits: CatalogLimits, field: &str) -> Result<(), CatalogError> {
    text(value, limits, field)?;
    if value == "." || value == ".." || value.contains(['/', '\\']) {
        return invalid(&format!("{field} must be one path component"));
    }
    Ok(())
}
fn names(
    values: &[String],
    limits: CatalogLimits,
    field: &str,
    empty_allowed: bool,
) -> Result<(), CatalogError> {
    if (!empty_allowed && values.is_empty()) || values.len() > limits.max_names_per_field {
        return invalid(&format!("invalid {field} count"));
    }
    let mut unique = HashSet::new();
    for value in values {
        name(value, limits, field)?;
        if !unique.insert(value.to_lowercase()) {
            return invalid(&format!("duplicate {field}"));
        }
    }
    Ok(())
}
fn relative(path: &Path) -> Result<(), CatalogError> {
    if path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return invalid("paths must be normalized relative paths");
    }
    Ok(())
}
fn normalize(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/").to_lowercase()
}
fn valid_id(id: &str) -> bool {
    id.bytes().enumerate().all(|(index, byte)| {
        byte.is_ascii_lowercase()
            || byte.is_ascii_digit()
            || (index > 0 && matches!(byte, b'.' | b'-' | b'_'))
    })
}
fn invalid<T>(reason: &str) -> Result<T, CatalogError> {
    Err(CatalogError::Invalid(reason.into()))
}
