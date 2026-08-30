use super::{CandidateDraft, Scanner, TraversalContext, excluded, old_enough, target_matches};
use crate::{CleanupRule, DiagnosticReason, EntryKind, FileIdentity, ScanError};
use std::{collections::HashSet, path::Path};

pub(crate) struct ProjectArtifactsScanner;

impl Scanner for ProjectArtifactsScanner {
    fn discover(
        &self,
        rule: &CleanupRule,
        root: &Path,
        context: &TraversalContext<'_>,
    ) -> Result<Vec<CandidateDraft>, ScanError> {
        let max_project_depth = rule.project_depth.unwrap_or(0);
        let mut output = Vec::new();
        let root_identity = context
            .fs
            .metadata_no_follow(root)
            .ok()
            .and_then(|metadata| metadata.identity)
            .ok_or_else(|| ScanError::InvalidInput("scan root identity disappeared".into()))?;
        let mut stack = vec![(root.to_path_buf(), 0_u16, root_identity)];
        let mut visited = HashSet::<FileIdentity>::from([root_identity]);
        while let Some((directory, depth, directory_identity)) = stack.pop() {
            context.check_cancelled()?;
            let Some(mut entries) = context.read_dir(&rule.id, &directory, directory_identity)?
            else {
                continue;
            };
            entries.sort_by_key(|entry| context.fs.semantics().key(&entry.path));
            let names: HashSet<String> = entries
                .iter()
                .filter(|entry| entry.kind != EntryKind::LinkLike)
                .map(|entry| entry.name.to_ascii_lowercase())
                .collect();
            let marker_match = rule
                .markers
                .all
                .iter()
                .all(|name| names.contains(&name.to_ascii_lowercase()))
                && (rule.markers.any.is_empty()
                    || rule
                        .markers
                        .any
                        .iter()
                        .any(|name| names.contains(&name.to_ascii_lowercase())));
            if marker_match {
                discover_targets(
                    rule,
                    root,
                    &directory,
                    directory_identity,
                    context,
                    &mut output,
                )?;
            }
            if depth >= max_project_depth {
                continue;
            }
            for entry in entries.into_iter().rev() {
                if entry.kind != EntryKind::Directory
                    || excluded(rule, root, &entry.path, context.fs.semantics())
                    || context.protection.is_protected(&entry.path)
                {
                    continue;
                }
                let metadata = match context.fs.metadata_no_follow(&entry.path) {
                    Ok(metadata) => metadata,
                    Err(_) => {
                        context.diagnostic(&rule.id, &entry.path, DiagnosticReason::Disappeared);
                        continue;
                    }
                };
                if metadata.kind == EntryKind::LinkLike {
                    context.diagnostic(&rule.id, &entry.path, DiagnosticReason::LinkLike);
                    continue;
                }
                if metadata.kind != EntryKind::Directory || metadata.identity != entry.identity {
                    context.diagnostic(&rule.id, &entry.path, DiagnosticReason::Changed);
                    continue;
                }
                match metadata.identity {
                    Some(identity) if visited.insert(identity) => {
                        stack.push((entry.path, depth + 1, identity))
                    }
                    Some(_) => context.diagnostic(&rule.id, &entry.path, DiagnosticReason::Loop),
                    None => {
                        context.diagnostic(&rule.id, &entry.path, DiagnosticReason::MissingIdentity)
                    }
                }
            }
        }
        Ok(output)
    }
}

fn discover_targets(
    rule: &CleanupRule,
    scan_root: &Path,
    project_root: &Path,
    project_identity: FileIdentity,
    context: &TraversalContext<'_>,
    output: &mut Vec<CandidateDraft>,
) -> Result<(), ScanError> {
    let mut stack = vec![(project_root.to_path_buf(), 0_u16, project_identity)];
    let mut visited = HashSet::<FileIdentity>::from([project_identity]);
    while let Some((directory, depth, directory_identity)) = stack.pop() {
        let Some(mut entries) = context.read_dir(&rule.id, &directory, directory_identity)? else {
            continue;
        };
        entries.sort_by_key(|entry| context.fs.semantics().key(&entry.path));
        for entry in entries.into_iter().rev() {
            context.check_cancelled()?;
            if !context.fs.semantics().contains(project_root, &entry.path) {
                context.diagnostic(&rule.id, &entry.path, DiagnosticReason::OutsideRoot);
                continue;
            }
            if excluded(rule, scan_root, &entry.path, context.fs.semantics())
                || context.protection.is_protected(&entry.path)
            {
                continue;
            }
            if entry.kind == EntryKind::LinkLike {
                context.diagnostic(&rule.id, &entry.path, DiagnosticReason::LinkLike);
                continue;
            }
            let metadata = match context.fs.metadata_no_follow(&entry.path) {
                Ok(metadata) => metadata,
                Err(_) => {
                    context.diagnostic(&rule.id, &entry.path, DiagnosticReason::Disappeared);
                    continue;
                }
            };
            if metadata.kind == EntryKind::LinkLike {
                context.diagnostic(&rule.id, &entry.path, DiagnosticReason::LinkLike);
                continue;
            }
            if metadata.kind != entry.kind || metadata.identity != entry.identity {
                context.diagnostic(&rule.id, &entry.path, DiagnosticReason::Changed);
                continue;
            }
            if target_matches(rule, &entry.name, metadata.kind)
                && old_enough(rule, &metadata, context.now)
            {
                context.push_candidate(
                    CandidateDraft {
                        rule: rule.clone(),
                        scan_root: scan_root.to_path_buf(),
                        context_root: project_root.to_path_buf(),
                        path: entry.path.clone(),
                        kind: metadata.kind,
                        identity: metadata.identity.expect("validated identity"),
                        scanned_at: context.now,
                    },
                    output,
                );
            }
            if metadata.kind == EntryKind::Directory && depth < rule.target_depth.unwrap_or(0) {
                match metadata.identity {
                    Some(identity) if visited.insert(identity) => {
                        stack.push((entry.path, depth + 1, identity))
                    }
                    Some(_) => context.diagnostic(&rule.id, &entry.path, DiagnosticReason::Loop),
                    None => {
                        context.diagnostic(&rule.id, &entry.path, DiagnosticReason::MissingIdentity)
                    }
                }
            }
        }
    }
    Ok(())
}
