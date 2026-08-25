//! Folders and the space order — shared session facts.
//!
//! A folder is a user-created, named, stable-identity container of spaces
//! (workspaces), one level deep. The space order is the explicit top-level
//! sequence interleaving folders and loose spaces, plus the ordered member
//! list inside each folder. Both are server-owned session facts; presentation
//! state such as collapse lives client-side.

use std::sync::atomic::{AtomicU64, Ordering};

use crate::workspace::{decode_public_number, encode_public_number};

static NEXT_FOLDER_ID: AtomicU64 = AtomicU64::new(1);

/// A user-created, named container of spaces. Identity is the stable `id`;
/// `name` is a mutable label and duplicates are allowed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Folder {
    /// Stable public folder identity, independent of display order.
    pub id: String,
    /// Mutable display label. Never empty or whitespace-only.
    pub name: String,
    /// Ordered workspace ids belonging to this folder.
    pub members: Vec<String>,
}

/// One entry in the top-level space order: a folder or a loose space.
///
/// Workspaces absent from the space order are implicitly loose at the end of
/// the top level, in their `AppState::workspaces` order. [`normalized_space_order`]
/// makes that explicit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpaceOrderEntry {
    Folder(Folder),
    Workspace(String),
}

pub(crate) fn generate_folder_id() -> String {
    let counter = NEXT_FOLDER_ID.fetch_add(1, Ordering::Relaxed);
    format!("f{}", encode_public_number(counter as usize))
}

pub(crate) fn public_folder_number(id: &str) -> Option<usize> {
    id.strip_prefix('f').and_then(decode_public_number)
}

/// Re-seed the folder id counter after restore so newly generated ids cannot
/// collide with restored ones. Mirrors `reserve_workspace_ids`.
pub(crate) fn reserve_folder_ids(entries: &[SpaceOrderEntry]) {
    let Some(next) = entries
        .iter()
        .filter_map(|entry| match entry {
            SpaceOrderEntry::Folder(folder) => public_folder_number(&folder.id),
            SpaceOrderEntry::Workspace(_) => None,
        })
        .max()
        .and_then(|max| u64::try_from(max.checked_add(1)?).ok())
    else {
        return;
    };

    let mut current = NEXT_FOLDER_ID.load(Ordering::Relaxed);
    while current < next {
        match NEXT_FOLDER_ID.compare_exchange_weak(
            current,
            next,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => break,
            Err(observed) => current = observed,
        }
    }
}

/// Repairs applied while normalizing a space order against the live
/// workspaces. The restore path logs each non-empty category so hand-edited
/// or stale snapshots heal silently but observably.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct SpaceOrderRepairs {
    /// References to workspaces that no longer exist, dropped from the order.
    pub dangling_workspace_refs: Vec<String>,
    /// Duplicate appearances of a workspace, dropped (first occurrence wins).
    pub duplicate_workspace_refs: Vec<String>,
    /// Duplicate folder ids whose members merged into the first occurrence.
    pub merged_duplicate_folder_ids: Vec<String>,
    /// Workspaces missing from the order, appended loose at the end.
    pub appended_missing_workspaces: Vec<String>,
}

impl SpaceOrderRepairs {
    /// Whether normalization changed nothing. Test-only: the restore path
    /// logs each category individually via [`Self::log_restore_warnings`].
    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.dangling_workspace_refs.is_empty()
            && self.duplicate_workspace_refs.is_empty()
            && self.merged_duplicate_folder_ids.is_empty()
            && self.appended_missing_workspaces.is_empty()
    }

    /// Log every repair applied while restoring a space order. Repairs are
    /// silent to the user; tracing warnings are their only surface.
    pub fn log_restore_warnings(&self) {
        if !self.dangling_workspace_refs.is_empty() {
            tracing::warn!(
                refs = ?self.dangling_workspace_refs,
                "restored space order referenced missing spaces; dropped the dangling references"
            );
        }
        if !self.duplicate_workspace_refs.is_empty() {
            tracing::warn!(
                refs = ?self.duplicate_workspace_refs,
                "restored space order listed spaces more than once; kept the first appearance"
            );
        }
        if !self.merged_duplicate_folder_ids.is_empty() {
            tracing::warn!(
                folders = ?self.merged_duplicate_folder_ids,
                "restored space order repeated folder ids; merged members into the first occurrence"
            );
        }
        if !self.appended_missing_workspaces.is_empty() {
            tracing::warn!(
                refs = ?self.appended_missing_workspaces,
                "restored space order was missing spaces; appended them loose at the end"
            );
        }
    }
}

/// Normalize a space order against the live workspace ids:
/// - folders are kept (even when empty); duplicate folder ids merge into the
///   first occurrence,
/// - stale (unknown) and duplicate workspace references are dropped
///   (first occurrence wins),
/// - workspaces missing from the order are appended as loose entries at the
///   end, in `workspace_ids` order.
///
/// The result references every live workspace exactly once.
pub(crate) fn normalized_space_order(
    entries: &[SpaceOrderEntry],
    workspace_ids: &[&str],
) -> Vec<SpaceOrderEntry> {
    normalized_space_order_with_repairs(entries, workspace_ids).0
}

/// [`normalized_space_order`] plus a deterministic report of every repair
/// applied, in input order.
pub(crate) fn normalized_space_order_with_repairs(
    entries: &[SpaceOrderEntry],
    workspace_ids: &[&str],
) -> (Vec<SpaceOrderEntry>, SpaceOrderRepairs) {
    let known: std::collections::HashSet<&str> = workspace_ids.iter().copied().collect();
    let mut seen = std::collections::HashSet::<String>::new();
    let mut folder_positions = std::collections::HashMap::<&str, usize>::new();
    let mut normalized: Vec<SpaceOrderEntry> = Vec::with_capacity(entries.len());
    let mut repairs = SpaceOrderRepairs::default();

    fn keep(
        id: &str,
        known: &std::collections::HashSet<&str>,
        seen: &mut std::collections::HashSet<String>,
        repairs: &mut SpaceOrderRepairs,
    ) -> bool {
        if !known.contains(id) {
            repairs.dangling_workspace_refs.push(id.to_string());
            return false;
        }
        if !seen.insert(id.to_string()) {
            repairs.duplicate_workspace_refs.push(id.to_string());
            return false;
        }
        true
    }

    for entry in entries {
        match entry {
            SpaceOrderEntry::Workspace(id) => {
                if keep(id, &known, &mut seen, &mut repairs) {
                    normalized.push(SpaceOrderEntry::Workspace(id.clone()));
                }
            }
            SpaceOrderEntry::Folder(folder) => {
                let members: Vec<String> = folder
                    .members
                    .iter()
                    .filter(|id| keep(id, &known, &mut seen, &mut repairs))
                    .cloned()
                    .collect();
                if let Some(&position) = folder_positions.get(folder.id.as_str()) {
                    if !repairs.merged_duplicate_folder_ids.contains(&folder.id) {
                        repairs.merged_duplicate_folder_ids.push(folder.id.clone());
                    }
                    if let SpaceOrderEntry::Folder(existing) = &mut normalized[position] {
                        existing.members.extend(members);
                    }
                } else {
                    folder_positions.insert(folder.id.as_str(), normalized.len());
                    normalized.push(SpaceOrderEntry::Folder(Folder {
                        id: folder.id.clone(),
                        name: folder.name.clone(),
                        members,
                    }));
                }
            }
        }
    }

    for id in workspace_ids {
        if !seen.contains(*id) {
            repairs.appended_missing_workspaces.push((*id).to_string());
            normalized.push(SpaceOrderEntry::Workspace((*id).to_string()));
        }
    }

    (normalized, repairs)
}

/// Canonical workspace-id order: the space order flattened (folder members in
/// place of their folder), with stale/duplicate references skipped and
/// unlisted workspaces appended at the end in `workspace_ids` order.
pub(crate) fn canonical_workspace_ids(
    entries: &[SpaceOrderEntry],
    workspace_ids: &[&str],
) -> Vec<String> {
    normalized_space_order(entries, workspace_ids)
        .into_iter()
        .flat_map(|entry| match entry {
            SpaceOrderEntry::Workspace(id) => vec![id],
            SpaceOrderEntry::Folder(folder) => folder.members,
        })
        .collect()
}

/// The folder containing `workspace_id`, if any (first occurrence wins,
/// consistent with [`normalized_space_order`]).
pub(crate) fn folder_id_of_workspace<'a>(
    entries: &'a [SpaceOrderEntry],
    workspace_id: &str,
) -> Option<&'a str> {
    for entry in entries {
        match entry {
            SpaceOrderEntry::Workspace(id) if id == workspace_id => return None,
            SpaceOrderEntry::Folder(folder)
                if folder.members.iter().any(|member| member == workspace_id) =>
            {
                return Some(&folder.id)
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(id: &str, name: &str, members: &[&str]) -> SpaceOrderEntry {
        SpaceOrderEntry::Folder(Folder {
            id: id.to_string(),
            name: name.to_string(),
            members: members.iter().map(|id| id.to_string()).collect(),
        })
    }

    fn loose(id: &str) -> SpaceOrderEntry {
        SpaceOrderEntry::Workspace(id.to_string())
    }

    #[test]
    fn generated_folder_ids_are_short_base32_handles() {
        let id = generate_folder_id();
        assert!(id.starts_with('f'), "folder id {id} must start with f");
        assert!(
            public_folder_number(&id).is_some(),
            "folder id {id} must round-trip through the public number codec"
        );
    }

    #[test]
    fn reserving_restored_folder_ids_prevents_reuse() {
        let seeded = generate_folder_id();
        let seeded_number = public_folder_number(&seeded).expect("generated id decodes");
        let restored_number = seeded_number + 50;
        let restored = format!("f{}", encode_public_number(restored_number));

        reserve_folder_ids(&[folder(&restored, "restored", &[])]);

        let next = generate_folder_id();
        let next_number = public_folder_number(&next).expect("generated id decodes");
        assert!(
            next_number > restored_number,
            "next folder id {next} must not collide with restored {restored}"
        );
    }

    #[test]
    fn normalize_reports_every_repair_it_applies() {
        let entries = vec![
            loose("w1"),
            folder("f1", "work", &["w2", "w-gone", "w2"]),
            folder("f1", "work-dup", &["w4"]),
            loose("w1"),
        ];

        let (normalized, repairs) =
            normalized_space_order_with_repairs(&entries, &["w1", "w2", "w3", "w4"]);

        assert_eq!(
            normalized,
            vec![
                loose("w1"),
                folder("f1", "work", &["w2", "w4"]),
                loose("w3")
            ]
        );
        assert_eq!(repairs.dangling_workspace_refs, vec!["w-gone"]);
        assert_eq!(repairs.duplicate_workspace_refs, vec!["w2", "w1"]);
        assert_eq!(repairs.merged_duplicate_folder_ids, vec!["f1"]);
        assert_eq!(repairs.appended_missing_workspaces, vec!["w3"]);
        assert!(!repairs.is_empty());
    }

    #[test]
    fn normalize_reports_no_repairs_for_a_clean_order() {
        let entries = vec![loose("w1"), folder("f1", "work", &["w2"])];

        let (normalized, repairs) = normalized_space_order_with_repairs(&entries, &["w1", "w2"]);

        assert_eq!(normalized, entries);
        assert!(
            repairs.is_empty(),
            "clean input must report no repairs: {repairs:?}"
        );
    }

    #[test]
    fn normalize_repairs_are_deterministic_for_the_same_corrupt_input() {
        let entries = vec![
            folder("f2", "beta", &["w2", "w-gone", "w1"]),
            loose("w1"),
            folder("f2", "beta-dup", &["w3"]),
            loose("w-other-gone"),
        ];
        let ids = ["w1", "w2", "w3", "w4", "w5"];

        let first = normalized_space_order_with_repairs(&entries, &ids);
        let second = normalized_space_order_with_repairs(&entries, &ids);

        assert_eq!(first, second, "same corrupt input must heal identically");
    }

    #[test]
    fn normalize_drops_stale_and_duplicate_references_and_appends_unlisted() {
        let entries = vec![
            loose("w1"),
            folder("f1", "work", &["w2", "w-gone", "w2"]),
            loose("w1"),
            loose("w-gone"),
        ];

        let normalized = normalized_space_order(&entries, &["w1", "w2", "w3"]);

        assert_eq!(
            normalized,
            vec![loose("w1"), folder("f1", "work", &["w2"]), loose("w3")]
        );
    }

    #[test]
    fn normalize_keeps_empty_folders_and_merges_duplicate_folder_ids() {
        let entries = vec![
            folder("f1", "work", &[]),
            folder("f2", "play", &["w1"]),
            folder("f2", "play-dup", &["w2"]),
        ];

        let normalized = normalized_space_order(&entries, &["w1", "w2"]);

        assert_eq!(
            normalized,
            vec![
                folder("f1", "work", &[]),
                folder("f2", "play", &["w1", "w2"])
            ]
        );
    }

    #[test]
    fn canonical_order_flattens_folders_in_place() {
        let entries = vec![loose("w1"), folder("f1", "work", &["w3", "w2"])];

        let canonical = canonical_workspace_ids(&entries, &["w1", "w2", "w3", "w4"]);

        assert_eq!(canonical, vec!["w1", "w3", "w2", "w4"]);
    }

    #[test]
    fn folder_id_of_workspace_reports_membership() {
        let entries = vec![loose("w1"), folder("f1", "work", &["w2"])];

        assert_eq!(folder_id_of_workspace(&entries, "w2"), Some("f1"));
        assert_eq!(folder_id_of_workspace(&entries, "w1"), None);
        assert_eq!(folder_id_of_workspace(&entries, "w9"), None);
    }
}
