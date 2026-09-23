use super::ObservedEntry;
use super::{Completeness, large_files::FileRecord};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

/// Internal pipeline inputs, deliberately not serializable as renderer records.
pub type Copy = (FileRecord, ObservedEntry);

/// Collapse filesystem identities before any content reads. Ordering is stable
/// because the bounded shared collector has already sorted the paths.
pub fn size_groups(files: Vec<Copy>) -> Vec<Vec<Copy>> {
    let mut identities = HashSet::new();
    let mut sizes = BTreeMap::<u64, Vec<Copy>>::new();
    for file in files {
        if identities.insert(file.1.identity) {
            sizes.entry(file.1.logical_bytes).or_default().push(file);
        }
    }
    sizes.into_values().filter(|g| g.len() > 1).collect()
}

/// Split a phase by its internal digest; failed reads cannot become members.
pub fn digest_groups(files: Vec<(Copy, [u8; 32])>) -> Vec<([u8; 32], Vec<Copy>)> {
    let mut groups = BTreeMap::<[u8; 32], Vec<Copy>>::new();
    for (file, hash) in files {
        groups.entry(hash).or_default().push(file);
    }
    groups.into_iter().filter(|(_, g)| g.len() > 1).collect()
}

/// Rank confirmed groups by reclaimable bytes (size x redundant copies), largest first,
/// matching Kudu. Retention limits then truncate the least valuable groups, not the
/// largest ones. Ties keep digest order so publication stays deterministic.
pub fn rank_by_reclaimable(groups: &mut [([u8; 32], Vec<Copy>)]) {
    let reclaimable = |group: &[Copy]| {
        group[0]
            .1
            .logical_bytes
            .saturating_mul(group.len().saturating_sub(1) as u64)
    };
    groups.sort_by(|a, b| {
        reclaimable(&b.1)
            .cmp(&reclaimable(&a.1))
            .then_with(|| a.0.cmp(&b.0))
    });
}

/// Rebind only explicitly selected rows to a remaining independent copy. Never
/// expands the selection, including when the default keeper itself was selected.
pub fn retain_copy(
    selected: &mut [super::StorageEvidence],
    available: impl Iterator<Item = super::StorageEvidence>,
) -> Result<(), super::StorageError> {
    let available: Vec<_> = available.collect();
    let selected_ids: HashSet<_> = selected.iter().map(|e| e.entry().identity).collect();
    for evidence in selected {
        if let super::StorageEvidence::DuplicateMember { keeper, .. } = evidence {
            let replacement = available.iter().find(|e| {
                matches!(e,
                super::StorageEvidence::DuplicateMember { entry, keeper: other, .. }
                if other.group_id == keeper.group_id && other.full_sha256 == keeper.full_sha256
                    && !selected_ids.contains(&entry.identity))
            });
            if let Some(replacement) = replacement {
                keeper.keeper = replacement.entry().clone();
                keeper.keeper_root = replacement.root().clone();
            } else if selected_ids.contains(&keeper.keeper.identity) {
                return Err(super::StorageError::InvalidEvidence);
            }
        }
    }
    Ok(())
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DuplicateGroup {
    pub group_id: String,
    pub member_count: usize,
    pub independent_copies: usize,
    pub bytes_per_copy: u64,
    pub completeness: Completeness,
    // Members are separately paged; full digests never cross this result boundary.
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DuplicateMember {
    pub group_id: String,
    pub file: FileRecord,
}

#[cfg(test)]
mod tests {
    use super::*;
    fn copy(identity: u64, size: u64) -> Copy {
        let path = format!("C:/fixture/{identity}");
        (
            FileRecord {
                record_id: String::new(),
                display_path: path.clone(),
                logical_bytes: size,
                allocated_bytes: Some(size),
                modified_unix_seconds: Some(1),
                eligibility: super::super::CandidateEligibility::ReadOnly,
            },
            ObservedEntry {
                canonical_path: path.into(),
                identity: crate::FileIdentity {
                    volume: 1,
                    file: identity,
                },
                kind: crate::EntryKind::File,
                logical_bytes: size,
                allocated_bytes: Some(size),
                modified_unix_nanos: 1_000_000_000,
            },
        )
    }
    #[test]
    fn duplicate_pipeline_collapses_identity_and_refines_each_phase() {
        let groups = size_groups(vec![
            copy(1, 10),
            copy(1, 10),
            copy(2, 10),
            copy(3, 10),
            copy(4, 10),
            copy(5, 20),
        ]);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].len(), 4);
        let partial = digest_groups(
            groups
                .into_iter()
                .next()
                .unwrap()
                .into_iter()
                .map(|file| {
                    let digest = if file.1.identity.file == 4 {
                        [2; 32]
                    } else {
                        [1; 32]
                    };
                    (file, digest)
                })
                .collect(),
        );
        assert_eq!(partial.len(), 1);
        assert_eq!(partial[0].1.len(), 3);
        let full = digest_groups(
            partial
                .into_iter()
                .next()
                .unwrap()
                .1
                .into_iter()
                .map(|file| {
                    let digest = if file.1.identity.file == 3 {
                        [4; 32]
                    } else {
                        [3; 32]
                    };
                    (file, digest)
                })
                .collect(),
        );
        assert_eq!(full.len(), 1);
        assert_eq!(full[0].1.len(), 2);
        assert!(size_groups(vec![copy(1, 0), copy(1, 0)]).is_empty());
    }
    #[test]
    fn confirmed_groups_rank_by_reclaimable_bytes_largest_first() {
        let mut groups = vec![
            ([1; 32], vec![copy(1, 10), copy(2, 10)]),
            ([2; 32], vec![copy(3, 1_000), copy(4, 1_000)]),
            (
                [3; 32],
                vec![copy(5, 400), copy(6, 400), copy(7, 400), copy(8, 400)],
            ),
            ([0; 32], vec![copy(9, 10), copy(10, 10)]),
        ];
        rank_by_reclaimable(&mut groups);
        let order: Vec<_> = groups.iter().map(|(digest, _)| digest[0]).collect();
        // 1_200 reclaimable (400 x 3) outranks 1_000 (1_000 x 1); ties keep digest order.
        assert_eq!(order, vec![3, 2, 0, 1]);
    }
    #[test]
    fn duplicate_group_and_members_share_one_record_budget() {
        use super::super::*;
        let mut builder =
            SnapshotBuilder::new("a".repeat(32), StorageModule::Duplicates, 2).unwrap();
        let order = || RecordOrder {
            numeric: 0,
            text: String::new(),
        };
        builder
            .push(
                StorageRecord::DuplicateGroup(DuplicateGroup {
                    group_id: "b".repeat(32),
                    member_count: 2,
                    independent_copies: 2,
                    bytes_per_copy: 10,
                    completeness: Completeness::default(),
                }),
                order(),
            )
            .unwrap();
        for i in 1..=2 {
            let mut file = copy(i, 10).0;
            file.record_id = format!("{i:032x}");
            let result = builder.push(
                StorageRecord::DuplicateMember(DuplicateMember {
                    group_id: "b".repeat(32),
                    file,
                }),
                order(),
            );
            assert_eq!(
                result,
                if i == 1 {
                    Ok(())
                } else {
                    Err(StorageError::LimitReached)
                }
            );
        }
    }
}
