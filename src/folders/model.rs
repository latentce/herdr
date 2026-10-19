//! Pure folder state and the algorithms that keep it consistent with the
//! server's flat workspace order. Nothing here touches PTYs, files, or events.

use std::collections::{HashMap, HashSet};

pub(crate) type FolderId = u64;

pub(crate) fn public_folder_id(id: FolderId) -> String {
    format!("f{id}")
}

pub(crate) fn parse_folder_id(value: &str) -> Option<FolderId> {
    let digits = value.trim().strip_prefix('f')?;
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok().filter(|id| *id > 0)
}

/// Trims a folder name; an empty result is not a valid name.
pub(crate) fn normalize_folder_name(name: &str) -> Option<String> {
    let trimmed = name.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WorktreeFact {
    pub(crate) key: String,
    pub(crate) linked: bool,
}

/// What folder logic needs to know about one workspace in the flat order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WorkspaceFact {
    pub(crate) id: String,
    pub(crate) worktree: Option<WorktreeFact>,
}

impl WorkspaceFact {
    fn matches(&self, id: &str, worktree: Option<(&str, bool)>) -> bool {
        self.id == id
            && match (&self.worktree, worktree) {
                (None, None) => true,
                (Some(fact), Some((key, linked))) => fact.key == key && fact.linked == linked,
                _ => false,
            }
    }
}

/// One entry in the combined top-level order.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Item {
    Workspace(String),
    Folder(FolderId),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Folder {
    pub(crate) id: FolderId,
    pub(crate) name: String,
    /// Members in display order: worktree parents are followed by their children.
    pub(crate) members: Vec<String>,
}

/// A neighbour used to express a position.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum FolderRef {
    Workspace(String),
    Folder(FolderId),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Placement {
    Before(FolderRef),
    After(FolderRef),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MembershipChange {
    pub(crate) workspace_id: String,
    pub(crate) from: Option<FolderId>,
    pub(crate) to: Option<FolderId>,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct FolderOutcome {
    /// The new flat workspace order to apply, when it changed.
    pub(crate) flat: Option<Vec<String>>,
    pub(crate) membership: Vec<MembershipChange>,
    /// Names, membership, or the combined order changed.
    pub(crate) changed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FolderError {
    pub(crate) code: &'static str,
    pub(crate) message: String,
}

impl FolderError {
    pub(crate) fn folder_not_found(id: FolderId) -> Self {
        Self {
            code: "folder_not_found",
            message: format!("folder {} not found", public_folder_id(id)),
        }
    }

    pub(crate) fn workspace_not_found(id: &str) -> Self {
        Self {
            code: "workspace_not_found",
            message: format!("workspace {id} not found"),
        }
    }

    pub(crate) fn invalid_position(message: impl Into<String>) -> Self {
        Self {
            code: "folder_position_invalid",
            message: message.into(),
        }
    }

    pub(crate) fn invalid_name() -> Self {
        Self {
            code: "folder_name_invalid",
            message: "folder name must not be empty".into(),
        }
    }

    /// Only reachable when saved folder data carries ids near the integer limit.
    pub(crate) fn ids_exhausted() -> Self {
        Self {
            code: "folder_ids_exhausted",
            message: "no folder ids are left; delete the folder with the highest id".into(),
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct FolderState {
    folders: Vec<Folder>,
    order: Vec<Item>,
    next_id: FolderId,
    member_index: HashMap<String, FolderId>,
    /// The flat order the folder state was last reconciled against.
    seen: Option<Vec<WorkspaceFact>>,
    revision: u64,
    pub(crate) storage: super::store::FolderStorage,
}

impl FolderState {
    pub(crate) fn is_empty(&self) -> bool {
        self.folders.is_empty()
    }

    /// Increases whenever names, membership, or the combined order change.
    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }

    pub(crate) fn next_id(&self) -> FolderId {
        self.next_id.max(1)
    }

    pub(crate) fn order(&self) -> &[Item] {
        &self.order
    }

    pub(crate) fn folder(&self, id: FolderId) -> Option<&Folder> {
        self.folders.iter().find(|folder| folder.id == id)
    }

    pub(crate) fn folder_of(&self, workspace_id: &str) -> Option<FolderId> {
        if self.member_index.is_empty() {
            return None;
        }
        self.member_index.get(workspace_id).copied()
    }

    /// Index of a folder in the combined order of folders and loose workspaces.
    /// Every workspace id filed in a folder.
    pub(crate) fn member_ids(&self) -> impl Iterator<Item = &str> {
        self.member_index.keys().map(String::as_str)
    }

    pub(crate) fn position(&self, id: FolderId) -> Option<usize> {
        self.order.iter().position(|item| *item == Item::Folder(id))
    }

    /// Folders in combined order, with their positions.
    pub(crate) fn folders_in_order(&self) -> impl Iterator<Item = (usize, &Folder)> {
        let order = if self.folders.is_empty() {
            &[][..]
        } else {
            &self.order[..]
        };
        order
            .iter()
            .enumerate()
            .filter_map(|(position, item)| match item {
                Item::Folder(id) => self.folder(*id).map(|folder| (position, folder)),
                Item::Workspace(_) => None,
            })
    }

    /// Replaces the state with saved data. The next sync reconciles it.
    pub(crate) fn restore(&mut self, folders: Vec<Folder>, order: Vec<Item>, next_id: FolderId) {
        let mut member_index = HashMap::new();
        let mut unique = Vec::with_capacity(folders.len());
        let mut ids = HashSet::new();
        for mut folder in folders {
            if folder.id == 0 || !ids.insert(folder.id) {
                continue;
            }
            folder
                .members
                .retain(|member| member_index.insert(member.clone(), folder.id).is_none());
            unique.push(folder);
        }
        let highest = unique.iter().map(|folder| folder.id).max().unwrap_or(0);
        self.folders = unique;
        self.order = order;
        self.member_index = member_index;
        self.next_id = next_id.max(highest.saturating_add(1)).max(1);
        self.seen = None;
        self.revision += 1;
    }

    /// Whether the given flat order matches what the state was reconciled against.
    pub(crate) fn is_synced<'a>(
        &self,
        mut workspaces: impl ExactSizeIterator<Item = (&'a str, Option<(&'a str, bool)>)>,
    ) -> bool {
        let Some(seen) = &self.seen else {
            return false;
        };
        seen.len() == workspaces.len()
            && seen.iter().all(|fact| {
                workspaces
                    .next()
                    .is_some_and(|(id, worktree)| fact.matches(id, worktree))
            })
    }

    /// Folds a changed flat order into folder state: drops closed workspaces,
    /// places worktree groups, and gathers each folder's members together.
    pub(crate) fn reconcile(&mut self, flat: Vec<WorkspaceFact>) -> FolderOutcome {
        let before = (self.order.clone(), self.folders.clone());
        let present = flat
            .iter()
            .map(|fact| fact.id.as_str())
            .collect::<HashSet<_>>();
        self.member_index
            .retain(|workspace, _| present.contains(workspace.as_str()));
        let (membership, newcomers) = self.normalize_groups(&flat);
        let previous_order = std::mem::take(&mut self.order);
        let desired = self.derive(&flat, &newcomers, &previous_order);
        let flat_changed = !desired.iter().eq(flat.iter().map(|fact| &fact.id));
        self.seen = Some(reorder(&flat, &desired));
        let changed = before != (self.order.clone(), self.folders.clone());
        if changed {
            self.revision += 1;
        }
        FolderOutcome {
            flat: flat_changed.then_some(desired),
            membership,
            changed,
        }
    }

    pub(crate) fn create(
        &mut self,
        name: String,
        workspaces: &[String],
        placement: Option<Placement>,
    ) -> Result<(FolderId, FolderOutcome), FolderError> {
        let flat = self.seen_facts();
        let layout = Layout::new(&flat);
        let mut units = Vec::<Vec<String>>::new();
        let mut moved = HashSet::<String>::new();
        for workspace in workspaces {
            if !layout.contains(workspace) {
                return Err(FolderError::workspace_not_found(workspace));
            }
            if moved.contains(workspace) {
                continue;
            }
            let unit = layout.unit_members(workspace);
            moved.extend(unit.iter().cloned());
            units.push(unit);
        }
        let slot = match &placement {
            Some(placement) => self.top_slot(&layout, placement)?,
            None => match units.first().and_then(|unit| unit.first()) {
                Some(first) => self.default_slot_for(&layout, first),
                None => self.order.len(),
            },
        };
        let (mut edited, slot) = without_workspace_items(&self.order, &moved, slot);
        let id = self.allocate_id()?;
        edited.insert(slot, Item::Folder(id));
        let block = units.concat();
        let remaining = flat_without(&flat, &moved);
        let at = self.flat_index_for_top_slot(&layout, &edited, slot + 1, &remaining);
        let new_flat = splice(remaining, at, &block);
        let mut changes = Vec::new();
        for workspace in &block {
            let from = self.member_index.insert(workspace.clone(), id);
            changes.push(MembershipChange {
                workspace_id: workspace.clone(),
                from,
                to: Some(id),
            });
        }
        self.folders.push(Folder {
            id,
            name,
            members: block,
        });
        Ok((id, self.finish(&flat, new_flat, edited, changes)))
    }

    /// Returns whether the name changed.
    pub(crate) fn rename(&mut self, id: FolderId, name: String) -> Result<bool, FolderError> {
        let folder = self
            .folders
            .iter_mut()
            .find(|folder| folder.id == id)
            .ok_or_else(|| FolderError::folder_not_found(id))?;
        if folder.name == name {
            return Ok(false);
        }
        folder.name = name;
        self.revision += 1;
        Ok(true)
    }

    /// Removes the folder; its members become loose workspaces in place.
    pub(crate) fn delete(&mut self, id: FolderId) -> Result<FolderOutcome, FolderError> {
        let flat = self.seen_facts();
        let index = self
            .folders
            .iter()
            .position(|folder| folder.id == id)
            .ok_or_else(|| FolderError::folder_not_found(id))?;
        let folder = self.folders.remove(index);
        let mut edited = Vec::with_capacity(self.order.len() + folder.members.len());
        for item in &self.order {
            if *item == Item::Folder(id) {
                edited.extend(folder.members.iter().cloned().map(Item::Workspace));
            } else {
                edited.push(item.clone());
            }
        }
        let mut changes = Vec::new();
        for member in &folder.members {
            if self.member_index.remove(member).is_some() {
                changes.push(MembershipChange {
                    workspace_id: member.clone(),
                    from: Some(id),
                    to: None,
                });
            }
        }
        let current = flat.iter().map(|fact| fact.id.clone()).collect();
        Ok(self.finish(&flat, current, edited, changes))
    }

    /// Moves a folder, with its members, to a top-level position.
    pub(crate) fn move_folder(
        &mut self,
        id: FolderId,
        placement: Placement,
    ) -> Result<FolderOutcome, FolderError> {
        let flat = self.seen_facts();
        let layout = Layout::new(&flat);
        let members = self
            .folder(id)
            .ok_or_else(|| FolderError::folder_not_found(id))?
            .members
            .clone();
        let slot = self.top_slot(&layout, &placement)?;
        if matches!(placement, Placement::Before(FolderRef::Folder(target)) | Placement::After(FolderRef::Folder(target)) if target == id)
        {
            return Ok(FolderOutcome::default());
        }
        let current = self.position(id).unwrap_or(self.order.len());
        let mut edited = self.order.clone();
        if current < edited.len() {
            edited.remove(current);
        }
        let slot = if current < slot { slot - 1 } else { slot };
        edited.insert(slot, Item::Folder(id));
        let moved = members.iter().cloned().collect::<HashSet<_>>();
        let remaining = flat_without(&flat, &moved);
        let at = self.flat_index_for_top_slot(&layout, &edited, slot + 1, &remaining);
        let new_flat = splice(remaining, at, &members);
        Ok(self.finish(&flat, new_flat, edited, Vec::new()))
    }

    /// Puts a workspace, with its whole worktree group, into a folder.
    pub(crate) fn add_workspace(
        &mut self,
        id: FolderId,
        workspace: &str,
        placement: Option<Placement>,
    ) -> Result<FolderOutcome, FolderError> {
        let flat = self.seen_facts();
        let layout = Layout::new(&flat);
        let target = self
            .folder(id)
            .ok_or_else(|| FolderError::folder_not_found(id))?;
        if !layout.contains(workspace) {
            return Err(FolderError::workspace_not_found(workspace));
        }
        let unit = layout.unit_members(workspace);
        let moved = unit.iter().cloned().collect::<HashSet<_>>();
        let source = self.folder_of(workspace);
        let mut members = target
            .members
            .iter()
            .filter(|member| !moved.contains(*member))
            .cloned()
            .collect::<Vec<_>>();
        let member_slot = match &placement {
            None => members.len(),
            Some(placement) => {
                let (reference, after) = match placement {
                    Placement::Before(reference) => (reference, false),
                    Placement::After(reference) => (reference, true),
                };
                let FolderRef::Workspace(reference) = reference else {
                    return Err(FolderError::invalid_position(
                        "a position inside a folder must name one of its workspaces",
                    ));
                };
                if !layout.contains(reference) {
                    return Err(FolderError::workspace_not_found(reference));
                }
                if self.folder_of(reference) != Some(id) {
                    return Err(FolderError::invalid_position(format!(
                        "workspace {reference} is not in folder {}",
                        public_folder_id(id)
                    )));
                }
                if moved.contains(reference) {
                    return Ok(FolderOutcome::default());
                }
                let (start, end) = unit_range(&members, &layout, reference);
                if after {
                    end
                } else {
                    start
                }
            }
        };
        let (edited, _) = if source.is_none() {
            without_workspace_items(&self.order, &moved, 0)
        } else {
            (self.order.clone(), 0)
        };
        let remaining = flat_without(&flat, &moved);
        let positions = flat_positions(&remaining);
        let at = if let Some(next) = members.get(member_slot) {
            positions
                .get(next.as_str())
                .copied()
                .unwrap_or(remaining.len())
        } else if let Some(last) = members
            .iter()
            .filter_map(|member| positions.get(member.as_str()))
            .max()
        {
            last + 1
        } else {
            let folder_slot = edited
                .iter()
                .position(|item| *item == Item::Folder(id))
                .unwrap_or(edited.len());
            self.flat_index_for_top_slot(&layout, &edited, folder_slot + 1, &remaining)
        };
        members.splice(member_slot..member_slot, unit.iter().cloned());
        if let Some(folder) = self.folders.iter_mut().find(|folder| folder.id == id) {
            folder.members = members;
        }
        let mut changes = Vec::new();
        for member in &unit {
            let from = self.member_index.insert(member.clone(), id);
            if from != Some(id) {
                changes.push(MembershipChange {
                    workspace_id: member.clone(),
                    from,
                    to: Some(id),
                });
            }
        }
        let new_flat = splice(remaining, at, &unit);
        Ok(self.finish(&flat, new_flat, edited, changes))
    }

    /// Takes a workspace, with its whole worktree group, out of its folder.
    pub(crate) fn remove_workspace(
        &mut self,
        workspace: &str,
        placement: Option<Placement>,
    ) -> Result<FolderOutcome, FolderError> {
        let flat = self.seen_facts();
        let layout = Layout::new(&flat);
        if !layout.contains(workspace) {
            return Err(FolderError::workspace_not_found(workspace));
        }
        let source = self.folder_of(workspace);
        let unit = layout.unit_members(workspace);
        let moved = unit.iter().cloned().collect::<HashSet<_>>();
        let slot = match (&placement, source) {
            (None, None) => return Ok(FolderOutcome::default()),
            (None, Some(source)) => self.position(source).map_or(self.order.len(), |at| at + 1),
            (Some(placement), _) => {
                let reference = match placement {
                    Placement::Before(reference) | Placement::After(reference) => reference,
                };
                if matches!(reference, FolderRef::Workspace(id) if moved.contains(id)) {
                    return Ok(FolderOutcome::default());
                }
                self.top_slot(&layout, placement)?
            }
        };
        let (mut edited, slot) = without_workspace_items(&self.order, &moved, slot);
        for (offset, member) in unit.iter().enumerate() {
            edited.insert(slot + offset, Item::Workspace(member.clone()));
        }
        let remaining = flat_without(&flat, &moved);
        let at = self.flat_index_for_top_slot(&layout, &edited, slot + unit.len(), &remaining);
        let mut changes = Vec::new();
        for member in &unit {
            if let Some(from) = self.member_index.remove(member) {
                changes.push(MembershipChange {
                    workspace_id: member.clone(),
                    from: Some(from),
                    to: None,
                });
            }
        }
        let new_flat = splice(remaining, at, &unit);
        Ok(self.finish(&flat, new_flat, edited, changes))
    }

    /// Clamps a legacy flat move so it never changes folder membership.
    ///
    /// `remaining` is the flat order without the moving workspaces and
    /// `target` the requested insertion index into it. A folder member stays
    /// inside its folder; anything else lands just outside the nearest edge
    /// of a folder it would otherwise split.
    pub(crate) fn clamp_flat_insert(
        &self,
        remaining: &[&str],
        moving: &[&str],
        target: usize,
    ) -> usize {
        let target = target.min(remaining.len());
        if let Some(folder) = moving.first().and_then(|id| self.folder_of(id)) {
            let block = remaining
                .iter()
                .enumerate()
                .filter(|(_, id)| self.folder_of(id) == Some(folder))
                .map(|(index, _)| index)
                .collect::<Vec<_>>();
            if let (Some(first), Some(last)) = (block.first(), block.last()) {
                return target.clamp(*first, last + 1);
            }
        }
        if target == 0 || target >= remaining.len() {
            return target;
        }
        let Some(folder) = self
            .folder_of(remaining[target - 1])
            .filter(|folder| self.folder_of(remaining[target]) == Some(*folder))
        else {
            return target;
        };
        let mut start = target;
        while start > 0 && self.folder_of(remaining[start - 1]) == Some(folder) {
            start -= 1;
        }
        let mut end = target;
        while end < remaining.len() && self.folder_of(remaining[end]) == Some(folder) {
            end += 1;
        }
        if target - start <= end - target {
            start
        } else {
            end
        }
    }

    fn seen_facts(&self) -> Vec<WorkspaceFact> {
        self.seen.clone().unwrap_or_default()
    }

    fn allocate_id(&mut self) -> Result<FolderId, FolderError> {
        let highest = self
            .folders
            .iter()
            .map(|folder| folder.id)
            .max()
            .unwrap_or(0);
        let id = highest
            .checked_add(1)
            .map(|successor| self.next_id.max(successor).max(1));
        let (id, next_id) = id
            .and_then(|id| Some((id, id.checked_add(1)?)))
            .ok_or_else(FolderError::ids_exhausted)?;
        self.next_id = next_id;
        Ok(id)
    }

    fn finish(
        &mut self,
        original: &[WorkspaceFact],
        new_flat: Vec<String>,
        edited: Vec<Item>,
        membership: Vec<MembershipChange>,
    ) -> FolderOutcome {
        let order_before = self.order.clone();
        let facts = reorder(original, &new_flat);
        let desired = self.derive(&facts, &HashSet::new(), &edited);
        let flat_changed = !desired.iter().eq(original.iter().map(|fact| &fact.id));
        self.seen = Some(reorder(original, &desired));
        let changed = flat_changed || !membership.is_empty() || self.order != order_before;
        if changed {
            self.revision += 1;
        }
        FolderOutcome {
            flat: flat_changed.then_some(desired),
            membership,
            changed,
        }
    }

    /// Default top-level slot for a new folder made from `workspace`: where
    /// it sits when loose, otherwise right after its folder.
    fn default_slot_for(&self, layout: &Layout<'_>, workspace: &str) -> usize {
        if let Some(folder) = self.folder_of(workspace) {
            return self.position(folder).map_or(self.order.len(), |at| at + 1);
        }
        self.order
            .iter()
            .position(|item| matches!(item, Item::Workspace(id) if id == workspace))
            .map_or(self.order.len(), |index| {
                item_unit_range(&self.order, layout, index).0
            })
    }

    fn top_slot(&self, layout: &Layout<'_>, placement: &Placement) -> Result<usize, FolderError> {
        let (reference, after) = match placement {
            Placement::Before(reference) => (reference, false),
            Placement::After(reference) => (reference, true),
        };
        match reference {
            FolderRef::Folder(id) => {
                let index = self
                    .position(*id)
                    .ok_or_else(|| FolderError::folder_not_found(*id))?;
                Ok(index + usize::from(after))
            }
            FolderRef::Workspace(workspace) => {
                if !layout.contains(workspace) {
                    return Err(FolderError::workspace_not_found(workspace));
                }
                if let Some(folder) = self.folder_of(workspace) {
                    return Err(FolderError::invalid_position(format!(
                        "workspace {workspace} is inside folder {}; name a folder or a loose workspace",
                        public_folder_id(folder)
                    )));
                }
                let index = self
                    .order
                    .iter()
                    .position(|item| matches!(item, Item::Workspace(id) if id == workspace))
                    .ok_or_else(|| FolderError::workspace_not_found(workspace))?;
                let (start, end) = item_unit_range(&self.order, layout, index);
                Ok(if after { end } else { start })
            }
        }
    }

    /// Flat insertion index that puts a block right before the first item at
    /// or after `slot` in `order` that has workspaces in `remaining`.
    fn flat_index_for_top_slot(
        &self,
        layout: &Layout<'_>,
        order: &[Item],
        slot: usize,
        remaining: &[String],
    ) -> usize {
        let positions = flat_positions(remaining);
        for item in order.iter().skip(slot) {
            let anchor = match item {
                Item::Folder(id) => self.folder(*id).and_then(|folder| {
                    folder
                        .members
                        .iter()
                        .filter_map(|member| positions.get(member.as_str()))
                        .min()
                        .copied()
                }),
                Item::Workspace(workspace) => layout
                    .unit_members(workspace)
                    .iter()
                    .filter_map(|member| positions.get(member.as_str()))
                    .min()
                    .copied(),
            };
            if let Some(anchor) = anchor {
                return anchor;
            }
        }
        remaining.len()
    }

    /// Applies the worktree-group placement rule: a group lives in its
    /// parents' folder, otherwise in the folder of any member already in one.
    fn normalize_groups(
        &mut self,
        flat: &[WorkspaceFact],
    ) -> (Vec<MembershipChange>, HashSet<String>) {
        let layout = Layout::new(flat);
        let mut changes = Vec::new();
        let mut newcomers = HashSet::new();
        let mut handled = HashSet::new();
        for index in 0..flat.len() {
            let Some(key) = layout.group_key(index) else {
                continue;
            };
            if !handled.insert(key) {
                continue;
            }
            let members = flat
                .iter()
                .filter(|member| {
                    member
                        .worktree
                        .as_ref()
                        .is_some_and(|worktree| worktree.key == key)
                })
                .collect::<Vec<_>>();
            let target = members
                .iter()
                .filter(|member| member.worktree.as_ref().is_some_and(|w| !w.linked))
                .find_map(|member| self.folder_of(&member.id))
                .or_else(|| members.iter().find_map(|member| self.folder_of(&member.id)));
            for member in members {
                let current = self.folder_of(&member.id);
                if current == target {
                    continue;
                }
                match target {
                    Some(folder) => {
                        self.member_index.insert(member.id.clone(), folder);
                        newcomers.insert(member.id.clone());
                    }
                    None => {
                        self.member_index.remove(&member.id);
                    }
                }
                changes.push(MembershipChange {
                    workspace_id: member.id.clone(),
                    from: current,
                    to: target,
                });
            }
        }
        (changes, newcomers)
    }

    /// Rebuilds folder member lists and the combined order from the flat
    /// order and membership, and returns the flat order with every folder's
    /// members gathered at the folder's position.
    fn derive(
        &mut self,
        flat: &[WorkspaceFact],
        newcomers: &HashSet<String>,
        previous_order: &[Item],
    ) -> Vec<String> {
        let layout = Layout::new(flat);

        #[derive(Default)]
        struct Unit {
            anchor: Option<usize>,
            first: usize,
            parents: Vec<usize>,
            children: Vec<usize>,
        }
        let mut units = HashMap::<FolderId, Vec<(UnitKey<'_>, Unit)>>::new();
        for (index, fact) in flat.iter().enumerate() {
            let Some(folder) = self.folder_of(&fact.id) else {
                continue;
            };
            let key = layout.unit_key(index);
            let folder_units = units.entry(folder).or_default();
            let unit = match folder_units
                .iter()
                .position(|(existing, _)| *existing == key)
            {
                Some(position) => &mut folder_units[position].1,
                None => {
                    folder_units.push((
                        key,
                        Unit {
                            first: index,
                            ..Unit::default()
                        },
                    ));
                    let last = folder_units.len() - 1;
                    &mut folder_units[last].1
                }
            };
            if !newcomers.contains(&fact.id) {
                unit.anchor = Some(unit.anchor.map_or(index, |anchor| anchor.min(index)));
            }
            if layout.is_child(index) {
                unit.children.push(index);
            } else {
                unit.parents.push(index);
            }
        }

        let mut blocks = HashMap::<FolderId, (usize, Vec<usize>)>::new();
        for (folder, mut folder_units) in units {
            folder_units.sort_by_key(|(_, unit)| (unit.anchor.unwrap_or(unit.first), unit.first));
            let anchor = folder_units
                .iter()
                .filter_map(|(_, unit)| unit.anchor)
                .min()
                .or_else(|| folder_units.iter().map(|(_, unit)| unit.first).min())
                .unwrap_or(0);
            let block = folder_units
                .into_iter()
                .flat_map(|(_, unit)| unit.parents.into_iter().chain(unit.children))
                .collect();
            blocks.insert(folder, (anchor, block));
        }

        let mut desired = Vec::with_capacity(flat.len());
        for (index, fact) in flat.iter().enumerate() {
            match self.folder_of(&fact.id) {
                None => desired.push(index),
                Some(folder) => {
                    if let Some((anchor, block)) = blocks.get(&folder) {
                        if *anchor == index {
                            desired.extend(block.iter().copied());
                        }
                    }
                }
            }
        }

        let mut loose_groups = HashMap::<&str, (Vec<usize>, Vec<usize>)>::new();
        for &index in &desired {
            if self.folder_of(&flat[index].id).is_some() {
                continue;
            }
            if let Some(key) = layout.group_key(index) {
                let group = loose_groups.entry(key).or_default();
                if layout.is_child(index) {
                    group.1.push(index);
                } else {
                    group.0.push(index);
                }
            }
        }
        let mut order = Vec::with_capacity(desired.len());
        let mut emitted_folders = HashSet::new();
        let mut emitted_groups = HashSet::new();
        for &index in &desired {
            let fact = &flat[index];
            if let Some(folder) = self.folder_of(&fact.id) {
                if emitted_folders.insert(folder) {
                    order.push(Item::Folder(folder));
                }
                continue;
            }
            match layout.group_key(index) {
                Some(key) => {
                    if emitted_groups.insert(key) {
                        if let Some((parents, children)) = loose_groups.get(key) {
                            order.extend(
                                parents
                                    .iter()
                                    .chain(children)
                                    .map(|member| Item::Workspace(flat[*member].id.clone())),
                            );
                        }
                    }
                }
                None => order.push(Item::Workspace(fact.id.clone())),
            }
        }

        let existing = self
            .folders
            .iter()
            .map(|folder| folder.id)
            .collect::<HashSet<_>>();
        for (previous_index, item) in previous_order.iter().enumerate() {
            let Item::Folder(folder) = item else {
                continue;
            };
            if !existing.contains(folder) || order.contains(item) {
                continue;
            }
            let at = previous_order[..previous_index]
                .iter()
                .rev()
                .find_map(|predecessor| order.iter().position(|current| current == predecessor))
                .map_or(0, |position| item_unit_range(&order, &layout, position).1);
            order.insert(at, item.clone());
        }
        for folder in &self.folders {
            if !order.contains(&Item::Folder(folder.id)) {
                order.push(Item::Folder(folder.id));
            }
        }

        for folder in &mut self.folders {
            folder.members = blocks
                .get(&folder.id)
                .map(|(_, block)| block.iter().map(|index| flat[*index].id.clone()).collect())
                .unwrap_or_default();
        }
        self.order = order;
        desired
            .into_iter()
            .map(|index| flat[index].id.clone())
            .collect()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum UnitKey<'a> {
    Group(&'a str),
    Single(&'a str),
}

/// Worktree grouping over a flat order, following the sidebar's rule: a
/// repository key forms a group when it has a parent and a linked worktree.
struct Layout<'a> {
    flat: &'a [WorkspaceFact],
    grouped: HashSet<&'a str>,
    index: HashMap<&'a str, usize>,
}

impl<'a> Layout<'a> {
    fn new(flat: &'a [WorkspaceFact]) -> Self {
        let mut parents = HashSet::new();
        let mut children = HashSet::new();
        for fact in flat {
            if let Some(worktree) = &fact.worktree {
                if worktree.linked {
                    children.insert(worktree.key.as_str());
                } else {
                    parents.insert(worktree.key.as_str());
                }
            }
        }
        Self {
            flat,
            grouped: parents.intersection(&children).copied().collect(),
            index: flat
                .iter()
                .enumerate()
                .map(|(index, fact)| (fact.id.as_str(), index))
                .collect(),
        }
    }

    fn contains(&self, id: &str) -> bool {
        self.index.contains_key(id)
    }

    fn group_key(&self, index: usize) -> Option<&'a str> {
        self.flat[index]
            .worktree
            .as_ref()
            .map(|worktree| worktree.key.as_str())
            .filter(|key| self.grouped.contains(key))
    }

    fn is_child(&self, index: usize) -> bool {
        self.group_key(index).is_some()
            && self.flat[index]
                .worktree
                .as_ref()
                .is_some_and(|worktree| worktree.linked)
    }

    fn unit_key(&self, index: usize) -> UnitKey<'a> {
        match self.group_key(index) {
            Some(key) => UnitKey::Group(key),
            None => UnitKey::Single(self.flat[index].id.as_str()),
        }
    }

    fn unit_key_of(&self, id: &str) -> Option<UnitKey<'a>> {
        self.index.get(id).map(|index| self.unit_key(*index))
    }

    /// The workspace's whole unit in display order: parents, then children.
    fn unit_members(&self, id: &str) -> Vec<String> {
        let Some(&index) = self.index.get(id) else {
            return Vec::new();
        };
        let Some(key) = self.group_key(index) else {
            return vec![id.to_owned()];
        };
        let members = (0..self.flat.len())
            .filter(|candidate| self.group_key(*candidate) == Some(key))
            .collect::<Vec<_>>();
        members
            .iter()
            .filter(|member| !self.is_child(**member))
            .chain(members.iter().filter(|member| self.is_child(**member)))
            .map(|member| self.flat[*member].id.clone())
            .collect()
    }
}

/// The run of `order` items around `index` that belong to the same loose
/// worktree group, as a half-open range.
fn item_unit_range(order: &[Item], layout: &Layout<'_>, index: usize) -> (usize, usize) {
    let key = match &order[index] {
        Item::Workspace(id) => layout.unit_key_of(id),
        Item::Folder(_) => None,
    };
    let same = |item: &Item| match (item, key) {
        (Item::Workspace(id), Some(key)) => {
            matches!(key, UnitKey::Group(_)) && layout.unit_key_of(id) == Some(key)
        }
        _ => false,
    };
    let mut start = index;
    while start > 0 && same(&order[start - 1]) {
        start -= 1;
    }
    let mut end = index + 1;
    while end < order.len() && same(&order[end]) {
        end += 1;
    }
    (start, end)
}

/// The run of `members` around `id` that belongs to the same worktree group.
fn unit_range(members: &[String], layout: &Layout<'_>, id: &str) -> (usize, usize) {
    let Some(index) = members.iter().position(|member| member == id) else {
        return (members.len(), members.len());
    };
    let key = layout.unit_key_of(id);
    let same = |member: &String| {
        key.is_some_and(|key| {
            matches!(key, UnitKey::Group(_)) && layout.unit_key_of(member) == Some(key)
        })
    };
    let mut start = index;
    while start > 0 && same(&members[start - 1]) {
        start -= 1;
    }
    let mut end = index + 1;
    while end < members.len() && same(&members[end]) {
        end += 1;
    }
    (start, end)
}

fn without_workspace_items(
    order: &[Item],
    moved: &HashSet<String>,
    slot: usize,
) -> (Vec<Item>, usize) {
    let mut edited = Vec::with_capacity(order.len());
    let mut adjusted = slot;
    for (index, item) in order.iter().enumerate() {
        if matches!(item, Item::Workspace(id) if moved.contains(id)) {
            if index < slot {
                adjusted -= 1;
            }
            continue;
        }
        edited.push(item.clone());
    }
    let adjusted = adjusted.min(edited.len());
    (edited, adjusted)
}

fn flat_without(flat: &[WorkspaceFact], moved: &HashSet<String>) -> Vec<String> {
    flat.iter()
        .filter(|fact| !moved.contains(&fact.id))
        .map(|fact| fact.id.clone())
        .collect()
}

fn flat_positions(flat: &[String]) -> HashMap<&str, usize> {
    flat.iter()
        .enumerate()
        .map(|(index, id)| (id.as_str(), index))
        .collect()
}

fn splice(mut flat: Vec<String>, at: usize, block: &[String]) -> Vec<String> {
    let at = at.min(flat.len());
    flat.splice(at..at, block.iter().cloned());
    flat
}

fn reorder(facts: &[WorkspaceFact], ids: &[String]) -> Vec<WorkspaceFact> {
    let by_id = facts
        .iter()
        .map(|fact| (fact.id.as_str(), fact))
        .collect::<HashMap<_, _>>();
    ids.iter()
        .filter_map(|id| by_id.get(id.as_str()).map(|fact| (*fact).clone()))
        .collect()
}

#[cfg(test)]
#[path = "model_tests.rs"]
mod tests;
