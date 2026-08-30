use super::{CandidateDraft, Scanner, TraversalContext, excluded, old_enough, target_matches};
use crate::{CleanupRule, DiagnosticReason, EntryKind, FileIdentity, ScanError};
use std::{collections::HashSet, path::Path};

pub(crate) struct DirectScanner;

impl Scanner for DirectScanner {
    fn discover(
        &self,
        rule: &CleanupRule,
        root: &Path,
        context: &TraversalContext<'_>,
    ) -> Result<Vec<CandidateDraft>, ScanError> {
        let semantics = context.fs.semantics();
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
            entries.sort_by_key(|entry| semantics.key(&entry.path));
            for entry in entries {
                context.check_cancelled()?;
                if !semantics.contains(root, &entry.path) {
                    context.diagnostic(&rule.id, &entry.path, DiagnosticReason::OutsideRoot);
                    continue;
                }
                if excluded(rule, root, &entry.path, semantics)
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
                            rule_id: rule.id.clone(),
                            root: root.to_path_buf(),
                            path: entry.path.clone(),
                            kind: metadata.kind,
                            identity: metadata.identity.expect("validated identity"),
                        },
                        &mut output,
                    );
                }
                if metadata.kind == EntryKind::Directory && depth < rule.root_depth {
                    match metadata.identity {
                        Some(identity) if visited.insert(identity) => {
                            stack.push((entry.path, depth + 1, identity))
                        }
                        Some(_) => {
                            context.diagnostic(&rule.id, &entry.path, DiagnosticReason::Loop)
                        }
                        None => context.diagnostic(
                            &rule.id,
                            &entry.path,
                            DiagnosticReason::MissingIdentity,
                        ),
                    }
                }
            }
        }
        Ok(output)
    }
}
