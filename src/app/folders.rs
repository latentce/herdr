//! Pure folder / space-order mutations on [`AppState`].
//!
//! Folder identity, membership, per-folder member order, and the top-level
//! space order are shared session facts. These mutations keep two invariants:
//! every workspace appears exactly once in the normalized order (top level
//! xor one folder), and `AppState::workspaces` stays sorted to the canonical
//! flattening of the space order.

use crate::folder::{Folder, SpaceOrderEntry};

use super::state::AppState;

/// Why a folder mutation was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FolderMutationError {
    /// Folder names must contain at least one non-whitespace character.
    EmptyName,
    /// The referenced workspace does not exist.
    WorkspaceNotFound,
    /// The referenced folder does not exist.
    FolderNotFound,
}

impl AppState {
    /// Create a folder with the given name, appended at the end of the
    /// top-level space order. Returns the new folder's stable id.
    /// Duplicate names are allowed; empty/whitespace-only names are rejected.
    pub fn create_folder(&mut self, name: &str) -> Result<String, FolderMutationError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(FolderMutationError::EmptyName);
        }

        // Make the implicit loose order explicit first so the new folder
        // lands after the existing spaces, not before them.
        self.normalize_space_order();
        let folder_id = crate::folder::generate_folder_id();
        self.space_order.push(SpaceOrderEntry::Folder(Folder {
            id: folder_id.clone(),
            name: name.to_string(),
            members: Vec::new(),
        }));
        self.mark_session_dirty();
        Ok(folder_id)
    }

    /// Assign a workspace to a folder, or back to the top level when
    /// `folder_id` is `None`, with append semantics. Assigning any worktree
    /// family member moves the whole family. Returns the affected workspace
    /// ids in canonical order.
    pub fn assign_workspace_to_folder(
        &mut self,
        workspace_id: &str,
        folder_id: Option<&str>,
    ) -> Result<Vec<String>, FolderMutationError> {
        let Some(workspace) = self.workspaces.iter().find(|ws| ws.id == workspace_id) else {
            return Err(FolderMutationError::WorkspaceNotFound);
        };
        if let Some(folder_id) = folder_id {
            if self.folder(folder_id).is_none() {
                return Err(FolderMutationError::FolderNotFound);
            }
        }

        // Family atomicity: assigning any worktree family member moves the
        // whole family, in the members' current canonical order.
        let family_key = workspace.worktree_space().map(|space| space.key.clone());
        let affected: Vec<String> = match family_key {
            Some(key) => self
                .workspaces
                .iter()
                .filter(|ws| ws.worktree_space().is_some_and(|space| space.key == key))
                .map(|ws| ws.id.clone())
                .collect(),
            None => vec![workspace_id.to_string()],
        };

        self.normalize_space_order();
        let affected_set: std::collections::HashSet<&str> =
            affected.iter().map(String::as_str).collect();
        for entry in &mut self.space_order {
            if let SpaceOrderEntry::Folder(folder) = entry {
                folder
                    .members
                    .retain(|member| !affected_set.contains(member.as_str()));
            }
        }
        self.space_order.retain(|entry| match entry {
            SpaceOrderEntry::Workspace(id) => !affected_set.contains(id.as_str()),
            SpaceOrderEntry::Folder(_) => true,
        });

        match folder_id {
            Some(folder_id) => {
                let Some(folder) = self.space_order.iter_mut().find_map(|entry| match entry {
                    SpaceOrderEntry::Folder(folder) if folder.id == folder_id => Some(folder),
                    _ => None,
                }) else {
                    // Unreachable in practice (existence checked above and
                    // normalization keeps folders), but degrade gracefully.
                    self.sync_workspaces_to_space_order();
                    return Err(FolderMutationError::FolderNotFound);
                };
                folder.members.extend(affected.iter().cloned());
            }
            None => {
                self.space_order.extend(
                    affected
                        .iter()
                        .map(|id| SpaceOrderEntry::Workspace(id.clone())),
                );
            }
        }

        self.sync_workspaces_to_space_order();
        self.mark_session_dirty();
        Ok(affected)
    }

    /// Normalize the space order against the live workspaces: prune stale and
    /// duplicate references, merge duplicate folder ids, and make implicitly
    /// loose workspaces explicit at the end of the top level.
    pub(crate) fn normalize_space_order(&mut self) {
        let ids: Vec<&str> = self.workspaces.iter().map(|ws| ws.id.as_str()).collect();
        let normalized = crate::folder::normalized_space_order(&self.space_order, &ids);
        if normalized != self.space_order {
            self.space_order = normalized;
        }
    }

    /// Re-sort `workspaces` to the canonical flattening of the space order,
    /// repairing `active` and `selected` by workspace identity.
    pub(crate) fn sync_workspaces_to_space_order(&mut self) {
        let canonical = self.canonical_workspace_order();
        if self
            .workspaces
            .iter()
            .map(|ws| ws.id.as_str())
            .eq(canonical.iter().map(String::as_str))
        {
            return;
        }

        let active_id = self
            .active
            .and_then(|idx| self.workspaces.get(idx))
            .map(|ws| ws.id.clone());
        let selected_id = self.workspaces.get(self.selected).map(|ws| ws.id.clone());
        let positions: std::collections::HashMap<&str, usize> = canonical
            .iter()
            .enumerate()
            .map(|(index, id)| (id.as_str(), index))
            .collect();
        self.workspaces
            .sort_by_key(|ws| positions.get(ws.id.as_str()).copied().unwrap_or(usize::MAX));
        self.active = active_id.and_then(|id| self.workspaces.iter().position(|ws| ws.id == id));
        self.selected = selected_id
            .and_then(|id| self.workspaces.iter().position(|ws| ws.id == id))
            .unwrap_or(0);
        self.ensure_workspace_visible(self.selected);
    }

    /// Drop space-order references to workspaces that no longer exist.
    /// Called after workspaces are removed from the vec.
    pub(crate) fn prune_space_order(&mut self) {
        if self.space_order.is_empty() {
            return;
        }
        self.normalize_space_order();
    }

    /// Rebuild the space order from the current `workspaces` vec order,
    /// preserving folder membership. Called after index-based workspace
    /// reorders (`move_workspace`, `move_workspace_block`) so the explicit
    /// space order follows the move. Folders are re-anchored at their first
    /// member's position; folders left without members keep their relative
    /// order at the end of the top level.
    pub(crate) fn rebuild_space_order_after_reorder(&mut self) {
        if self.space_order.is_empty() {
            return;
        }

        // Clone each folder once (first occurrence wins on duplicate ids),
        // with members cleared, and map each member to its folder.
        let mut folders: Vec<Folder> = Vec::new();
        let mut folder_positions = std::collections::HashMap::<String, usize>::new();
        let mut membership = std::collections::HashMap::<String, usize>::new();
        for entry in &self.space_order {
            if let SpaceOrderEntry::Folder(folder) = entry {
                let position = *folder_positions
                    .entry(folder.id.clone())
                    .or_insert_with(|| {
                        folders.push(Folder {
                            id: folder.id.clone(),
                            name: folder.name.clone(),
                            members: Vec::new(),
                        });
                        folders.len() - 1
                    });
                for member in &folder.members {
                    membership.entry(member.clone()).or_insert(position);
                }
            }
        }

        // Refill member lists in the new vec order.
        for ws in &self.workspaces {
            if let Some(&position) = membership.get(ws.id.as_str()) {
                folders[position].members.push(ws.id.clone());
            }
        }

        // Interleave: each folder anchors at its first member, loose spaces
        // stay flat, and memberless folders append at the end.
        let mut entries = Vec::with_capacity(self.space_order.len());
        let mut folder_slots: Vec<Option<Folder>> = folders.into_iter().map(Some).collect();
        for ws in &self.workspaces {
            match membership.get(ws.id.as_str()) {
                Some(&position) => {
                    if let Some(folder) = folder_slots[position].take() {
                        entries.push(SpaceOrderEntry::Folder(folder));
                    }
                }
                None => entries.push(SpaceOrderEntry::Workspace(ws.id.clone())),
            }
        }
        for slot in &mut folder_slots {
            if let Some(folder) = slot.take() {
                entries.push(SpaceOrderEntry::Folder(folder));
            }
        }

        self.space_order = entries;
        // A move can interleave a loose workspace between folder members; the
        // rebuild coalesces the folder at its first member, so re-sync the vec
        // to the resulting canonical order.
        self.sync_workspaces_to_space_order();
    }

    /// Install a restored space order: reserve its folder ids against reuse,
    /// then silently repair stale data — dangling references and duplicates
    /// are dropped, unlisted workspaces appended loose, and split worktree
    /// families moved to the parent's folder — before re-sorting the
    /// workspaces vec to the canonical order.
    pub(crate) fn install_space_order(&mut self, entries: Vec<SpaceOrderEntry>) {
        crate::folder::reserve_folder_ids(&entries);
        self.space_order = entries;
        if self.space_order.is_empty() {
            return;
        }
        self.normalize_space_order();
        self.heal_split_worktree_families();
        self.sync_workspaces_to_space_order();
    }

    /// Keep a worktree family in one folder: when members disagree, the whole
    /// family follows the parent checkout's membership (or the first member's
    /// when no parent is present).
    pub(crate) fn ensure_worktree_family_colocated(&mut self, key: &str) {
        if self.space_order.is_empty() {
            return;
        }

        let members: Vec<(String, bool)> = self
            .workspaces
            .iter()
            .filter_map(|ws| {
                ws.worktree_space()
                    .filter(|space| space.key == key)
                    .map(|space| (ws.id.clone(), space.is_linked_worktree))
            })
            .collect();
        if members.len() < 2 {
            return;
        }

        let anchor = members
            .iter()
            .find(|(_, is_linked)| !is_linked)
            .map(|(id, _)| id.clone())
            .unwrap_or_else(|| members[0].0.clone());
        let target = self.workspace_folder_id(&anchor).map(str::to_string);
        let split = members
            .iter()
            .any(|(id, _)| self.workspace_folder_id(id) != target.as_deref());
        if !split {
            return;
        }

        tracing::warn!(
            key,
            target_folder = target.as_deref().unwrap_or("top level"),
            "worktree family split across folders; moving family to the parent's folder"
        );
        if self
            .assign_workspace_to_folder(&anchor, target.as_deref())
            .is_err()
        {
            tracing::warn!(key, "failed to colocate worktree family");
        }
    }

    /// Repair every worktree family that is split across folders. Used after
    /// restoring a session snapshot.
    pub(crate) fn heal_split_worktree_families(&mut self) {
        if self.space_order.is_empty() {
            return;
        }
        let keys: Vec<String> = self
            .workspaces
            .iter()
            .filter_map(|ws| ws.worktree_space().map(|space| space.key.clone()))
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();
        for key in keys {
            self.ensure_worktree_family_colocated(&key);
        }
    }

    /// Test-only invariants for the space order, called from
    /// `AppState::assert_invariants_for_test`.
    #[cfg(test)]
    pub(crate) fn assert_space_order_invariants_for_test(&self) {
        let known: std::collections::HashSet<&str> =
            self.workspaces.iter().map(|ws| ws.id.as_str()).collect();
        let mut folder_ids = std::collections::HashSet::<&str>::new();
        let mut referenced = std::collections::HashSet::<&str>::new();
        for entry in &self.space_order {
            match entry {
                SpaceOrderEntry::Workspace(id) => {
                    assert!(
                        known.contains(id.as_str()),
                        "space order references unknown workspace {id}"
                    );
                    assert!(
                        referenced.insert(id.as_str()),
                        "workspace {id} appears in the space order more than once"
                    );
                }
                SpaceOrderEntry::Folder(folder) => {
                    assert!(
                        folder_ids.insert(folder.id.as_str()),
                        "duplicate folder id {} in space order",
                        folder.id
                    );
                    assert!(
                        !folder.name.trim().is_empty(),
                        "folder {} has an empty name",
                        folder.id
                    );
                    for member in &folder.members {
                        assert!(
                            known.contains(member.as_str()),
                            "folder {} references unknown workspace {member}",
                            folder.id
                        );
                        assert!(
                            referenced.insert(member.as_str()),
                            "workspace {member} appears in the space order more than once"
                        );
                    }
                }
            }
        }

        let canonical = self.canonical_workspace_order();
        let vec_order: Vec<&str> = self.workspaces.iter().map(|ws| ws.id.as_str()).collect();
        assert!(
            canonical
                .iter()
                .map(String::as_str)
                .eq(vec_order.iter().copied()),
            "workspaces vec order {vec_order:?} diverged from canonical space order {canonical:?}"
        );

        let mut family_folders = std::collections::HashMap::<&str, Option<&str>>::new();
        for ws in &self.workspaces {
            let Some(space) = ws.worktree_space() else {
                continue;
            };
            let folder_id = self.workspace_folder_id(&ws.id);
            match family_folders.entry(space.key.as_str()) {
                std::collections::hash_map::Entry::Vacant(entry) => {
                    entry.insert(folder_id);
                }
                std::collections::hash_map::Entry::Occupied(entry) => {
                    assert_eq!(
                        *entry.get(),
                        folder_id,
                        "worktree family {} is split across folders",
                        space.key
                    );
                }
            }
        }
    }

    /// The folder with the given id, if it exists.
    pub fn folder(&self, folder_id: &str) -> Option<&Folder> {
        self.space_order.iter().find_map(|entry| match entry {
            SpaceOrderEntry::Folder(folder) if folder.id == folder_id => Some(folder),
            _ => None,
        })
    }

    /// The folder containing the given workspace, if any.
    pub fn workspace_folder_id(&self, workspace_id: &str) -> Option<&str> {
        crate::folder::folder_id_of_workspace(&self.space_order, workspace_id)
    }

    /// Canonical workspace-id order: the space order flattened, with unlisted
    /// workspaces appended at the end in their current vec order.
    pub fn canonical_workspace_order(&self) -> Vec<String> {
        let ids: Vec<&str> = self.workspaces.iter().map(|ws| ws.id.as_str()).collect();
        crate::folder::canonical_workspace_ids(&self.space_order, &ids)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::state::Mode;
    use crate::workspace::Workspace;

    fn app_with_workspaces(names: &[&str]) -> AppState {
        let mut state = AppState::test_new();
        for name in names {
            state.workspaces.push(Workspace::test_new(name));
        }
        state.ensure_test_terminals();
        if !state.workspaces.is_empty() {
            state.active = Some(0);
            state.mode = Mode::Terminal;
        }
        state
    }

    fn workspace_id(state: &AppState, idx: usize) -> String {
        state.workspaces[idx].id.clone()
    }

    fn workspace_id_order(state: &AppState) -> Vec<String> {
        state.workspaces.iter().map(|ws| ws.id.clone()).collect()
    }

    fn mark_family_member(state: &mut AppState, ws_idx: usize, key: &str, is_linked: bool) {
        state.workspaces[ws_idx].worktree_space = Some(crate::workspace::WorktreeSpaceMembership {
            key: key.into(),
            label: "repo".into(),
            repo_root: "/repo".into(),
            checkout_path: if is_linked {
                format!("/repo/worktree-{ws_idx}").into()
            } else {
                "/repo".into()
            },
            is_linked_worktree: is_linked,
        });
    }

    #[test]
    fn create_folder_returns_f_prefixed_id_and_appends_to_top_level() {
        let mut state = app_with_workspaces(&["one", "two"]);

        let folder_id = state.create_folder("work").expect("create folder");

        assert!(folder_id.starts_with('f'), "folder id {folder_id}");
        let folder = state.folder(&folder_id).expect("folder exists");
        assert_eq!(folder.name, "work");
        assert!(folder.members.is_empty());
        // The folder lands at the end of the top level, after the existing
        // loose spaces.
        assert!(
            matches!(
                state.space_order.last(),
                Some(SpaceOrderEntry::Folder(folder)) if folder.id == folder_id
            ),
            "folder must be the last top-level entry: {:?}",
            state.space_order
        );
        assert_eq!(
            state.canonical_workspace_order(),
            workspace_id_order(&state)
        );
        state.assert_invariants_for_test();
    }

    #[test]
    fn create_folder_rejects_empty_and_whitespace_only_names() {
        let mut state = app_with_workspaces(&["one"]);

        assert_eq!(state.create_folder(""), Err(FolderMutationError::EmptyName));
        assert_eq!(
            state.create_folder("   \t"),
            Err(FolderMutationError::EmptyName)
        );
        assert!(
            state.space_order.is_empty(),
            "rejected names must not mutate"
        );
    }

    #[test]
    fn create_folder_allows_duplicate_names() {
        let mut state = app_with_workspaces(&["one"]);

        let first = state.create_folder("work").expect("first folder");
        let second = state.create_folder("work").expect("second folder");

        assert_ne!(first, second, "duplicate names get distinct ids");
        state.assert_invariants_for_test();
    }

    #[test]
    fn create_folder_trims_surrounding_whitespace() {
        let mut state = app_with_workspaces(&["one"]);

        let folder_id = state.create_folder("  work  ").expect("create folder");

        assert_eq!(state.folder(&folder_id).expect("folder").name, "work");
    }

    #[test]
    fn assign_moves_loose_space_into_folder_and_resorts_workspaces() {
        let mut state = app_with_workspaces(&["one", "two", "three"]);
        let w1 = workspace_id(&state, 0);
        let w2 = workspace_id(&state, 1);
        let w3 = workspace_id(&state, 2);
        let folder_id = state.create_folder("work").expect("create folder");

        let affected = state
            .assign_workspace_to_folder(&w1, Some(&folder_id))
            .expect("assign");

        assert_eq!(affected, vec![w1.clone()]);
        assert_eq!(
            state.folder(&folder_id).expect("folder").members,
            vec![w1.clone()]
        );
        assert_eq!(state.workspace_folder_id(&w1), Some(folder_id.as_str()));
        // The folder sits after the remaining loose spaces, so the canonical
        // order (and the workspaces vec) becomes w2, w3, w1.
        assert_eq!(workspace_id_order(&state), vec![w2, w3, w1]);
        assert_eq!(
            state.canonical_workspace_order(),
            workspace_id_order(&state)
        );
        state.assert_invariants_for_test();
    }

    #[test]
    fn assign_preserves_active_and_selected_by_identity() {
        let mut state = app_with_workspaces(&["one", "two", "three"]);
        let w1 = workspace_id(&state, 0);
        state.active = Some(0);
        state.selected = 0;
        let folder_id = state.create_folder("work").expect("create folder");

        state
            .assign_workspace_to_folder(&w1, Some(&folder_id))
            .expect("assign");

        let new_idx = state
            .workspaces
            .iter()
            .position(|ws| ws.id == w1)
            .expect("workspace still present");
        assert_eq!(state.active, Some(new_idx));
        assert_eq!(state.selected, new_idx);
        state.assert_invariants_for_test();
    }

    #[test]
    fn assign_to_none_returns_space_to_top_level_end() {
        let mut state = app_with_workspaces(&["one", "two"]);
        let w1 = workspace_id(&state, 0);
        let w2 = workspace_id(&state, 1);
        let folder_id = state.create_folder("work").expect("create folder");
        state
            .assign_workspace_to_folder(&w1, Some(&folder_id))
            .expect("assign in");

        let affected = state
            .assign_workspace_to_folder(&w1, None)
            .expect("assign out");

        assert_eq!(affected, vec![w1.clone()]);
        assert_eq!(state.workspace_folder_id(&w1), None);
        assert!(state.folder(&folder_id).expect("folder").members.is_empty());
        assert_eq!(workspace_id_order(&state), vec![w2, w1]);
        assert_eq!(
            state.canonical_workspace_order(),
            workspace_id_order(&state)
        );
        state.assert_invariants_for_test();
    }

    #[test]
    fn assign_rejects_unknown_workspace_and_folder() {
        let mut state = app_with_workspaces(&["one"]);
        let w1 = workspace_id(&state, 0);

        assert_eq!(
            state.assign_workspace_to_folder("w-missing", None),
            Err(FolderMutationError::WorkspaceNotFound)
        );
        assert_eq!(
            state.assign_workspace_to_folder(&w1, Some("f-missing")),
            Err(FolderMutationError::FolderNotFound)
        );
    }

    #[test]
    fn assign_moves_whole_worktree_family_and_reports_all_members() {
        let mut state = app_with_workspaces(&["parent", "child", "other"]);
        mark_family_member(&mut state, 0, "repo-key", false);
        mark_family_member(&mut state, 1, "repo-key", true);
        let parent = workspace_id(&state, 0);
        let child = workspace_id(&state, 1);
        let other = workspace_id(&state, 2);
        let folder_id = state.create_folder("work").expect("create folder");

        let affected = state
            .assign_workspace_to_folder(&child, Some(&folder_id))
            .expect("assign family member");

        assert_eq!(affected, vec![parent.clone(), child.clone()]);
        assert_eq!(
            state.folder(&folder_id).expect("folder").members,
            vec![parent.clone(), child.clone()]
        );
        assert_eq!(state.workspace_folder_id(&parent), Some(folder_id.as_str()));
        assert_eq!(state.workspace_folder_id(&child), Some(folder_id.as_str()));
        assert_eq!(workspace_id_order(&state), vec![other, parent, child]);
        state.assert_invariants_for_test();
    }

    #[test]
    fn assign_family_out_of_folder_moves_all_members_to_top_level() {
        let mut state = app_with_workspaces(&["parent", "child", "other"]);
        mark_family_member(&mut state, 0, "repo-key", false);
        mark_family_member(&mut state, 1, "repo-key", true);
        let parent = workspace_id(&state, 0);
        let child = workspace_id(&state, 1);
        let other = workspace_id(&state, 2);
        let folder_id = state.create_folder("work").expect("create folder");
        state
            .assign_workspace_to_folder(&parent, Some(&folder_id))
            .expect("assign in");

        let affected = state
            .assign_workspace_to_folder(&parent, None)
            .expect("assign out");

        assert_eq!(affected, vec![parent.clone(), child.clone()]);
        assert_eq!(state.workspace_folder_id(&parent), None);
        assert_eq!(state.workspace_folder_id(&child), None);
        assert_eq!(workspace_id_order(&state), vec![other, parent, child]);
        state.assert_invariants_for_test();
    }

    #[test]
    #[should_panic(expected = "unknown workspace")]
    fn invariants_catch_dangling_space_order_reference() {
        let mut state = app_with_workspaces(&["one"]);
        state
            .space_order
            .push(SpaceOrderEntry::Workspace("w-missing".into()));

        state.assert_invariants_for_test();
    }

    #[test]
    #[should_panic(expected = "more than once")]
    fn invariants_catch_duplicate_space_order_membership() {
        let mut state = app_with_workspaces(&["one"]);
        let w1 = workspace_id(&state, 0);
        state
            .space_order
            .push(SpaceOrderEntry::Workspace(w1.clone()));
        state.space_order.push(SpaceOrderEntry::Folder(Folder {
            id: "f1".into(),
            name: "work".into(),
            members: vec![w1],
        }));

        state.assert_invariants_for_test();
    }

    #[test]
    #[should_panic(expected = "worktree family")]
    fn invariants_catch_split_worktree_family() {
        let mut state = app_with_workspaces(&["parent", "child"]);
        mark_family_member(&mut state, 0, "repo-key", false);
        mark_family_member(&mut state, 1, "repo-key", true);
        let parent = workspace_id(&state, 0);
        let child = workspace_id(&state, 1);
        state.space_order = vec![
            SpaceOrderEntry::Workspace(parent),
            SpaceOrderEntry::Folder(Folder {
                id: "f1".into(),
                name: "work".into(),
                members: vec![child],
            }),
        ];

        state.assert_invariants_for_test();
    }

    #[test]
    #[should_panic(expected = "canonical")]
    fn invariants_catch_workspaces_out_of_canonical_order() {
        let mut state = app_with_workspaces(&["one", "two"]);
        let w1 = workspace_id(&state, 0);
        let w2 = workspace_id(&state, 1);
        state.space_order = vec![
            SpaceOrderEntry::Workspace(w2),
            SpaceOrderEntry::Workspace(w1),
        ];

        state.assert_invariants_for_test();
    }

    #[test]
    fn closing_selected_workspace_prunes_it_from_space_order() {
        let mut state = app_with_workspaces(&["one", "two"]);
        let w1 = workspace_id(&state, 0);
        let folder_id = state.create_folder("work").expect("create folder");
        state
            .assign_workspace_to_folder(&w1, Some(&folder_id))
            .expect("assign");
        state.selected = state
            .workspaces
            .iter()
            .position(|ws| ws.id == w1)
            .expect("workspace present");

        state.close_selected_workspace();

        assert!(
            state
                .folder(&folder_id)
                .expect("folder kept")
                .members
                .is_empty(),
            "closed workspace must leave its folder"
        );
        state.assert_invariants_for_test();
    }

    #[test]
    fn move_workspace_rebuilds_space_order_and_keeps_membership() {
        let mut state = app_with_workspaces(&["one", "two", "three"]);
        let w1 = workspace_id(&state, 0);
        let w2 = workspace_id(&state, 1);
        let folder_id = state.create_folder("work").expect("create folder");
        state
            .assign_workspace_to_folder(&w2, Some(&folder_id))
            .expect("assign");
        // Order is now: one, three, [work: two]
        let from = state
            .workspaces
            .iter()
            .position(|ws| ws.id == w1)
            .expect("workspace present");

        assert!(state.move_workspace(from, state.workspaces.len()));

        assert_eq!(state.workspace_folder_id(&w2), Some(folder_id.as_str()));
        state.assert_invariants_for_test();
    }

    #[test]
    fn move_workspace_block_rebuilds_space_order_and_keeps_membership() {
        let mut state = app_with_workspaces(&["one", "two", "three"]);
        let w1 = workspace_id(&state, 0);
        let w2 = workspace_id(&state, 1);
        let w3 = workspace_id(&state, 2);
        let folder_id = state.create_folder("work").expect("create folder");
        state
            .assign_workspace_to_folder(&w2, Some(&folder_id))
            .expect("assign");
        // Order is now: one, three, [work: two]

        assert!(state.move_workspace_block(std::slice::from_ref(&w3), Some(&w1)));

        assert_eq!(state.workspace_folder_id(&w2), Some(folder_id.as_str()));
        assert_eq!(workspace_id_order(&state), vec![w3, w1, w2]);
        state.assert_invariants_for_test();
    }

    #[test]
    fn colocating_family_joins_members_into_parents_folder() {
        let mut state = app_with_workspaces(&["parent", "other"]);
        mark_family_member(&mut state, 0, "repo-key", false);
        let parent = workspace_id(&state, 0);
        let folder_id = state.create_folder("work").expect("create folder");
        state
            .assign_workspace_to_folder(&parent, Some(&folder_id))
            .expect("assign parent");

        // A new worktree child appears loose, then joins the family.
        state.workspaces.push(Workspace::test_new("child"));
        state.ensure_test_terminals();
        let child_idx = state.workspaces.len() - 1;
        mark_family_member(&mut state, child_idx, "repo-key", true);
        let child = state.workspaces[child_idx].id.clone();

        state.ensure_worktree_family_colocated("repo-key");

        assert_eq!(state.workspace_folder_id(&child), Some(folder_id.as_str()));
        assert_eq!(state.workspace_folder_id(&parent), Some(folder_id.as_str()));
        state.assert_invariants_for_test();
    }

    #[test]
    fn colocating_family_with_loose_parent_releases_foldered_child() {
        let mut state = app_with_workspaces(&["parent", "child"]);
        mark_family_member(&mut state, 0, "repo-key", false);
        mark_family_member(&mut state, 1, "repo-key", true);
        let parent = workspace_id(&state, 0);
        let child = workspace_id(&state, 1);
        let folder_id = state.create_folder("work").expect("create folder");
        // Corrupt state: child foldered, parent loose.
        state.space_order = vec![
            SpaceOrderEntry::Workspace(parent.clone()),
            SpaceOrderEntry::Folder(Folder {
                id: folder_id.clone(),
                name: "work".into(),
                members: vec![child.clone()],
            }),
        ];

        state.ensure_worktree_family_colocated("repo-key");

        assert_eq!(state.workspace_folder_id(&parent), None);
        assert_eq!(state.workspace_folder_id(&child), None);
        state.assert_invariants_for_test();
    }

    #[test]
    fn installing_space_order_reorders_workspaces_and_reserves_folder_ids() {
        let mut state = app_with_workspaces(&["one", "two"]);
        let w1 = workspace_id(&state, 0);
        let w2 = workspace_id(&state, 1);
        let restored_folder = "fZZ".to_string();

        state.install_space_order(vec![
            SpaceOrderEntry::Folder(Folder {
                id: restored_folder.clone(),
                name: "restored".into(),
                members: vec![w2.clone()],
            }),
            SpaceOrderEntry::Workspace(w1.clone()),
        ]);

        assert_eq!(workspace_id_order(&state), vec![w2.clone(), w1]);
        assert_eq!(
            state.workspace_folder_id(&w2),
            Some(restored_folder.as_str())
        );
        let fresh = state.create_folder("fresh").expect("create folder");
        assert_ne!(
            fresh, restored_folder,
            "restored folder ids must be reserved against reuse"
        );
        state.assert_invariants_for_test();
    }

    #[test]
    fn installing_space_order_drops_stale_references_and_appends_unlisted() {
        let mut state = app_with_workspaces(&["one", "two"]);
        let w1 = workspace_id(&state, 0);

        state.install_space_order(vec![
            SpaceOrderEntry::Folder(Folder {
                id: "f1".into(),
                name: "work".into(),
                members: vec![w1.clone(), "w-gone".into()],
            }),
            SpaceOrderEntry::Workspace("w-gone".into()),
        ]);

        assert_eq!(state.folder("f1").expect("folder").members, vec![w1]);
        state.assert_invariants_for_test();
    }

    #[test]
    fn installing_space_order_heals_split_families_into_parents_folder() {
        let mut state = app_with_workspaces(&["parent", "child"]);
        mark_family_member(&mut state, 0, "repo-key", false);
        mark_family_member(&mut state, 1, "repo-key", true);
        let parent = workspace_id(&state, 0);
        let child = workspace_id(&state, 1);

        state.install_space_order(vec![
            SpaceOrderEntry::Folder(Folder {
                id: "f1".into(),
                name: "work".into(),
                members: vec![parent.clone()],
            }),
            SpaceOrderEntry::Workspace(child.clone()),
        ]);

        assert_eq!(state.workspace_folder_id(&parent), Some("f1"));
        assert_eq!(state.workspace_folder_id(&child), Some("f1"));
        state.assert_invariants_for_test();
    }

    #[test]
    fn installing_empty_space_order_keeps_prior_workspace_order() {
        let mut state = app_with_workspaces(&["one", "two"]);
        let before = workspace_id_order(&state);

        state.install_space_order(Vec::new());

        assert_eq!(workspace_id_order(&state), before);
        assert!(state.space_order.is_empty());
        state.assert_invariants_for_test();
    }
}
