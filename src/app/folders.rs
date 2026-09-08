//! Folder and space-order mutations on [`AppState`].
//!
//! Every workspace appears exactly once in the normalized order, and
//! `AppState::workspaces` stays sorted to its canonical flattening.

use crate::folder::{Folder, SpaceOrderEntry};

use super::state::AppState;

/// Why a folder mutation was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FolderMutationError {
    EmptyName,
    WorkspaceNotFound,
    FolderNotFound,
}

/// Reorder `items` so each worktree family forms one block at its first
/// member's position, parent first.
fn coalesce_blocks<T>(items: Vec<T>, families: &[Option<(String, bool)>]) -> Vec<T> {
    debug_assert_eq!(items.len(), families.len());
    let is_parent = |index: usize| families[index].as_ref().is_some_and(|(_, parent)| *parent);
    let mut order = Vec::with_capacity(items.len());
    let mut placed = vec![false; items.len()];
    for index in 0..items.len() {
        if placed[index] {
            continue;
        }
        let Some((key, _)) = &families[index] else {
            order.push(index);
            placed[index] = true;
            continue;
        };
        let members: Vec<usize> = (index..items.len())
            .filter(|candidate| {
                families[*candidate]
                    .as_ref()
                    .is_some_and(|(candidate_key, _)| candidate_key == key)
            })
            .collect();
        for member in members
            .iter()
            .copied()
            .filter(|member| is_parent(*member))
            .chain(members.iter().copied().filter(|member| !is_parent(*member)))
        {
            order.push(member);
            placed[member] = true;
        }
    }
    if order.iter().enumerate().all(|(index, item)| index == *item) {
        return items;
    }
    let mut slots: Vec<Option<T>> = items.into_iter().map(Some).collect();
    order
        .into_iter()
        .filter_map(|index| slots[index].take())
        .collect()
}

impl AppState {
    /// Create a folder at the end of the top level and return its id.
    pub fn create_folder(&mut self, name: &str) -> Result<String, FolderMutationError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(FolderMutationError::EmptyName);
        }

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

    /// Rename a folder. Names may repeat; the stored name is trimmed.
    pub fn rename_folder(
        &mut self,
        folder_id: &str,
        name: &str,
    ) -> Result<(), FolderMutationError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(FolderMutationError::EmptyName);
        }
        let Some(folder) = self.space_order.iter_mut().find_map(|entry| match entry {
            SpaceOrderEntry::Folder(folder) if folder.id == folder_id => Some(folder),
            _ => None,
        }) else {
            return Err(FolderMutationError::FolderNotFound);
        };
        if folder.name != name {
            folder.name = name.to_string();
            self.mark_session_dirty();
        }
        Ok(())
    }

    /// Delete a folder, releasing its members in place. Returns the released ids.
    pub fn delete_folder(&mut self, folder_id: &str) -> Result<Vec<String>, FolderMutationError> {
        if self.folder(folder_id).is_none() {
            return Err(FolderMutationError::FolderNotFound);
        }

        self.normalize_space_order();
        let Some(index) = self.space_order.iter().position(
            |entry| matches!(entry, SpaceOrderEntry::Folder(folder) if folder.id == folder_id),
        ) else {
            return Err(FolderMutationError::FolderNotFound);
        };
        let released = match self.space_order.remove(index) {
            SpaceOrderEntry::Folder(folder) => folder.members,
            other => {
                self.space_order.insert(index, other);
                return Err(FolderMutationError::FolderNotFound);
            }
        };
        self.space_order.splice(
            index..index,
            released
                .iter()
                .map(|id| SpaceOrderEntry::Workspace(id.clone())),
        );

        self.sync_workspaces_to_space_order();
        self.mark_session_dirty();
        Ok(released)
    }

    /// Assign a workspace (and its worktree family) to a folder, or to the top
    /// level when `folder_id` is `None`. `position` is the block's final index
    /// in the target container; `None` appends. Returns the affected ids.
    pub fn assign_workspace_to_folder(
        &mut self,
        workspace_id: &str,
        folder_id: Option<&str>,
        position: Option<usize>,
    ) -> Result<Vec<String>, FolderMutationError> {
        let Some(workspace) = self.workspaces.iter().find(|ws| ws.id == workspace_id) else {
            return Err(FolderMutationError::WorkspaceNotFound);
        };
        if let Some(folder_id) = folder_id {
            if self.folder(folder_id).is_none() {
                return Err(FolderMutationError::FolderNotFound);
            }
        }

        let family_key = workspace.worktree_space().map(|space| space.key.clone());
        self.normalize_space_order();
        let affected: Vec<String> = match family_key {
            Some(key) => self
                .canonical_workspace_order()
                .into_iter()
                .filter(|id| {
                    self.workspaces.iter().any(|ws| {
                        ws.id == *id && ws.worktree_space().is_some_and(|space| space.key == key)
                    })
                })
                .collect(),
            None => vec![workspace_id.to_string()],
        };

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
                    self.sync_workspaces_to_space_order();
                    return Err(FolderMutationError::FolderNotFound);
                };
                let index = position
                    .unwrap_or(folder.members.len())
                    .min(folder.members.len());
                folder
                    .members
                    .splice(index..index, affected.iter().cloned());
            }
            None => {
                let index = position
                    .unwrap_or(self.space_order.len())
                    .min(self.space_order.len());
                self.space_order.splice(
                    index..index,
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

    /// Move a folder to `position` among top-level entries. Returns the
    /// effective position and whether anything changed.
    pub fn move_folder(
        &mut self,
        folder_id: &str,
        position: usize,
    ) -> Result<(usize, bool), FolderMutationError> {
        if self.folder(folder_id).is_none() {
            return Err(FolderMutationError::FolderNotFound);
        }

        self.normalize_space_order();
        let Some(index) = self.space_order.iter().position(
            |entry| matches!(entry, SpaceOrderEntry::Folder(folder) if folder.id == folder_id),
        ) else {
            return Err(FolderMutationError::FolderNotFound);
        };
        let entry = self.space_order.remove(index);
        let target = position.min(self.space_order.len());
        self.space_order.insert(target, entry);
        let moved = target != index;
        if moved {
            self.sync_workspaces_to_space_order();
            self.mark_session_dirty();
        }
        Ok((target, moved))
    }

    /// Normalize the space order against the live workspaces and re-sort the
    /// workspaces vec if that reordered anything.
    pub(crate) fn normalize_space_order(&mut self) {
        if self.normalize_space_order_entries() {
            self.sync_workspaces_to_space_order();
        }
    }

    /// Normalize without touching the workspaces vec; returns whether the order changed.
    fn normalize_space_order_entries(&mut self) -> bool {
        let ids: Vec<&str> = self.workspaces.iter().map(|ws| ws.id.as_str()).collect();
        let mut normalized = crate::folder::normalized_space_order(&self.space_order, &ids);
        self.coalesce_family_blocks(&mut normalized);
        if normalized == self.space_order {
            return false;
        }
        self.space_order = normalized;
        true
    }

    fn family_membership(&self, workspace_id: &str) -> Option<(String, bool)> {
        self.workspaces
            .iter()
            .find(|ws| ws.id == workspace_id)
            .and_then(|ws| ws.worktree_space())
            .map(|space| (space.key.clone(), !space.is_linked_worktree))
    }

    /// Gather each worktree family into one parent-first block inside its
    /// container, matching how the sidebar draws it.
    fn coalesce_family_blocks(&self, entries: &mut Vec<SpaceOrderEntry>) {
        let top_level: Vec<Option<(String, bool)>> = entries
            .iter()
            .map(|entry| match entry {
                SpaceOrderEntry::Workspace(id) => self.family_membership(id),
                SpaceOrderEntry::Folder(_) => None,
            })
            .collect();
        *entries = coalesce_blocks(std::mem::take(entries), &top_level);
        for entry in entries.iter_mut() {
            if let SpaceOrderEntry::Folder(folder) = entry {
                let members: Vec<Option<(String, bool)>> = folder
                    .members
                    .iter()
                    .map(|id| self.family_membership(id))
                    .collect();
                folder.members = coalesce_blocks(std::mem::take(&mut folder.members), &members);
            }
        }
    }

    /// Re-sort `workspaces` to the canonical order, keeping `active` and `selected` by identity.
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
    }

    /// Drop references to removed workspaces. Leaves the vec untouched: callers
    /// repair `active`/`selected` by index afterwards.
    pub(crate) fn prune_space_order(&mut self) {
        if self.space_order.is_empty() {
            return;
        }
        self.normalize_space_order_entries();
    }

    /// Rebuild the space order from the vec order after an index-based move,
    /// preserving folder membership.
    pub(crate) fn rebuild_space_order_after_reorder(&mut self) {
        if self.space_order.is_empty() {
            return;
        }

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

        for ws in &self.workspaces {
            if let Some(&position) = membership.get(ws.id.as_str()) {
                folders[position].members.push(ws.id.clone());
            }
        }

        // Each folder anchors at its first member; memberless folders append at the end.
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
        self.normalize_space_order();
        self.sync_workspaces_to_space_order();
    }

    /// Install a restored space order, repairing stale data and logging each repair.
    pub(crate) fn install_space_order(&mut self, entries: Vec<SpaceOrderEntry>) {
        crate::folder::reserve_folder_ids(&entries);
        self.space_order = entries;
        if self.space_order.is_empty() {
            return;
        }
        let ids: Vec<&str> = self.workspaces.iter().map(|ws| ws.id.as_str()).collect();
        let (normalized, repairs) =
            crate::folder::normalized_space_order_with_repairs(&self.space_order, &ids);
        self.space_order = normalized;
        repairs.log_restore_warnings();
        self.coalesce_family_blocks_in_place();
        self.heal_split_worktree_families();
        self.sync_workspaces_to_space_order();
    }

    fn coalesce_family_blocks_in_place(&mut self) {
        let mut entries = std::mem::take(&mut self.space_order);
        self.coalesce_family_blocks(&mut entries);
        self.space_order = entries;
    }

    /// Move a split worktree family into the parent checkout's folder.
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
            members = ?members.iter().map(|(id, _)| id.as_str()).collect::<Vec<_>>(),
            target_folder = target.as_deref().unwrap_or("top level"),
            "worktree family split across folders; moving family to the parent's folder"
        );
        if self
            .assign_workspace_to_folder(&anchor, target.as_deref(), None)
            .is_err()
        {
            tracing::warn!(key, "failed to colocate worktree family");
        }
    }

    /// Repair every worktree family split across folders, in vec order.
    pub(crate) fn heal_split_worktree_families(&mut self) {
        if self.space_order.is_empty() {
            return;
        }
        let mut seen = std::collections::HashSet::<String>::new();
        let mut keys: Vec<String> = Vec::new();
        for ws in &self.workspaces {
            if let Some(space) = ws.worktree_space() {
                if seen.insert(space.key.clone()) {
                    keys.push(space.key.clone());
                }
            }
        }
        for key in keys {
            self.ensure_worktree_family_colocated(&key);
        }
    }

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

    pub fn folder(&self, folder_id: &str) -> Option<&Folder> {
        self.space_order.iter().find_map(|entry| match entry {
            SpaceOrderEntry::Folder(folder) if folder.id == folder_id => Some(folder),
            _ => None,
        })
    }

    pub fn workspace_folder_id(&self, workspace_id: &str) -> Option<&str> {
        crate::folder::folder_id_of_workspace(&self.space_order, workspace_id)
    }

    /// Rank of each workspace in the fully expanded folder view: canonical
    /// order with each worktree family grouped at its parent, parent first.
    pub(crate) fn folder_view_workspace_ranks(&self) -> Vec<usize> {
        let mut members = std::collections::HashMap::<&str, Vec<usize>>::new();
        for (idx, ws) in self.workspaces.iter().enumerate() {
            if let Some(space) = ws.worktree_space() {
                members.entry(space.key.as_str()).or_default().push(idx);
            }
        }
        let is_parent = |idx: usize| {
            self.workspaces[idx]
                .worktree_space()
                .is_some_and(|space| !space.is_linked_worktree)
        };
        let grouped: std::collections::HashSet<&str> = members
            .iter()
            .filter(|(_, indices)| indices.len() >= 2 && indices.iter().any(|idx| is_parent(*idx)))
            .map(|(key, _)| *key)
            .collect();

        let mut ranks = vec![usize::MAX; self.workspaces.len()];
        let mut next_rank = 0usize;
        let mut emitted = std::collections::HashSet::<&str>::new();
        for (idx, ws) in self.workspaces.iter().enumerate() {
            let Some(key) = ws
                .worktree_space()
                .map(|space| space.key.as_str())
                .filter(|key| grouped.contains(key))
            else {
                ranks[idx] = next_rank;
                next_rank += 1;
                continue;
            };
            if !emitted.insert(key) {
                continue;
            }
            let Some(group) = members.get(key) else {
                continue;
            };
            let parent = group
                .iter()
                .copied()
                .find(|idx| is_parent(*idx))
                .unwrap_or(idx);
            ranks[parent] = next_rank;
            next_rank += 1;
            for child in group.iter().copied().filter(|idx| *idx != parent) {
                ranks[child] = next_rank;
                next_rank += 1;
            }
        }
        ranks
    }

    /// The space order flattened, with unlisted workspaces appended at the end.
    pub fn canonical_workspace_order(&self) -> Vec<String> {
        let ids: Vec<&str> = self.workspaces.iter().map(|ws| ws.id.as_str()).collect();
        crate::folder::canonical_workspace_ids(&self.space_order, &ids)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::Workspace;

    fn app_with_workspaces(names: &[&str]) -> AppState {
        let mut state = AppState::test_new();
        for name in names {
            state.workspaces.push(Workspace::test_new(name));
        }
        state.ensure_test_terminals();
        if !state.workspaces.is_empty() {
            state.active = Some(0);
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
    fn rename_folder_updates_label_and_trims() {
        let mut state = app_with_workspaces(&["one"]);
        let folder_id = state.create_folder("work").expect("create folder");

        state
            .rename_folder(&folder_id, "  personal  ")
            .expect("rename folder");

        assert_eq!(state.folder(&folder_id).expect("folder").name, "personal");
        state.assert_invariants_for_test();
    }

    #[test]
    fn rename_folder_allows_duplicate_names() {
        let mut state = app_with_workspaces(&["one"]);
        let first = state.create_folder("work").expect("first folder");
        let second = state.create_folder("other").expect("second folder");

        state.rename_folder(&second, "work").expect("rename folder");

        assert_eq!(state.folder(&first).expect("folder").name, "work");
        assert_eq!(state.folder(&second).expect("folder").name, "work");
        state.assert_invariants_for_test();
    }

    #[test]
    fn rename_folder_rejects_empty_names_and_unknown_folders() {
        let mut state = app_with_workspaces(&["one"]);
        let folder_id = state.create_folder("work").expect("create folder");

        assert_eq!(
            state.rename_folder(&folder_id, ""),
            Err(FolderMutationError::EmptyName)
        );
        assert_eq!(
            state.rename_folder(&folder_id, "  \t"),
            Err(FolderMutationError::EmptyName)
        );
        assert_eq!(
            state.rename_folder("f-missing", "personal"),
            Err(FolderMutationError::FolderNotFound)
        );
        assert_eq!(
            state.folder(&folder_id).expect("folder").name,
            "work",
            "rejected renames must not mutate"
        );
        state.assert_invariants_for_test();
    }

    #[test]
    fn delete_folder_releases_members_at_former_position_in_order() {
        let mut state = app_with_workspaces(&["one", "two", "three", "four"]);
        let w1 = workspace_id(&state, 0);
        let w2 = workspace_id(&state, 1);
        let w3 = workspace_id(&state, 2);
        let w4 = workspace_id(&state, 3);
        state.install_space_order(vec![
            SpaceOrderEntry::Workspace(w1.clone()),
            SpaceOrderEntry::Folder(Folder {
                id: "f1".into(),
                name: "work".into(),
                members: vec![w2.clone(), w3.clone()],
            }),
            SpaceOrderEntry::Workspace(w4.clone()),
        ]);

        let released = state.delete_folder("f1").expect("delete folder");

        assert_eq!(released, vec![w2.clone(), w3.clone()]);
        assert!(state.folder("f1").is_none(), "folder must be gone");
        assert_eq!(state.workspace_folder_id(&w2), None);
        assert_eq!(state.workspace_folder_id(&w3), None);
        assert_eq!(
            state.space_order,
            vec![
                SpaceOrderEntry::Workspace(w1.clone()),
                SpaceOrderEntry::Workspace(w2.clone()),
                SpaceOrderEntry::Workspace(w3.clone()),
                SpaceOrderEntry::Workspace(w4.clone()),
            ]
        );
        assert_eq!(workspace_id_order(&state), vec![w1, w2, w3, w4]);
        state.assert_invariants_for_test();
    }

    #[test]
    fn delete_folder_preserves_active_and_selected_by_identity() {
        let mut state = app_with_workspaces(&["one", "two"]);
        let w2 = workspace_id(&state, 1);
        let folder_id = state.create_folder("work").expect("create folder");
        state
            .assign_workspace_to_folder(&w2, Some(&folder_id), None)
            .expect("assign");
        let w2_idx = state
            .workspaces
            .iter()
            .position(|ws| ws.id == w2)
            .expect("workspace present");
        state.active = Some(w2_idx);
        state.selected = w2_idx;

        state.delete_folder(&folder_id).expect("delete folder");

        let new_idx = state
            .workspaces
            .iter()
            .position(|ws| ws.id == w2)
            .expect("workspace still present");
        assert_eq!(state.active, Some(new_idx));
        assert_eq!(state.selected, new_idx);
        state.assert_invariants_for_test();
    }

    #[test]
    fn delete_empty_folder_works() {
        let mut state = app_with_workspaces(&["one"]);
        let folder_id = state.create_folder("work").expect("create folder");
        let before = workspace_id_order(&state);

        let released = state.delete_folder(&folder_id).expect("delete folder");

        assert!(released.is_empty());
        assert!(state.folder(&folder_id).is_none());
        assert_eq!(workspace_id_order(&state), before);
        state.assert_invariants_for_test();
    }

    #[test]
    fn delete_folder_rejects_unknown_folder() {
        let mut state = app_with_workspaces(&["one"]);

        assert_eq!(
            state.delete_folder("f-missing"),
            Err(FolderMutationError::FolderNotFound)
        );
    }

    #[test]
    fn assign_moves_loose_space_into_folder_and_resorts_workspaces() {
        let mut state = app_with_workspaces(&["one", "two", "three"]);
        let w1 = workspace_id(&state, 0);
        let w2 = workspace_id(&state, 1);
        let w3 = workspace_id(&state, 2);
        let folder_id = state.create_folder("work").expect("create folder");

        let affected = state
            .assign_workspace_to_folder(&w1, Some(&folder_id), None)
            .expect("assign");

        assert_eq!(affected, vec![w1.clone()]);
        assert_eq!(
            state.folder(&folder_id).expect("folder").members,
            vec![w1.clone()]
        );
        assert_eq!(state.workspace_folder_id(&w1), Some(folder_id.as_str()));
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
            .assign_workspace_to_folder(&w1, Some(&folder_id), None)
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
            .assign_workspace_to_folder(&w1, Some(&folder_id), None)
            .expect("assign in");

        let affected = state
            .assign_workspace_to_folder(&w1, None, None)
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
            state.assign_workspace_to_folder("w-missing", None, None),
            Err(FolderMutationError::WorkspaceNotFound)
        );
        assert_eq!(
            state.assign_workspace_to_folder(&w1, Some("f-missing"), None),
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
            .assign_workspace_to_folder(&child, Some(&folder_id), None)
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
            .assign_workspace_to_folder(&parent, Some(&folder_id), None)
            .expect("assign in");

        let affected = state
            .assign_workspace_to_folder(&parent, None, None)
            .expect("assign out");

        assert_eq!(affected, vec![parent.clone(), child.clone()]);
        assert_eq!(state.workspace_folder_id(&parent), None);
        assert_eq!(state.workspace_folder_id(&child), None);
        assert_eq!(workspace_id_order(&state), vec![other, parent, child]);
        state.assert_invariants_for_test();
    }

    #[test]
    fn positional_assign_inserts_at_position_inside_folder() {
        let mut state = app_with_workspaces(&["one", "two", "three"]);
        let w1 = workspace_id(&state, 0);
        let w2 = workspace_id(&state, 1);
        let w3 = workspace_id(&state, 2);
        let folder_id = state.create_folder("work").expect("create folder");
        state
            .assign_workspace_to_folder(&w1, Some(&folder_id), None)
            .expect("assign first");
        state
            .assign_workspace_to_folder(&w2, Some(&folder_id), None)
            .expect("assign second");

        let affected = state
            .assign_workspace_to_folder(&w3, Some(&folder_id), Some(1))
            .expect("positional assign");

        assert_eq!(affected, vec![w3.clone()]);
        assert_eq!(
            state.folder(&folder_id).expect("folder").members,
            vec![w1, w3, w2]
        );
        state.assert_invariants_for_test();
    }

    #[test]
    fn positional_assign_within_same_folder_reorders_members() {
        let mut state = app_with_workspaces(&["one", "two", "three"]);
        let w1 = workspace_id(&state, 0);
        let w2 = workspace_id(&state, 1);
        let w3 = workspace_id(&state, 2);
        let folder_id = state.create_folder("work").expect("create folder");
        for id in [&w1, &w2, &w3] {
            state
                .assign_workspace_to_folder(id, Some(&folder_id), None)
                .expect("assign");
        }

        let affected = state
            .assign_workspace_to_folder(&w3, Some(&folder_id), Some(0))
            .expect("reorder within folder");

        assert_eq!(affected, vec![w3.clone()]);
        assert_eq!(
            state.folder(&folder_id).expect("folder").members,
            vec![w3.clone(), w1, w2]
        );
        assert_eq!(state.workspace_folder_id(&w3), Some(folder_id.as_str()));
        state.assert_invariants_for_test();
    }

    #[test]
    fn positional_assign_to_top_level_inserts_at_entry_position() {
        let mut state = app_with_workspaces(&["one", "two", "three"]);
        let w1 = workspace_id(&state, 0);
        let w2 = workspace_id(&state, 1);
        let w3 = workspace_id(&state, 2);
        let folder_id = state.create_folder("work").expect("create folder");
        state
            .assign_workspace_to_folder(&w2, Some(&folder_id), None)
            .expect("assign into folder");

        // Top level is now [w1, w3, folder]; pull w2 out to position 1.
        let affected = state
            .assign_workspace_to_folder(&w2, None, Some(1))
            .expect("positional assign out");

        assert_eq!(affected, vec![w2.clone()]);
        assert_eq!(state.workspace_folder_id(&w2), None);
        assert_eq!(
            state.space_order,
            vec![
                SpaceOrderEntry::Workspace(w1.clone()),
                SpaceOrderEntry::Workspace(w2.clone()),
                SpaceOrderEntry::Workspace(w3.clone()),
                SpaceOrderEntry::Folder(Folder {
                    id: folder_id,
                    name: "work".into(),
                    members: Vec::new(),
                }),
            ]
        );
        assert_eq!(workspace_id_order(&state), vec![w1, w2, w3]);
        state.assert_invariants_for_test();
    }

    #[test]
    fn positional_assign_with_null_folder_reorders_loose_space() {
        let mut state = app_with_workspaces(&["one", "two", "three"]);
        let w1 = workspace_id(&state, 0);
        let w2 = workspace_id(&state, 1);
        let w3 = workspace_id(&state, 2);

        // A loose space with a position is a pure top-level reorder.
        let affected = state
            .assign_workspace_to_folder(&w3, None, Some(0))
            .expect("top-level reorder");

        assert_eq!(affected, vec![w3.clone()]);
        assert_eq!(workspace_id_order(&state), vec![w3, w1, w2]);
        assert_eq!(
            state.canonical_workspace_order(),
            workspace_id_order(&state)
        );
        state.assert_invariants_for_test();
    }

    #[test]
    fn positional_assign_clamps_out_of_range_positions() {
        let mut state = app_with_workspaces(&["one", "two", "three"]);
        let w1 = workspace_id(&state, 0);
        let w2 = workspace_id(&state, 1);
        let w3 = workspace_id(&state, 2);
        let folder_id = state.create_folder("work").expect("create folder");

        state
            .assign_workspace_to_folder(&w1, Some(&folder_id), Some(99))
            .expect("clamped folder assign");
        assert_eq!(
            state.folder(&folder_id).expect("folder").members,
            vec![w1.clone()]
        );

        state
            .assign_workspace_to_folder(&w2, None, Some(99))
            .expect("clamped top-level assign");
        assert_eq!(workspace_id_order(&state), vec![w3, w1, w2.clone()]);
        assert!(
            matches!(
                state.space_order.last(),
                Some(SpaceOrderEntry::Workspace(id)) if *id == w2
            ),
            "clamped position must land at the end: {:?}",
            state.space_order
        );
        state.assert_invariants_for_test();
    }

    #[test]
    fn positional_assign_moves_family_as_contiguous_block() {
        let mut state = app_with_workspaces(&["parent", "child", "one", "two"]);
        mark_family_member(&mut state, 0, "repo-key", false);
        mark_family_member(&mut state, 1, "repo-key", true);
        let parent = workspace_id(&state, 0);
        let child = workspace_id(&state, 1);
        let w1 = workspace_id(&state, 2);
        let w2 = workspace_id(&state, 3);
        let folder_id = state.create_folder("work").expect("create folder");
        for id in [&w1, &w2] {
            state
                .assign_workspace_to_folder(id, Some(&folder_id), None)
                .expect("assign filler");
        }

        let affected = state
            .assign_workspace_to_folder(&child, Some(&folder_id), Some(1))
            .expect("positional family assign");

        assert_eq!(affected, vec![parent.clone(), child.clone()]);
        assert_eq!(
            state.folder(&folder_id).expect("folder").members,
            vec![w1.clone(), parent.clone(), child.clone(), w2.clone()]
        );

        // Family positional moves to the top level keep the block contiguous.
        let affected = state
            .assign_workspace_to_folder(&parent, None, Some(0))
            .expect("positional family move out");
        assert_eq!(affected, vec![parent.clone(), child.clone()]);
        assert_eq!(workspace_id_order(&state), vec![parent, child, w1, w2]);
        state.assert_invariants_for_test();
    }

    #[test]
    fn index_move_that_splits_a_family_coalesces_it_parent_first() {
        let mut state = app_with_workspaces(&["parent", "child", "one", "two"]);
        mark_family_member(&mut state, 0, "repo-key", false);
        mark_family_member(&mut state, 1, "repo-key", true);
        let parent = workspace_id(&state, 0);
        let child = workspace_id(&state, 1);
        let w1 = workspace_id(&state, 2);
        let w2 = workspace_id(&state, 3);
        state.create_folder("work").expect("create folder");

        // Move `one` between the parent and its child: [parent, one, child, two].
        assert!(state.move_workspace(2, 1));
        assert_eq!(
            workspace_id_order(&state),
            vec![parent.clone(), child.clone(), w1.clone(), w2.clone()],
            "the family closes ranks at the parent's position"
        );

        // Move the child ahead of its parent: [child, parent, one, two].
        assert!(state.move_workspace(1, 0));
        assert_eq!(
            workspace_id_order(&state),
            vec![parent.clone(), child.clone(), w1.clone(), w2.clone()],
            "the parent checkout leads its family"
        );

        // A drop before the family lands ahead of the parent, never inside the block.
        state
            .assign_workspace_to_folder(&w2, None, Some(0))
            .expect("assign before the family");
        assert_eq!(workspace_id_order(&state), vec![w2, parent, child, w1]);
        state.assert_invariants_for_test();
    }

    #[test]
    fn create_folder_resorts_an_interleaved_family_and_keeps_focus_identity() {
        let mut state = app_with_workspaces(&["parent", "other", "child"]);
        mark_family_member(&mut state, 0, "repo-key", false);
        mark_family_member(&mut state, 2, "repo-key", true);
        let parent = workspace_id(&state, 0);
        let other = workspace_id(&state, 1);
        let child = workspace_id(&state, 2);
        state.active = Some(1);
        state.selected = 2;

        let folder_id = state.create_folder("work").expect("create folder");

        assert_eq!(
            workspace_id_order(&state),
            vec![parent.clone(), child.clone(), other.clone()],
            "the vec follows the coalesced canonical order"
        );
        assert_eq!(
            state.space_order,
            vec![
                SpaceOrderEntry::Workspace(parent),
                SpaceOrderEntry::Workspace(child.clone()),
                SpaceOrderEntry::Workspace(other.clone()),
                SpaceOrderEntry::Folder(Folder {
                    id: folder_id,
                    name: "work".into(),
                    members: Vec::new(),
                }),
            ]
        );
        assert_eq!(
            state.active.map(|idx| state.workspaces[idx].id.clone()),
            Some(other),
            "active follows identity, not index"
        );
        assert_eq!(state.workspaces[state.selected].id, child);
        state.assert_invariants_for_test();
    }

    #[test]
    fn move_folder_noop_still_resorts_an_interleaved_family() {
        let mut state = app_with_workspaces(&["parent", "other", "child"]);
        let folder_id = state.create_folder("work").expect("create folder");
        // Interleave after the folder exists, as an index move that bypassed the rebuild would.
        mark_family_member(&mut state, 0, "repo-key", false);
        mark_family_member(&mut state, 2, "repo-key", true);
        let parent = workspace_id(&state, 0);
        let other = workspace_id(&state, 1);
        let child = workspace_id(&state, 2);

        let (_, moved) = state.move_folder(&folder_id, 3).expect("noop move");

        assert!(!moved);
        assert_eq!(workspace_id_order(&state), vec![parent, child, other]);
        state.assert_invariants_for_test();
    }

    #[test]
    fn restored_space_order_coalesces_families_inside_folders() {
        let mut state = app_with_workspaces(&["parent", "child", "one"]);
        mark_family_member(&mut state, 0, "repo-key", false);
        mark_family_member(&mut state, 1, "repo-key", true);
        let parent = workspace_id(&state, 0);
        let child = workspace_id(&state, 1);
        let w1 = workspace_id(&state, 2);

        state.install_space_order(vec![SpaceOrderEntry::Folder(Folder {
            id: "f1".into(),
            name: "work".into(),
            members: vec![child.clone(), w1.clone(), parent.clone()],
        })]);

        assert_eq!(
            state.folder("f1").expect("folder").members,
            vec![parent.clone(), child.clone(), w1.clone()]
        );
        assert_eq!(workspace_id_order(&state), vec![parent, child, w1]);
        state.assert_invariants_for_test();
    }

    #[test]
    fn move_folder_repositions_folder_in_top_level_order() {
        let mut state = app_with_workspaces(&["one", "two"]);
        let w1 = workspace_id(&state, 0);
        let w2 = workspace_id(&state, 1);
        let folder_id = state.create_folder("work").expect("create folder");
        state
            .assign_workspace_to_folder(&w2, Some(&folder_id), None)
            .expect("assign");

        // Top level is [w1, folder]; move the folder to the front.
        let (position, moved) = state.move_folder(&folder_id, 0).expect("move folder");

        assert_eq!(position, 0);
        assert!(moved);
        assert!(
            matches!(
                state.space_order.first(),
                Some(SpaceOrderEntry::Folder(folder)) if folder.id == folder_id
            ),
            "folder must lead the top level: {:?}",
            state.space_order
        );
        assert_eq!(state.workspace_folder_id(&w2), Some(folder_id.as_str()));
        assert_eq!(workspace_id_order(&state), vec![w2, w1]);
        state.assert_invariants_for_test();
    }

    #[test]
    fn move_folder_clamps_out_of_range_position() {
        let mut state = app_with_workspaces(&["one", "two"]);
        let folder_id = state.create_folder("work").expect("create folder");
        let (_, _) = state.move_folder(&folder_id, 0).expect("move to front");

        let (position, moved) = state.move_folder(&folder_id, 99).expect("clamped move");

        assert_eq!(position, 2, "clamps to the last top-level index");
        assert!(moved);
        assert!(matches!(
            state.space_order.last(),
            Some(SpaceOrderEntry::Folder(folder)) if folder.id == folder_id
        ));
        state.assert_invariants_for_test();
    }

    #[test]
    fn move_folder_to_current_position_reports_no_move() {
        let mut state = app_with_workspaces(&["one", "two"]);
        let folder_id = state.create_folder("work").expect("create folder");
        let before = state.space_order.clone();

        let (position, moved) = state.move_folder(&folder_id, 2).expect("noop move");

        assert_eq!(position, 2);
        assert!(!moved);
        assert_eq!(state.space_order, before);
        state.assert_invariants_for_test();
    }

    #[test]
    fn move_folder_rejects_unknown_folder() {
        let mut state = app_with_workspaces(&["one"]);

        assert_eq!(
            state.move_folder("f-missing", 0),
            Err(FolderMutationError::FolderNotFound)
        );
    }

    #[test]
    fn new_workspace_appears_loose_at_end_of_canonical_order() {
        let mut state = app_with_workspaces(&["one", "two"]);
        let w1 = workspace_id(&state, 0);
        let w2 = workspace_id(&state, 1);
        let folder_id = state.create_folder("work").expect("create folder");
        state
            .assign_workspace_to_folder(&w1, Some(&folder_id), None)
            .expect("assign");

        state.workspaces.push(Workspace::test_new("new"));
        state.ensure_test_terminals();
        let new_id = state.workspaces.last().expect("new workspace").id.clone();

        assert_eq!(
            state.canonical_workspace_order(),
            vec![w2.clone(), w1.clone(), new_id.clone()]
        );
        state.normalize_space_order();
        assert!(
            matches!(
                state.space_order.last(),
                Some(SpaceOrderEntry::Workspace(id)) if *id == new_id
            ),
            "new space must be loose at the end: {:?}",
            state.space_order
        );
        assert_eq!(state.workspace_folder_id(&new_id), None);
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
            .assign_workspace_to_folder(&w1, Some(&folder_id), None)
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
            .assign_workspace_to_folder(&w2, Some(&folder_id), None)
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
            .assign_workspace_to_folder(&w2, Some(&folder_id), None)
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
            .assign_workspace_to_folder(&parent, Some(&folder_id), None)
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
    fn healing_multiple_split_families_is_deterministic() {
        let build = || {
            let mut state = app_with_workspaces(&["pa", "ca", "pb", "cb"]);
            mark_family_member(&mut state, 0, "repo-a", false);
            mark_family_member(&mut state, 1, "repo-a", true);
            mark_family_member(&mut state, 2, "repo-b", false);
            mark_family_member(&mut state, 3, "repo-b", true);
            let ids: Vec<String> = state.workspaces.iter().map(|ws| ws.id.clone()).collect();
            // Both families split: each parent foldered, each child loose.
            let entries = vec![
                SpaceOrderEntry::Folder(Folder {
                    id: "f1".into(),
                    name: "alpha".into(),
                    members: vec![ids[0].clone()],
                }),
                SpaceOrderEntry::Workspace(ids[1].clone()),
                SpaceOrderEntry::Folder(Folder {
                    id: "f2".into(),
                    name: "beta".into(),
                    members: vec![ids[2].clone()],
                }),
                SpaceOrderEntry::Workspace(ids[3].clone()),
            ];
            state.install_space_order(entries);
            state.assert_invariants_for_test();
            let label_of = |id: &String| {
                state
                    .workspaces
                    .iter()
                    .find(|ws| &ws.id == id)
                    .map(|ws| ws.custom_name.clone().unwrap_or_default())
                    .unwrap_or_default()
            };
            state
                .space_order
                .iter()
                .map(|entry| match entry {
                    SpaceOrderEntry::Workspace(id) => format!("loose:{}", label_of(id)),
                    SpaceOrderEntry::Folder(folder) => format!(
                        "{}[{}]",
                        folder.id,
                        folder
                            .members
                            .iter()
                            .map(label_of)
                            .collect::<Vec<_>>()
                            .join(",")
                    ),
                })
                .collect::<Vec<_>>()
        };

        let first = build();
        let second = build();

        assert_eq!(
            first,
            vec!["f1[pa,ca]".to_string(), "f2[pb,cb]".to_string()],
            "both families heal into their parent's folder"
        );
        assert_eq!(first, second, "healing must be deterministic across runs");
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
