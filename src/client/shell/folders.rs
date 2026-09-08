//! Sidebar folders: projection, hit testing, drag-and-drop, menus, and rendering.
//!
//! Folder membership and the space order are server facts from the snapshot;
//! collapse state is client presentation state kept in the chrome preferences.

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    sync::LazyLock,
};

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Widget},
};

use super::*;
use crate::protocol::{ClientShellFolder, ClientShellSpaceOrderEntry};

pub(super) const MENU_ITEM_MOVE_TO_FOLDER: &str = "Move to folder \u{25b8}";
pub(super) const MENU_ITEM_REMOVE_FROM_FOLDER: &str = "Remove from folder";
pub(super) const MENU_ITEM_NEW_FOLDER: &str = "New folder...";

pub(super) const FOLDER_MEMBER_INDENT: u16 = 2;
pub(super) const FOLDER_HEADER_ROWS: u16 = 1;
pub(super) const IN_FOLDER_DROP_INDENT: u16 = 3;

// ---------------------------------------------------------------------------
// Snapshot projection
// ---------------------------------------------------------------------------

pub(super) fn has_folders(snapshot: &ClientShellSnapshot) -> bool {
    !snapshot.folders.is_empty()
}

pub(super) fn folder_by_id<'a>(
    snapshot: &'a ClientShellSnapshot,
    folder_id: &str,
) -> Option<&'a ClientShellFolder> {
    snapshot
        .folders
        .iter()
        .find(|folder| folder.folder_id == folder_id)
}

pub(super) fn folder_of_workspace<'a>(
    snapshot: &'a ClientShellSnapshot,
    workspace_id: &str,
) -> Option<&'a ClientShellFolder> {
    snapshot
        .workspaces
        .iter()
        .find(|workspace| workspace.workspace_id == workspace_id)
        .and_then(|workspace| workspace.folder_id.as_deref())
        .and_then(|folder_id| folder_by_id(snapshot, folder_id))
}

/// The collapsed folder hiding `workspace_id`, if any.
pub(super) fn collapsed_folder_containing<'a>(
    snapshot: &'a ClientShellSnapshot,
    collapsed_folders: &HashSet<String>,
    workspace_id: &str,
) -> Option<&'a ClientShellFolder> {
    folder_of_workspace(snapshot, workspace_id)
        .filter(|folder| collapsed_folders.contains(&folder.folder_id))
}

/// "Move to folder ▸" targets as `(folder_id, name)`, excluding the current folder.
pub(super) fn folder_move_targets(
    snapshot: &ClientShellSnapshot,
    workspace_id: &str,
) -> Vec<(String, String)> {
    let current =
        folder_of_workspace(snapshot, workspace_id).map(|folder| folder.folder_id.as_str());
    snapshot
        .space_order
        .iter()
        .filter_map(|entry| match entry {
            ClientShellSpaceOrderEntry::Folder(folder_id)
                if Some(folder_id.as_str()) != current =>
            {
                folder_by_id(snapshot, folder_id)
                    .map(|folder| (folder.folder_id.clone(), folder.name.clone()))
            }
            _ => None,
        })
        .collect()
}

#[derive(Clone, Debug)]
pub(super) enum SidebarEntry {
    Folder {
        folder_index: usize,
    },
    Workspace {
        entry: WorkspaceEntry,
        foldered: bool,
    },
}

impl SidebarEntry {
    pub(super) fn is_indented_workspace(&self) -> bool {
        matches!(
            self,
            SidebarEntry::Workspace {
                entry: WorkspaceEntry { indented: true, .. },
                ..
            }
        )
    }
}

/// The spaces panel rows: folder members nested under their header, worktree
/// families hoisted at their parent, collapsed folders hiding their members.
pub(super) fn sidebar_entries(
    snapshot: &ClientShellSnapshot,
    collapsed_groups: &HashSet<String>,
    collapsed_folders: &HashSet<String>,
    force_expanded: bool,
) -> Vec<SidebarEntry> {
    if !has_folders(snapshot) {
        return render::workspace_entries(snapshot, collapsed_groups)
            .into_iter()
            .map(|entry| SidebarEntry::Workspace {
                entry,
                foldered: false,
            })
            .collect();
    }

    let mut members = HashMap::<&str, Vec<usize>>::new();
    for (index, workspace) in snapshot.workspaces.iter().enumerate() {
        if let Some(worktree) = &workspace.worktree {
            members.entry(&worktree.key).or_default().push(index);
        }
    }
    let is_parent = |index: usize| {
        snapshot.workspaces[index]
            .worktree
            .as_ref()
            .is_some_and(|worktree| !worktree.is_linked_worktree)
    };
    let grouped = members
        .iter()
        .filter(|(_, indices)| indices.len() >= 2 && indices.iter().any(|index| is_parent(*index)))
        .map(|(key, _)| *key)
        .collect::<HashSet<_>>();
    let index_by_id = snapshot
        .workspaces
        .iter()
        .enumerate()
        .map(|(index, workspace)| (workspace.workspace_id.as_str(), index))
        .collect::<HashMap<_, _>>();

    let mut listed = vec![false; snapshot.workspaces.len()];
    let mut emitted_groups = HashSet::<&str>::new();
    let mut entries = Vec::new();

    let mut emit_workspace = |index: usize, foldered: bool, entries: &mut Vec<SidebarEntry>| {
        let Some(workspace) = snapshot.workspaces.get(index) else {
            return;
        };
        let Some(worktree) = workspace
            .worktree
            .as_ref()
            .filter(|worktree| grouped.contains(worktree.key.as_str()))
        else {
            entries.push(SidebarEntry::Workspace {
                entry: WorkspaceEntry {
                    index,
                    indented: false,
                    last_child: false,
                },
                foldered,
            });
            return;
        };
        if !emitted_groups.insert(worktree.key.as_str()) {
            return;
        }
        let Some(group) = members.get(worktree.key.as_str()) else {
            return;
        };
        let parent = group
            .iter()
            .copied()
            .find(|member| is_parent(*member))
            .unwrap_or(index);
        entries.push(SidebarEntry::Workspace {
            entry: WorkspaceEntry {
                index: parent,
                indented: false,
                last_child: false,
            },
            foldered,
        });
        if !force_expanded && collapsed_groups.contains(&worktree.key) {
            if let Some(active) = group
                .iter()
                .copied()
                .find(|member| *member != parent && snapshot.workspaces[*member].focused)
            {
                entries.push(SidebarEntry::Workspace {
                    entry: WorkspaceEntry {
                        index: active,
                        indented: true,
                        last_child: true,
                    },
                    foldered,
                });
            }
            return;
        }
        let children = group
            .iter()
            .copied()
            .filter(|member| *member != parent)
            .collect::<Vec<_>>();
        for (child_index, child) in children.iter().enumerate() {
            entries.push(SidebarEntry::Workspace {
                entry: WorkspaceEntry {
                    index: *child,
                    indented: true,
                    last_child: child_index + 1 == children.len(),
                },
                foldered,
            });
        }
    };

    for order_entry in &snapshot.space_order {
        match order_entry {
            ClientShellSpaceOrderEntry::Workspace(id) => {
                let Some(&index) = index_by_id.get(id.as_str()) else {
                    continue;
                };
                if std::mem::replace(&mut listed[index], true) {
                    continue;
                }
                emit_workspace(index, false, &mut entries);
            }
            ClientShellSpaceOrderEntry::Folder(folder_id) => {
                let Some(folder_index) = snapshot
                    .folders
                    .iter()
                    .position(|folder| folder.folder_id == *folder_id)
                else {
                    continue;
                };
                entries.push(SidebarEntry::Folder { folder_index });
                let collapsed = !force_expanded && collapsed_folders.contains(folder_id);
                for member in &snapshot.folders[folder_index].members {
                    let Some(&index) = index_by_id.get(member.as_str()) else {
                        continue;
                    };
                    if std::mem::replace(&mut listed[index], true) {
                        continue;
                    }
                    if collapsed {
                        continue;
                    }
                    emit_workspace(index, true, &mut entries);
                }
            }
            ClientShellSpaceOrderEntry::Unknown => {}
        }
    }
    for (index, was_listed) in listed.iter().enumerate() {
        if !was_listed {
            emit_workspace(index, false, &mut entries);
        }
    }
    entries
}

pub(super) fn visible_workspace_entries(
    snapshot: &ClientShellSnapshot,
    collapsed_groups: &HashSet<String>,
    collapsed_folders: &HashSet<String>,
) -> Vec<WorkspaceEntry> {
    sidebar_entries(snapshot, collapsed_groups, collapsed_folders, false)
        .into_iter()
        .filter_map(|entry| match entry {
            SidebarEntry::Workspace { entry, .. } => Some(entry),
            SidebarEntry::Folder { .. } => None,
        })
        .collect()
}

/// Rank of each workspace id in the fully expanded spaces panel order.
pub(super) fn expanded_workspace_ranks(snapshot: &ClientShellSnapshot) -> HashMap<&str, usize> {
    sidebar_entries(snapshot, &HashSet::new(), &HashSet::new(), true)
        .into_iter()
        .filter_map(|entry| match entry {
            SidebarEntry::Workspace { entry, .. } => Some(entry.index),
            SidebarEntry::Folder { .. } => None,
        })
        .enumerate()
        .filter_map(|(rank, index)| {
            snapshot
                .workspaces
                .get(index)
                .map(|workspace| (workspace.workspace_id.as_str(), rank))
        })
        .collect()
}

/// Number of visible spaces above the collapsed folder hiding `workspace_id`.
fn folder_hidden_gap_position(
    snapshot: &ClientShellSnapshot,
    collapsed_groups: &HashSet<String>,
    collapsed_folders: &HashSet<String>,
    workspace_id: &str,
) -> Option<usize> {
    let folder = collapsed_folder_containing(snapshot, collapsed_folders, workspace_id)?;
    let mut visible_before = 0usize;
    for entry in sidebar_entries(snapshot, collapsed_groups, collapsed_folders, false) {
        match entry {
            SidebarEntry::Folder { folder_index }
                if snapshot.folders[folder_index].folder_id == folder.folder_id =>
            {
                return Some(visible_before);
            }
            SidebarEntry::Folder { .. } => {}
            SidebarEntry::Workspace { .. } => visible_before += 1,
        }
    }
    None
}

/// Highlight a collapsed folder header carries on behalf of its hidden members.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) struct FolderHeaderHighlight {
    pub selected: bool,
    pub active: bool,
}

pub(super) fn collapsed_folder_header_highlight(
    snapshot: &ClientShellSnapshot,
    folder: &ClientShellFolder,
    collapsed_folders: &HashSet<String>,
    selected_workspace_id: Option<&str>,
) -> FolderHeaderHighlight {
    if !collapsed_folders.contains(&folder.folder_id) {
        return FolderHeaderHighlight::default();
    }
    let contains = |workspace_id: &str| folder.members.iter().any(|member| member == workspace_id);
    FolderHeaderHighlight {
        selected: selected_workspace_id.is_some_and(contains),
        active: snapshot
            .focused_workspace_id
            .as_deref()
            .is_some_and(contains),
    }
}

// ---------------------------------------------------------------------------
// Render-time state and hits
// ---------------------------------------------------------------------------

pub(super) struct FolderRenderState<'a> {
    /// The endpoint these collapse sets belong to.
    pub(super) endpoint_id: &'a ClientEndpointId,
    pub(super) collapsed_folders: &'a HashSet<String>,
    pub(super) collapsed_agent_spaces: &'a HashSet<String>,
    pub(super) collapse_by_endpoint: &'a HashMap<String, FolderCollapseState>,
    pub(super) dragged_folder_id: Option<&'a str>,
    pub(super) drop_into_folder_id: Option<&'a str>,
    pub(super) drop_indicator_indent: u16,
}

/// One endpoint's collapse state. Folder and workspace ids are server-scoped,
/// so each endpoint keeps its own.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct FolderCollapseState {
    /// Collapsed folders, shared by the spaces and agents panels.
    pub(super) folders: HashSet<String>,
    /// Spaces whose agent list is collapsed in the agents panel folder view.
    pub(super) agent_spaces: HashSet<String>,
}

impl FolderCollapseState {
    fn is_empty(&self) -> bool {
        self.folders.is_empty() && self.agent_spaces.is_empty()
    }

    /// Load from preferences; the legacy flat fields fold into Local's entry.
    pub(super) fn from_preferences(
        per_endpoint: BTreeMap<String, preferences::FolderCollapsePreferences>,
        legacy_folders: Vec<String>,
        legacy_agent_spaces: Vec<String>,
    ) -> HashMap<String, Self> {
        let mut map: HashMap<String, Self> = per_endpoint
            .into_iter()
            .map(|(key, stored)| {
                (
                    key,
                    Self {
                        folders: stored.collapsed_folders.into_iter().collect(),
                        agent_spaces: stored.collapsed_agent_spaces.into_iter().collect(),
                    },
                )
            })
            .collect();
        if !legacy_folders.is_empty() || !legacy_agent_spaces.is_empty() {
            let local = map
                .entry(ClientEndpointId::Local.storage_key())
                .or_default();
            local.folders.extend(legacy_folders);
            local.agent_spaces.extend(legacy_agent_spaces);
        }
        map.retain(|_, state| !state.is_empty());
        map
    }

    pub(super) fn to_preferences(
        map: &HashMap<String, Self>,
    ) -> BTreeMap<String, preferences::FolderCollapsePreferences> {
        map.iter()
            .filter(|(_, state)| !state.is_empty())
            .map(|(key, state)| {
                let mut collapsed_folders = state.folders.iter().cloned().collect::<Vec<_>>();
                collapsed_folders.sort();
                let mut collapsed_agent_spaces =
                    state.agent_spaces.iter().cloned().collect::<Vec<_>>();
                collapsed_agent_spaces.sort();
                (
                    key.clone(),
                    preferences::FolderCollapsePreferences {
                        collapsed_folders,
                        collapsed_agent_spaces,
                    },
                )
            })
            .collect()
    }
}

impl FolderCollapseState {
    /// One endpoint's entry, or an empty default. Takes the map directly so
    /// render code holding other `ClientShellState` borrows can use it.
    pub(super) fn of<'a>(
        map: &'a HashMap<String, Self>,
        endpoint_id: &ClientEndpointId,
    ) -> &'a Self {
        static EMPTY: LazyLock<FolderCollapseState> = LazyLock::new(FolderCollapseState::default);
        map.get(&endpoint_id.storage_key()).unwrap_or(&EMPTY)
    }
}

impl ClientShellState {
    pub(super) fn folder_collapse(&self) -> &FolderCollapseState {
        FolderCollapseState::of(&self.folder_collapse, &self.active_endpoint_id)
    }

    pub(super) fn folder_collapse_mut(&mut self) -> &mut FolderCollapseState {
        self.folder_collapse
            .entry(self.active_endpoint_id.storage_key())
            .or_default()
    }

    pub(super) fn collapsed_folders(&self) -> &HashSet<String> {
        &self.folder_collapse().folders
    }
}

pub(super) struct FolderHit {
    pub(super) rect: Rect,
    pub(super) endpoint_id: ClientEndpointId,
    pub(super) folder_id: String,
}

#[derive(Default)]
pub(super) struct FolderHits {
    pub(super) headers: Vec<FolderHit>,
    pub(super) agent_folder_headers: Vec<FolderHit>,
    /// Agents panel space headers as `(rect, workspace_id)`.
    pub(super) agent_space_headers: Vec<(Rect, String)>,
}

pub(super) struct ClientFolderPress {
    pub(super) folder_id: String,
    pub(super) start_column: u16,
    pub(super) start_row: u16,
}

// ---------------------------------------------------------------------------
// Drag and drop
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum SpaceDragSource {
    Workspace(String),
    Folder(String),
}

/// Where a space-order drag would land.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum SpaceDropTarget {
    Before(String),
    BeforeFolder(String),
    InFolderBefore {
        folder_id: String,
        workspace_id: String,
    },
    InFolderEnd {
        folder_id: String,
    },
    /// Dropped onto the folder's header row.
    IntoFolder(String),
    End,
}

impl SpaceDropTarget {
    pub(super) fn is_top_level(&self) -> bool {
        matches!(
            self,
            SpaceDropTarget::Before(_) | SpaceDropTarget::BeforeFolder(_) | SpaceDropTarget::End
        )
    }

    pub(super) fn indent(&self) -> u16 {
        match self {
            SpaceDropTarget::InFolderBefore { .. } | SpaceDropTarget::InFolderEnd { .. } => {
                IN_FOLDER_DROP_INDENT
            }
            _ => 0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct SpaceDropSlot {
    pub(super) target: SpaceDropTarget,
    /// Indicator row; `None` for header drops, which highlight the header instead.
    pub(super) row: Option<u16>,
}

impl ClientShellState {
    pub(super) fn folder_drag_active(&self) -> bool {
        self.snapshot.as_deref().is_some_and(has_folders)
    }

    /// Drop slot nearest to `point`. Folder drags only see top-level slots.
    pub(super) fn space_drop_slot_at(
        &self,
        point: (u16, u16),
        source: &SpaceDragSource,
    ) -> Option<SpaceDropSlot> {
        if self.hits.workspace_body.height == 0
            || point.1 < self.hits.workspace_body.y.saturating_sub(1)
            || point.1
                >= self
                    .hits
                    .new_workspace
                    .y
                    .max(self.hits.workspace_body.bottom())
            || self.hits.workspaces.iter().any(|hit| {
                hit.endpoint_id != self.active_endpoint_id && super::contains(hit.rect, point)
            })
            || self.hits.folders.headers.iter().any(|header| {
                header.endpoint_id != self.active_endpoint_id && super::contains(header.rect, point)
            })
        {
            return None;
        }
        let snapshot = self.snapshot.as_deref()?;
        let limit = self
            .hits
            .new_workspace
            .y
            .max(self.hits.workspace_body.bottom());
        let own_headers = || {
            self.hits
                .folders
                .headers
                .iter()
                .filter(|header| header.endpoint_id == self.active_endpoint_id)
        };
        let own_workspaces = || {
            self.hits
                .workspaces
                .iter()
                .filter(|hit| hit.endpoint_id == self.active_endpoint_id)
        };

        if matches!(source, SpaceDragSource::Workspace(_)) {
            if let Some(header) = own_headers().find(|header| super::contains(header.rect, point)) {
                return Some(SpaceDropSlot {
                    target: SpaceDropTarget::IntoFolder(header.folder_id.clone()),
                    row: None,
                });
            }
        }

        #[derive(Clone)]
        enum Row<'a> {
            Header(&'a FolderHit),
            Card(&'a WorkspaceHit),
        }
        let mut rows = own_headers()
            .map(|header| (header.rect.y, Row::Header(header)))
            .chain(
                own_workspaces()
                    .filter(|hit| !hit.indented)
                    .map(|hit| (hit.rect.y, Row::Card(hit))),
            )
            .collect::<Vec<_>>();
        rows.sort_by_key(|(y, _)| *y);

        let block_bottom = |hit: &WorkspaceHit| {
            let mut bottom = hit.rect.bottom();
            let mut seen = false;
            for candidate in own_workspaces() {
                if candidate.workspace_id == hit.workspace_id {
                    seen = true;
                    continue;
                }
                if seen {
                    if candidate.indented {
                        bottom = candidate.rect.bottom();
                    } else {
                        break;
                    }
                }
            }
            for header in own_headers() {
                if header.rect.y > hit.rect.y && header.rect.y < bottom {
                    bottom = header.rect.y;
                }
            }
            bottom
        };

        // Natural rows may coincide or run backwards; `settle_slot_rows` spreads them.
        let mut slots: Vec<(SpaceDropTarget, u16)> = Vec::new();
        let folder_drag = matches!(source, SpaceDragSource::Folder(_));

        for (position, (_, row)) in rows.iter().enumerate() {
            match row {
                Row::Header(header) => {
                    slots.push((
                        SpaceDropTarget::BeforeFolder(header.folder_id.clone()),
                        header.rect.y.saturating_sub(1),
                    ));
                }
                Row::Card(hit) => {
                    if hit.foldered {
                        if folder_drag {
                            continue;
                        }
                        let Some(folder) = folder_of_workspace(snapshot, &hit.workspace_id) else {
                            continue;
                        };
                        let previous = position
                            .checked_sub(1)
                            .and_then(|previous| rows.get(previous))
                            .map(|(_, row)| row);
                        // The header row itself files into the folder, so the
                        // first member's slot sits on its own top row.
                        let first_member = matches!(previous, Some(Row::Header(_)));
                        let row_y = if first_member {
                            hit.rect.y
                        } else {
                            hit.rect.y.saturating_sub(1)
                        };
                        slots.push((
                            SpaceDropTarget::InFolderBefore {
                                folder_id: folder.folder_id.clone(),
                                workspace_id: hit.workspace_id.clone(),
                            },
                            row_y,
                        ));
                        // The row below the last member appends into the folder
                        // unless another header follows; that gap stays top-level.
                        let next = rows.get(position + 1).map(|(_, row)| row);
                        let bottom = block_bottom(hit);
                        match next {
                            None => slots.push((
                                SpaceDropTarget::InFolderEnd {
                                    folder_id: folder.folder_id.clone(),
                                },
                                bottom,
                            )),
                            Some(Row::Card(next_hit)) if !next_hit.foldered => slots.push((
                                SpaceDropTarget::InFolderEnd {
                                    folder_id: folder.folder_id.clone(),
                                },
                                bottom.min(next_hit.rect.y.saturating_sub(1)),
                            )),
                            Some(_) => {}
                        }
                    } else {
                        slots.push((
                            SpaceDropTarget::Before(hit.workspace_id.clone()),
                            hit.rect.y.saturating_sub(1),
                        ));
                    }
                }
            }
        }

        if let Some((_, last)) = rows.last() {
            let bottom = match last {
                Row::Header(header) => header.rect.bottom(),
                Row::Card(hit) => block_bottom(hit),
            };
            slots.push((SpaceDropTarget::End, bottom));
        }

        let slots = settle_slot_rows(slots, limit);
        slots
            .into_iter()
            .enumerate()
            .filter(|(_, slot)| !folder_drag || slot.target.is_top_level())
            .min_by_key(|(index, slot)| (point.1.abs_diff(slot.row.unwrap_or(u16::MAX)), *index))
            .map(|(_, slot)| slot)
    }

    /// The API method for a drop, or `None` for no-ops and illegal drops.
    pub(super) fn space_drop_method(
        &self,
        source: &SpaceDragSource,
        target: &SpaceDropTarget,
    ) -> Option<crate::api::schema::Method> {
        use crate::api::schema::{FolderAssignParams, FolderMoveParams, Method};

        let snapshot = self.snapshot.as_deref()?;
        match source {
            SpaceDragSource::Folder(folder_id) => {
                if !target.is_top_level() {
                    return None;
                }
                if matches!(target, SpaceDropTarget::BeforeFolder(id) if id == folder_id) {
                    return None;
                }
                let remaining = snapshot
                    .space_order
                    .iter()
                    .filter(|entry| !matches!(entry, ClientShellSpaceOrderEntry::Folder(id) if id == folder_id))
                    .collect::<Vec<_>>();
                let current = top_level_position(
                    &snapshot.space_order,
                    |entry| matches!(entry, ClientShellSpaceOrderEntry::Folder(id) if id == folder_id),
                )?;
                // A `Before*` target gone from the snapshot mid-drag is a no-op.
                let position = match target {
                    SpaceDropTarget::End => remaining.len(),
                    _ => top_level_target_position(&remaining, target)?,
                };
                let effective_current = current;
                if position == effective_current {
                    return None;
                }
                Some(Method::FolderMove(FolderMoveParams {
                    folder_id: folder_id.clone(),
                    position,
                }))
            }
            SpaceDragSource::Workspace(workspace_id) => {
                let workspace = snapshot
                    .workspaces
                    .iter()
                    .find(|workspace| workspace.workspace_id == *workspace_id)?;
                if workspace
                    .worktree
                    .as_ref()
                    .is_some_and(|worktree| worktree.is_linked_worktree)
                {
                    return None;
                }
                let family = family_member_ids(snapshot, workspace);
                let affected = |id: &str| family.iter().any(|member| member == id);
                let current_folder = folder_of_workspace(snapshot, workspace_id)
                    .map(|folder| folder.folder_id.clone());
                match target {
                    SpaceDropTarget::IntoFolder(folder_id)
                    | SpaceDropTarget::InFolderEnd { folder_id } => {
                        let folder = folder_by_id(snapshot, folder_id)?;
                        if current_folder.as_deref() == Some(folder_id.as_str()) {
                            let last_block_is_source = folder
                                .members
                                .iter()
                                .rev()
                                .take_while(|member| affected(member))
                                .count()
                                == family.len()
                                && folder.members.len() >= family.len();
                            if last_block_is_source {
                                return None;
                            }
                        }
                        Some(Method::FolderAssign(FolderAssignParams {
                            workspace_id: workspace_id.clone(),
                            folder_id: Some(folder_id.clone()),
                            position: None,
                        }))
                    }
                    SpaceDropTarget::InFolderBefore {
                        folder_id,
                        workspace_id: before,
                    } => {
                        if affected(before) {
                            return None;
                        }
                        let folder = folder_by_id(snapshot, folder_id)?;
                        let remaining = folder
                            .members
                            .iter()
                            .filter(|member| !affected(member))
                            .collect::<Vec<_>>();
                        let position = remaining.iter().position(|member| *member == before)?;
                        if current_folder.as_deref() == Some(folder_id.as_str()) {
                            let current = folder
                                .members
                                .iter()
                                .filter(|member| !affected(member))
                                .take_while(|member| {
                                    folder.members.iter().position(|m| m == *member)
                                        < folder.members.iter().position(|m| m == workspace_id)
                                })
                                .count();
                            if current == position {
                                return None;
                            }
                        }
                        Some(Method::FolderAssign(FolderAssignParams {
                            workspace_id: workspace_id.clone(),
                            folder_id: Some(folder_id.clone()),
                            position: Some(position),
                        }))
                    }
                    SpaceDropTarget::Before(_)
                    | SpaceDropTarget::BeforeFolder(_)
                    | SpaceDropTarget::End => {
                        if matches!(target, SpaceDropTarget::Before(before) if affected(before)) {
                            return None;
                        }
                        let remaining = snapshot
                            .space_order
                            .iter()
                            .filter(|entry| {
                                !matches!(entry, ClientShellSpaceOrderEntry::Workspace(id) if affected(id))
                            })
                            .collect::<Vec<_>>();
                        let position = top_level_target_position(&remaining, target);
                        if current_folder.is_none() {
                            let current = top_level_position(
                                &snapshot.space_order,
                                |entry| matches!(entry, ClientShellSpaceOrderEntry::Workspace(id) if id == workspace_id),
                            );
                            let current_remaining = current.map(|current| {
                                snapshot.space_order[..current]
                                    .iter()
                                    .filter(|entry| {
                                        !matches!(entry, ClientShellSpaceOrderEntry::Workspace(id) if affected(id))
                                    })
                                    .count()
                            });
                            if current_remaining.is_some()
                                && current_remaining == position.or(Some(remaining.len()))
                            {
                                return None;
                            }
                        }
                        Some(Method::FolderAssign(FolderAssignParams {
                            workspace_id: workspace_id.clone(),
                            folder_id: None,
                            position,
                        }))
                    }
                }
            }
        }
    }
}

fn family_member_ids(
    snapshot: &ClientShellSnapshot,
    workspace: &ClientShellWorkspace,
) -> Vec<String> {
    match workspace.worktree.as_ref() {
        Some(worktree) => snapshot
            .workspaces
            .iter()
            .filter(|candidate| {
                candidate
                    .worktree
                    .as_ref()
                    .is_some_and(|candidate| candidate.key == worktree.key)
            })
            .map(|candidate| candidate.workspace_id.clone())
            .collect(),
        None => vec![workspace.workspace_id.clone()],
    }
}

fn top_level_position(
    order: &[ClientShellSpaceOrderEntry],
    matches_entry: impl Fn(&ClientShellSpaceOrderEntry) -> bool,
) -> Option<usize> {
    order.iter().position(matches_entry)
}

/// Spread natural slot rows into strictly increasing rows below `limit`.
///
/// Colliding slots shift down so each stays reachable. When that drift pushes
/// the trailing slot past `limit`, end-of-folder slots in the colliding run
/// are dropped last-first so the top-level `End` stays reachable. Slots whose
/// natural row is already past `limit` are simply dropped.
pub(super) fn settle_slot_rows(
    mut slots: Vec<(SpaceDropTarget, u16)>,
    limit: u16,
) -> Vec<SpaceDropSlot> {
    fn settled(slots: &[(SpaceDropTarget, u16)]) -> Vec<u16> {
        let mut rows = Vec::with_capacity(slots.len());
        let mut previous: Option<u16> = None;
        for (_, natural) in slots {
            let row = previous.map_or(*natural, |previous| {
                (*natural).max(previous.saturating_add(1))
            });
            rows.push(row);
            previous = Some(row);
        }
        rows
    }

    let mut rows = settled(&slots);
    while let Some((last, &(_, natural_last))) = rows.last().zip(slots.last()) {
        if *last < limit || natural_last >= limit {
            break;
        }
        // Only slots in the run pushed into the trailing one can lower it.
        let mut anchor = slots.len() - 1;
        while anchor > 0 && rows[anchor] > slots[anchor].1 {
            anchor -= 1;
        }
        let Some(index) = slots[anchor..]
            .iter()
            .rposition(|(target, _)| matches!(target, SpaceDropTarget::InFolderEnd { .. }))
            .map(|offset| anchor + offset)
        else {
            break;
        };
        slots.remove(index);
        rows = settled(&slots);
    }
    slots
        .into_iter()
        .zip(rows)
        .filter(|(_, row)| *row < limit)
        .map(|((target, _), row)| SpaceDropSlot {
            target,
            row: Some(row),
        })
        .collect()
}

/// Index of a top-level drop target among `remaining` entries; `None` appends.
fn top_level_target_position(
    remaining: &[&ClientShellSpaceOrderEntry],
    target: &SpaceDropTarget,
) -> Option<usize> {
    match target {
        SpaceDropTarget::Before(workspace_id) => remaining
            .iter()
            .position(|entry| matches!(entry, ClientShellSpaceOrderEntry::Workspace(id) if id == workspace_id)),
        SpaceDropTarget::BeforeFolder(folder_id) => remaining
            .iter()
            .position(|entry| matches!(entry, ClientShellSpaceOrderEntry::Folder(id) if id == folder_id)),
        SpaceDropTarget::End => None,
        SpaceDropTarget::InFolderBefore { .. }
        | SpaceDropTarget::InFolderEnd { .. }
        | SpaceDropTarget::IntoFolder(_) => None,
    }
}

// ---------------------------------------------------------------------------
// Navigation
// ---------------------------------------------------------------------------

impl ClientShellState {
    /// Position of `workspace_id` among the visible entries, or the collapsed
    /// folder's gap so one `delta` step lands on its visible neighbor.
    pub(super) fn navigation_anchor(
        &self,
        snapshot: &ClientShellSnapshot,
        entries: &[WorkspaceEntry],
        workspace_id: Option<&str>,
        delta: isize,
    ) -> isize {
        let Some(workspace_id) = workspace_id else {
            return 0;
        };
        if let Some(position) = entries
            .iter()
            .position(|entry| snapshot.workspaces[entry.index].workspace_id == workspace_id)
        {
            return position as isize;
        }
        if self.mobile_layout_active() {
            return 0;
        }
        let Some(gap) = folder_hidden_gap_position(
            snapshot,
            &self.collapsed_groups,
            self.collapsed_folders(),
            workspace_id,
        ) else {
            return 0;
        };
        if delta > 0 {
            gap as isize - 1
        } else {
            gap as isize
        }
    }

    pub(super) fn toggle_folder_collapse(
        &mut self,
        folder_id: &str,
        outcome: &mut ClientShellInput,
    ) {
        let endpoint_id = self.active_endpoint_id.clone();
        self.toggle_folder_collapse_for(&endpoint_id, folder_id, outcome);
    }

    pub(super) fn toggle_folder_collapse_for(
        &mut self,
        endpoint_id: &ClientEndpointId,
        folder_id: &str,
        outcome: &mut ClientShellInput,
    ) {
        let collapsed = &mut self
            .folder_collapse
            .entry(endpoint_id.storage_key())
            .or_default()
            .folders;
        if !collapsed.remove(folder_id) {
            collapsed.insert(folder_id.to_owned());
        }
        outcome.repaint = true;
        self.persist_chrome_preferences(outcome);
    }

    pub(super) fn toggle_agent_space_collapse(
        &mut self,
        workspace_id: &str,
        outcome: &mut ClientShellInput,
    ) {
        let collapsed = &mut self.folder_collapse_mut().agent_spaces;
        if !collapsed.remove(workspace_id) {
            collapsed.insert(workspace_id.to_owned());
        }
        outcome.repaint = true;
        self.persist_chrome_preferences(outcome);
    }

    /// Drop the active endpoint's collapse state for folders and spaces that
    /// no longer exist in its snapshot.
    pub(super) fn prune_folder_collapse_state(&mut self, outcome: &mut ClientShellInput) {
        let Some(snapshot) = self.snapshot.as_deref() else {
            return;
        };
        let key = self.active_endpoint_id.storage_key();
        let Some(state) = self.folder_collapse.get_mut(&key) else {
            return;
        };
        let before = state.folders.len() + state.agent_spaces.len();
        state
            .folders
            .retain(|folder_id| folder_by_id(snapshot, folder_id).is_some());
        state.agent_spaces.retain(|workspace_id| {
            snapshot
                .workspaces
                .iter()
                .any(|workspace| workspace.workspace_id == *workspace_id)
        });
        let changed = before != state.folders.len() + state.agent_spaces.len();
        if state.is_empty() {
            self.folder_collapse.remove(&key);
        }
        if changed {
            self.persist_chrome_preferences(outcome);
        }
    }
}

// ---------------------------------------------------------------------------
// Context menus and modals
// ---------------------------------------------------------------------------

impl ClientShellState {
    pub(super) fn folder_header_hit_at(&self, point: (u16, u16)) -> Option<&FolderHit> {
        self.hits
            .folders
            .headers
            .iter()
            .find(|header| super::contains(header.rect, point))
    }

    /// The active endpoint's folder header under `point`.
    pub(super) fn folder_header_at(&self, point: (u16, u16)) -> Option<String> {
        self.folder_header_hit_at(point)
            .filter(|header| header.endpoint_id == self.active_endpoint_id)
            .map(|header| header.folder_id.clone())
    }

    /// Whether `point` is on the spaces panel's title rows or list background.
    pub(super) fn spaces_panel_background_at(&self, point: (u16, u16)) -> bool {
        let body = self.hits.workspace_body;
        if body.is_empty() {
            return false;
        }
        let title_rows = WORKSPACE_HEADER_ROWS.min(body.y);
        let panel = Rect::new(
            body.x,
            body.y - title_rows,
            body.width,
            body.height.saturating_add(title_rows),
        );
        super::contains(panel, point)
            && !self
                .hits
                .workspaces
                .iter()
                .any(|hit| super::contains(hit.rect, point))
            && self.folder_header_at(point).is_none()
    }

    pub(super) fn open_folder_context_menu(&mut self, folder_id: String, x: u16, y: u16) {
        self.overlay = Some(ClientShellOverlay::ContextMenu(ClientContextMenuOverlay {
            target: ClientContextMenuTarget::Folder { folder_id },
            x,
            y,
            highlighted: 0,
        }));
    }

    pub(super) fn open_spaces_panel_context_menu(&mut self, x: u16, y: u16) {
        self.overlay = Some(ClientShellOverlay::ContextMenu(ClientContextMenuOverlay {
            target: ClientContextMenuTarget::SpacesPanel,
            x,
            y,
            highlighted: 0,
        }));
    }

    pub(super) fn open_move_to_folder_menu(&mut self, workspace_id: String, x: u16, y: u16) {
        let folders = self
            .snapshot
            .as_deref()
            .map(|snapshot| folder_move_targets(snapshot, &workspace_id))
            .unwrap_or_default();
        self.overlay = Some(ClientShellOverlay::ContextMenu(ClientContextMenuOverlay {
            target: ClientContextMenuTarget::MoveToFolder {
                workspace_id,
                folders,
            },
            x,
            y,
            highlighted: 0,
        }));
    }

    pub(super) fn open_rename_folder_overlay(&mut self, folder_id: String) {
        let Some(name) = self
            .snapshot
            .as_deref()
            .and_then(|snapshot| folder_by_id(snapshot, &folder_id))
            .map(|folder| folder.name.clone())
        else {
            return;
        };
        self.overlay = Some(ClientShellOverlay::Rename(ClientRenameOverlay {
            title: "rename folder",
            input: name,
            replace_on_type: false,
            target: ClientRenameTarget::Folder { folder_id },
        }));
    }

    pub(super) fn open_new_folder_overlay(&mut self, move_workspace_id: Option<String>) {
        self.overlay = Some(ClientShellOverlay::Rename(ClientRenameOverlay {
            title: "new folder",
            input: String::new(),
            replace_on_type: false,
            target: ClientRenameTarget::NewFolder { move_workspace_id },
        }));
    }

    /// Dispatch a folder menu action; returns `false` for other actions.
    pub(super) fn activate_folder_context_action(
        &mut self,
        target: &ClientContextMenuTarget,
        action: ClientContextMenuAction,
        menu_x: u16,
        menu_y: u16,
        outcome: &mut ClientShellInput,
    ) -> bool {
        use crate::api::schema::{FolderAssignParams, FolderTarget, Method};

        match (target, action) {
            (
                ClientContextMenuTarget::Workspace { workspace_id, .. },
                ClientContextMenuAction::MoveToFolderMenu,
            ) => {
                self.open_move_to_folder_menu(workspace_id.clone(), menu_x, menu_y);
            }
            (
                ClientContextMenuTarget::Workspace { workspace_id, .. },
                ClientContextMenuAction::RemoveFromFolder,
            ) => {
                self.push_endpoint_method(
                    Method::FolderAssign(FolderAssignParams {
                        workspace_id: workspace_id.clone(),
                        folder_id: None,
                        position: None,
                    }),
                    outcome,
                );
            }
            (ClientContextMenuTarget::Folder { folder_id }, ClientContextMenuAction::Rename) => {
                self.open_rename_folder_overlay(folder_id.clone());
            }
            (
                ClientContextMenuTarget::Folder { folder_id },
                ClientContextMenuAction::DeleteFolder,
            ) => {
                self.push_endpoint_method(
                    Method::FolderDelete(FolderTarget {
                        folder_id: folder_id.clone(),
                    }),
                    outcome,
                );
            }
            (
                ClientContextMenuTarget::MoveToFolder {
                    workspace_id,
                    folders,
                },
                ClientContextMenuAction::MoveToFolder(index),
            ) => {
                if let Some((folder_id, _)) = folders.get(index) {
                    self.push_endpoint_method(
                        Method::FolderAssign(FolderAssignParams {
                            workspace_id: workspace_id.clone(),
                            folder_id: Some(folder_id.clone()),
                            position: None,
                        }),
                        outcome,
                    );
                }
            }
            (
                ClientContextMenuTarget::MoveToFolder { workspace_id, .. },
                ClientContextMenuAction::NewFolder,
            ) => {
                self.open_new_folder_overlay(Some(workspace_id.clone()));
            }
            (ClientContextMenuTarget::SpacesPanel, ClientContextMenuAction::NewFolder) => {
                self.open_new_folder_overlay(None);
            }
            _ => return false,
        }
        true
    }

    /// Complete a `folder.create`, filing the requesting space into the new folder.
    pub(super) fn complete_folder_create(
        &mut self,
        move_workspace_id: Option<String>,
        result: &Result<crate::api::schema::ResponseResult, ClientShellEndpointError>,
    ) -> Vec<ClientShellAction> {
        use crate::api::schema::{FolderAssignParams, Method, ResponseResult};

        let (Some(workspace_id), Ok(ResponseResult::FolderCreated { folder_id })) =
            (move_workspace_id, result)
        else {
            return Vec::new();
        };
        let mut outcome = ClientShellInput::default();
        self.push_endpoint_method(
            Method::FolderAssign(FolderAssignParams {
                workspace_id,
                folder_id: Some(folder_id.clone()),
                position: None,
            }),
            &mut outcome,
        );
        outcome.actions
    }
}

/// Menu items for folder, submenu, and panel targets.
pub(super) fn folder_menu_items(
    target: &ClientContextMenuTarget,
) -> Option<Vec<ClientContextMenuItem>> {
    use std::borrow::Cow;
    use ClientContextMenuAction as Action;

    let item = |label: &'static str, action| ClientContextMenuItem {
        label: Cow::Borrowed(label),
        action,
    };
    match target {
        ClientContextMenuTarget::Folder { .. } => Some(vec![
            item("Rename", Action::Rename),
            item("Delete", Action::DeleteFolder),
        ]),
        ClientContextMenuTarget::MoveToFolder { folders, .. } => Some(
            folders
                .iter()
                .enumerate()
                .map(|(index, (_, name))| ClientContextMenuItem {
                    label: Cow::Owned(name.clone()),
                    action: Action::MoveToFolder(index),
                })
                .chain(std::iter::once(item(
                    MENU_ITEM_NEW_FOLDER,
                    Action::NewFolder,
                )))
                .collect(),
        ),
        ClientContextMenuTarget::SpacesPanel => {
            Some(vec![item(MENU_ITEM_NEW_FOLDER, Action::NewFolder)])
        }
        _ => None,
    }
}

/// Folder items spliced into a space's menu after "Rename".
pub(super) fn space_menu_folder_items(foldered: bool) -> Vec<ClientContextMenuItem> {
    use std::borrow::Cow;

    let mut items = vec![ClientContextMenuItem {
        label: Cow::Borrowed(MENU_ITEM_MOVE_TO_FOLDER),
        action: ClientContextMenuAction::MoveToFolderMenu,
    }];
    if foldered {
        items.push(ClientContextMenuItem {
            label: Cow::Borrowed(MENU_ITEM_REMOVE_FROM_FOLDER),
            action: ClientContextMenuAction::RemoveFromFolder,
        });
    }
    items
}

// ---------------------------------------------------------------------------
// Spaces panel rendering
// ---------------------------------------------------------------------------

pub(super) fn truncate_end(text: &str, max_width: usize) -> String {
    let mut width = 0usize;
    let mut out = String::new();
    for character in text.chars() {
        let w = unicode_width::UnicodeWidthChar::width(character).unwrap_or(0);
        if width + w > max_width {
            break;
        }
        width += w;
        out.push(character);
    }
    out
}

#[allow(clippy::too_many_arguments)] // render call sites pass explicit projection facts
pub(super) fn render_folder_header(
    buffer: &mut Buffer,
    rect: Rect,
    snapshot: &ClientShellSnapshot,
    folder: &ClientShellFolder,
    folders: &FolderRenderState<'_>,
    selected_workspace_id: Option<&str>,
    palette: &Palette,
    hits: &mut ShellHitMap,
) {
    if rect.width == 0 || rect.height == 0 {
        return;
    }
    let collapsed = folders.collapsed_folders.contains(&folder.folder_id);
    let highlight = collapsed_folder_header_highlight(
        snapshot,
        folder,
        folders.collapsed_folders,
        selected_workspace_id,
    );
    let dragged = folders.dragged_folder_id == Some(folder.folder_id.as_str());
    let drop_target = folders.drop_into_folder_id == Some(folder.folder_id.as_str());
    if highlight.selected {
        buffer.set_style(rect, Style::default().bg(palette.selection_bg));
    } else if dragged {
        buffer.set_style(rect, Style::default().bg(palette.surface1));
    } else if highlight.active {
        buffer.set_style(rect, Style::default().bg(palette.active_row_bg));
    }
    let name_style = if drop_target {
        Style::default()
            .fg(palette.accent)
            .add_modifier(Modifier::BOLD)
    } else if highlight.selected || highlight.active || dragged {
        Style::default()
            .fg(palette.text)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(palette.subtext0)
            .add_modifier(Modifier::BOLD)
    };
    // The chevron aligns with loose cards' state icons, the name with their names.
    let name = truncate_end(&folder.name, rect.width.saturating_sub(3) as usize);
    Paragraph::new(Line::from(vec![
        Span::raw("   "),
        Span::styled(name, name_style),
    ]))
    .render(Rect::new(rect.x, rect.y, rect.width, 1), buffer);
    if rect.width >= 2 {
        render::put_text(
            buffer,
            rect.x + 1,
            rect.y,
            1,
            if collapsed { "▸" } else { "▾" },
            Style::default().fg(palette.accent),
        );
    }
    hits.folders.headers.push(FolderHit {
        rect,
        endpoint_id: folders.endpoint_id.clone(),
        folder_id: folder.folder_id.clone(),
    });
}

/// Gap after an entry: none before an indented child or a folder's first member.
pub(super) fn sidebar_entry_gap(entries: &[SidebarEntry], index: usize, row_gap: u16) -> u16 {
    let Some(next) = entries.get(index + 1) else {
        return 0;
    };
    if next.is_indented_workspace() {
        return 0;
    }
    if matches!(entries.get(index), Some(SidebarEntry::Folder { .. }))
        && matches!(next, SidebarEntry::Workspace { foldered: true, .. })
    {
        return 0;
    }
    row_gap
}

// ---------------------------------------------------------------------------
// Agents panel folder view
// ---------------------------------------------------------------------------

/// Whether the agents panel shows the folder view; an API agent view overrides it.
pub(super) fn agent_folder_view_active(
    snapshot: &ClientShellSnapshot,
    config: &ClientShellConfig,
) -> bool {
    config.agent_panel_sort == crate::config::AgentPanelSortConfig::Folders
        && snapshot.agent_view_label.is_none()
}

pub(super) enum AgentPanelRow {
    FolderHeader {
        folder_id: String,
        name: String,
        collapsed: bool,
        indicates_active: bool,
    },
    SpaceHeader {
        workspace_id: String,
        label: String,
        indented: bool,
        last_child: bool,
        foldered: bool,
        /// A family parent without agents, shown so its children are not orphaned.
        thin: bool,
        collapsed: bool,
        indicates_active: bool,
    },
    Agent {
        row: super::agent_sidebar::AgentRow,
        indent: u16,
    },
}

const AGENT_PANEL_HEADER_GUTTER: u16 = 1;

fn space_header_chevron_indent(indented: bool, foldered: bool) -> u16 {
    AGENT_PANEL_HEADER_GUTTER + if foldered { 2 } else { 0 } + if indented { 6 } else { 0 }
}

fn space_header_prefix_width(indented: bool, foldered: bool) -> u16 {
    space_header_chevron_indent(indented, foldered) + 2
}

fn space_header_agent_indent(indented: bool, foldered: bool) -> u16 {
    space_header_chevron_indent(indented, foldered) + 1
}

/// Project the flat agent sequence into folder-view rows. Folders and spaces
/// without agents are hidden, except a family parent whose children have agents.
pub(super) fn agent_folder_view_rows(
    snapshot: &ClientShellSnapshot,
    config: &ClientShellConfig,
    folders: &FolderRenderState<'_>,
) -> Vec<AgentPanelRow> {
    let agent_rows = super::agent_sidebar::agent_rows(snapshot, config, None);
    let mut agents_by_workspace = HashMap::<&str, Vec<usize>>::new();
    for (index, row) in agent_rows.iter().enumerate() {
        if let Some(agent) = snapshot
            .agents
            .iter()
            .find(|agent| agent.pane_id == row.pane_id)
        {
            agents_by_workspace
                .entry(agent.workspace_id.as_str())
                .or_default()
                .push(index);
        }
    }
    let has_agents = |workspace_id: &str| {
        agents_by_workspace
            .get(workspace_id)
            .is_some_and(|rows| !rows.is_empty())
    };

    let entries = sidebar_entries(snapshot, &HashSet::new(), &HashSet::new(), true);
    let workspace_id_at = |entry: &SidebarEntry| match entry {
        SidebarEntry::Workspace { entry, .. } => snapshot
            .workspaces
            .get(entry.index)
            .map(|workspace| workspace.workspace_id.as_str()),
        SidebarEntry::Folder { .. } => None,
    };
    let mut rows: Vec<AgentPanelRow> = Vec::new();
    let mut agent_row_indices: Vec<Option<usize>> = Vec::new();
    let mut pending_folder: Option<usize> = None;
    let mut in_collapsed_folder = false;
    let mut in_collapsed_parent = false;
    for (position, entry) in entries.iter().enumerate() {
        match entry {
            SidebarEntry::Folder { folder_index } => {
                let folder = &snapshot.folders[*folder_index];
                in_collapsed_parent = false;
                in_collapsed_folder = folders.collapsed_folders.contains(&folder.folder_id);
                if in_collapsed_folder {
                    pending_folder = None;
                    let members_have_agents = entries[position + 1..]
                        .iter()
                        .take_while(|next| {
                            matches!(next, SidebarEntry::Workspace { foldered: true, .. })
                        })
                        .any(|next| workspace_id_at(next).is_some_and(has_agents));
                    if members_have_agents {
                        let indicates_active = snapshot
                            .focused_workspace_id
                            .as_deref()
                            .is_some_and(|focused| {
                                folder.members.iter().any(|member| member == focused)
                            });
                        rows.push(AgentPanelRow::FolderHeader {
                            folder_id: folder.folder_id.clone(),
                            name: folder.name.clone(),
                            collapsed: true,
                            indicates_active,
                        });
                        agent_row_indices.push(None);
                    }
                } else {
                    pending_folder = Some(*folder_index);
                }
            }
            SidebarEntry::Workspace {
                entry: workspace_entry,
                foldered,
            } => {
                if !*foldered {
                    in_collapsed_folder = false;
                }
                if *foldered && in_collapsed_folder {
                    continue;
                }
                if !workspace_entry.indented {
                    in_collapsed_parent = false;
                } else if in_collapsed_parent {
                    continue;
                }
                let Some(workspace) = snapshot.workspaces.get(workspace_entry.index) else {
                    continue;
                };
                let workspace_id = workspace.workspace_id.as_str();
                let thin = !has_agents(workspace_id);
                if thin {
                    let child_has_agents = !workspace_entry.indented
                        && entries[position + 1..]
                            .iter()
                            .take_while(|next| next.is_indented_workspace())
                            .any(|next| workspace_id_at(next).is_some_and(has_agents));
                    if !child_has_agents {
                        continue;
                    }
                }
                if *foldered {
                    if let Some(folder_index) = pending_folder.take() {
                        let folder = &snapshot.folders[folder_index];
                        rows.push(AgentPanelRow::FolderHeader {
                            folder_id: folder.folder_id.clone(),
                            name: folder.name.clone(),
                            collapsed: false,
                            indicates_active: false,
                        });
                        agent_row_indices.push(None);
                    }
                }
                let collapsed = !thin && folders.collapsed_agent_spaces.contains(workspace_id);
                // A collapsed family parent hides its worktree children too.
                let hidden_child_focused = collapsed
                    && !workspace_entry.indented
                    && entries[position + 1..]
                        .iter()
                        .take_while(|next| next.is_indented_workspace())
                        .any(|next| {
                            workspace_id_at(next).is_some_and(|id| {
                                snapshot.focused_workspace_id.as_deref() == Some(id)
                            })
                        });
                if collapsed && !workspace_entry.indented {
                    in_collapsed_parent = true;
                }
                let label = if workspace_entry.indented && !workspace.custom_label {
                    workspace
                        .branch
                        .as_deref()
                        .and_then(|branch| branch.strip_prefix("worktree/").or(Some(branch)))
                        .unwrap_or(&workspace.label)
                        .to_owned()
                } else {
                    workspace.label.clone()
                };
                rows.push(AgentPanelRow::SpaceHeader {
                    workspace_id: workspace_id.to_owned(),
                    label,
                    indented: workspace_entry.indented,
                    last_child: workspace_entry.last_child,
                    foldered: *foldered,
                    thin,
                    collapsed,
                    indicates_active: collapsed && (workspace.focused || hidden_child_focused),
                });
                agent_row_indices.push(None);
                if collapsed {
                    continue;
                }
                let indent = space_header_agent_indent(workspace_entry.indented, *foldered);
                for index in agents_by_workspace.get(workspace_id).into_iter().flatten() {
                    rows.push(AgentPanelRow::Agent {
                        row: super::agent_sidebar::AgentRow {
                            pane_id: String::new(),
                            status: crate::api::schema::AgentStatus::Unknown,
                            focused: false,
                            rows: Vec::new(),
                        },
                        indent,
                    });
                    agent_row_indices.push(Some(*index));
                }
            }
        }
    }
    let mut agent_rows = agent_rows.into_iter().map(Some).collect::<Vec<_>>();
    for (row, source) in rows.iter_mut().zip(agent_row_indices) {
        if let (AgentPanelRow::Agent { row: slot, .. }, Some(index)) = (row, source) {
            if let Some(agent) = agent_rows.get_mut(index).and_then(Option::take) {
                *slot = agent;
            }
        }
    }
    rows
}

pub(super) fn render_agent_folder_view(
    buffer: &mut Buffer,
    area: Rect,
    snapshot: &ClientShellSnapshot,
    config: &ClientShellConfig,
    folders: &FolderRenderState<'_>,
    agent_scroll: &mut usize,
    hits: &mut ShellHitMap,
) {
    let palette = &config.palette;
    let rows = agent_folder_view_rows(snapshot, config, folders);
    let body = Rect::new(
        area.x,
        area.y.saturating_add(3),
        area.width,
        area.height.saturating_sub(3),
    );
    hits.agent_body = body;
    if body.is_empty() || rows.is_empty() {
        *agent_scroll = 0;
        return;
    }

    let row_heights = rows
        .iter()
        .map(|row| match row {
            AgentPanelRow::FolderHeader { .. } | AgentPanelRow::SpaceHeader { .. } => 1,
            AgentPanelRow::Agent { row, .. } => row.rows.len().max(1).min(u16::MAX as usize) as u16,
        })
        .collect::<Vec<_>>();
    let gaps = rows
        .iter()
        .enumerate()
        .map(|(index, row)| {
            if index + 1 >= rows.len() {
                0
            } else if matches!(row, AgentPanelRow::Agent { .. }) {
                config.agents.row_gap
            } else {
                0
            }
        })
        .collect::<Vec<_>>();
    let metrics =
        super::scroll::list_scroll_metrics(&row_heights, &gaps, body.height, *agent_scroll);
    hits.agent_max_scroll = metrics.max_offset_from_bottom;
    hits.agent_scroll_metrics = Some(metrics);
    *agent_scroll = metrics
        .max_offset_from_bottom
        .saturating_sub(metrics.offset_from_bottom);
    let show_scrollbar = metrics.max_offset_from_bottom > 0 && body.width > 1;
    let content_width = body.width.saturating_sub(u16::from(show_scrollbar));
    let mut y = body.y;
    for (index, row) in rows.iter().enumerate().skip(*agent_scroll) {
        let height = row_heights[index].min(body.height);
        if y.saturating_add(height) > body.bottom() {
            break;
        }
        let rect = Rect::new(body.x, y, content_width, height);
        match row {
            AgentPanelRow::FolderHeader {
                folder_id,
                name,
                collapsed,
                indicates_active,
            } => {
                if *indicates_active {
                    buffer.set_style(rect, Style::default().bg(palette.active_row_bg));
                }
                let name_style = if *indicates_active {
                    Style::default()
                        .fg(palette.text)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                        .fg(palette.subtext0)
                        .add_modifier(Modifier::BOLD)
                };
                let name = truncate_end(name, rect.width.saturating_sub(3) as usize);
                Paragraph::new(Line::from(vec![
                    Span::raw("   "),
                    Span::styled(name, name_style),
                ]))
                .render(rect, buffer);
                render::put_text(
                    buffer,
                    rect.x + AGENT_PANEL_HEADER_GUTTER,
                    rect.y,
                    1,
                    if *collapsed { "▸" } else { "▾" },
                    Style::default().fg(palette.accent),
                );
                hits.folders.agent_folder_headers.push(FolderHit {
                    rect,
                    endpoint_id: folders.endpoint_id.clone(),
                    folder_id: folder_id.clone(),
                });
            }
            AgentPanelRow::SpaceHeader {
                workspace_id,
                label,
                indented,
                last_child,
                foldered,
                thin,
                collapsed,
                indicates_active,
            } => {
                if *indicates_active {
                    buffer.set_style(rect, Style::default().bg(palette.active_row_bg));
                }
                let mut spans = vec![Span::raw(" ".repeat(AGENT_PANEL_HEADER_GUTTER as usize))];
                if *foldered {
                    spans.push(Span::raw("  "));
                }
                if *indented {
                    spans.push(Span::raw("   "));
                    spans.push(Span::styled(
                        if *last_child { "└─ " } else { "├─ " },
                        Style::default().fg(palette.overlay0),
                    ));
                }
                spans.push(Span::raw("  "));
                let prefix_width = space_header_prefix_width(*indented, *foldered);
                let name_style = if *thin {
                    Style::default()
                        .fg(palette.overlay0)
                        .add_modifier(Modifier::DIM)
                } else if *indicates_active {
                    Style::default()
                        .fg(palette.text)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                        .fg(palette.subtext0)
                        .add_modifier(Modifier::BOLD)
                };
                spans.push(Span::styled(
                    truncate_end(label, rect.width.saturating_sub(prefix_width) as usize),
                    name_style,
                ));
                Paragraph::new(Line::from(spans)).render(rect, buffer);
                if !*thin {
                    render::put_text(
                        buffer,
                        rect.x + space_header_chevron_indent(*indented, *foldered),
                        rect.y,
                        1,
                        if *collapsed { "▸" } else { "▾" },
                        Style::default().fg(palette.accent),
                    );
                    hits.folders
                        .agent_space_headers
                        .push((rect, workspace_id.clone()));
                }
            }
            AgentPanelRow::Agent { row, indent } => {
                hits.agents.push((rect, row.pane_id.clone()));
                let indent = (*indent).min(rect.width.saturating_sub(1));
                let inner = Rect::new(rect.x + indent, rect.y, rect.width - indent, rect.height);
                if row.focused {
                    buffer.set_style(rect, Style::default().bg(palette.active_row_bg));
                }
                super::agent_sidebar::render_agent_row(buffer, inner, row, config);
            }
        }
        y = y.saturating_add(height).saturating_add(gaps[index]);
    }

    if show_scrollbar {
        let track = Rect::new(body.right().saturating_sub(1), body.y, 1, body.height);
        hits.agent_scrollbar = track;
        super::scroll::render_list_scrollbar(buffer, track, metrics, palette);
    }
}
