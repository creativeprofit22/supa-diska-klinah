//! Read-only aggregation over the shared bounded no-follow traversal.
use super::scans::ScanContext;
use cleanup_core::{EntryKind, FileSystem, ProtectionPolicy, storage::*};
use std::{collections::HashMap, path::Path};

pub fn discover(
    context: &mut ScanContext,
    protection: &ProtectionPolicy,
    displayed_depth: u16,
) -> Result<(), StorageError> {
    if displayed_depth > context.limits.depth {
        return Err(StorageError::InvalidRequest);
    }
    let root = context.root.as_ref().ok_or(StorageError::InvalidEvidence)?;
    let mut analyzer = analysis::Analyzer::new(
        &root.canonical_path,
        displayed_depth,
        context.limits.retained_records,
    )?;
    context.phase(StoragePhase::Walking);
    let machine = super::protection::MachineRoots::resolve()?;
    let report = context.walk(
        protection,
        &|path| machine.excludes(path),
        &mut |_, event| {
            let allocated = match &event {
                walk::WalkEvent::Entry { path, metadata, .. }
                    if metadata.kind == EntryKind::File =>
                {
                    crate::WindowsFileSystem.allocated_size(path, metadata).ok()
                }
                _ => None,
            };
            analyzer.observe(event, allocated)
        },
    )?;
    let (mut directories, extensions) = analyzer.finish(&report.completeness);
    context.phase(StoragePhase::Finalizing);
    let mut ids = HashMap::new();
    for row in &mut directories {
        row.node_id = super::opaque_id()?;
        ids.insert(row.display_path.clone(), row.node_id.clone());
    }
    for mut row in directories {
        row.parent_id = Path::new(&row.display_path)
            .parent()
            .and_then(|p| ids.get(p.to_string_lossy().as_ref()))
            .cloned();
        for reason in &row.completeness.reasons {
            context.mark_partial(*reason);
        }
        if context.cancellation.is_cancelled() {
            return Ok(());
        }
        let order = RecordOrder {
            numeric: 0,
            text: row.display_path.to_lowercase(),
        };
        context.push(StorageRecord::Directory(row), order)?;
    }
    for row in extensions {
        for reason in &row.completeness.reasons {
            context.mark_partial(*reason);
        }
        if context.cancellation.is_cancelled() {
            return Ok(());
        }
        let order = RecordOrder {
            numeric: 0,
            text: row.extension.clone(),
        };
        context.push(StorageRecord::Extension(row), order)?;
    }
    Ok(())
}
