//! Sidebar entries with folder headers inserted between workspace rows.

use super::*;

/// Indent of folder member rows, matching the sidebar's nesting step.
pub(in crate::client::shell) const FOLDER_INDENT: u16 = 2;
/// `WorkspaceEntry::index` of a folder header row, which has no workspace.
const HEADER_INDEX: usize = usize::MAX;

/// Where a sidebar entry sits relative to folders.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(in crate::client::shell) enum FolderRow {
    #[default]
    Loose,
    /// A row inside an expanded folder, indented one step.
    Member,
    /// A folder header; `folder` indexes the machine's folder layout.
    Header { folder: usize, collapsed: bool },
}

impl WorkspaceEntry {
    pub(in crate::client::shell) fn folder_header(&self) -> Option<(usize, bool)> {
        match self.folder {
            FolderRow::Header { folder, collapsed } => Some((folder, collapsed)),
            _ => None,
        }
    }

    /// Extra indent for rows inside a folder.
    pub(in crate::client::shell) fn folder_indent(&self) -> u16 {
        if self.folder == FolderRow::Member {
            FOLDER_INDENT
        } else {
            0
        }
    }

    fn header(folder: usize, collapsed: bool) -> Self {
        Self {
            index: HEADER_INDEX,
            indented: false,
            last_child: false,
            folder: FolderRow::Header { folder, collapsed },
        }
    }
}

/// Indents a row's drawing area for folder members.
pub(in crate::client::shell) fn member_rect(rect: Rect, entry: &WorkspaceEntry) -> Rect {
    let indent = entry.folder_indent().min(rect.width);
    Rect::new(rect.x + indent, rect.y, rect.width - indent, rect.height)
}

/// The sidebar's entries: upstream's workspace rows, with folder headers
/// inserted and collapsed folders' members left out.
pub(in crate::client::shell) fn folder_sidebar_entries(
    snapshot: &ClientShellSnapshot,
    view: Option<&FolderView<'_>>,
    collapsed_groups: &HashSet<String>,
) -> Vec<WorkspaceEntry> {
    let base = render::workspace_entries(snapshot, collapsed_groups);
    match view.filter(|view| view.has_folders()) {
        Some(view) => arrange(snapshot, view, base, true),
        None => base,
    }
}

/// Every workspace in sidebar order, including members of collapsed
/// folders, paired with whether the sidebar hides it.
pub(in crate::client::shell) fn entries_with_hidden(
    snapshot: &ClientShellSnapshot,
    view: Option<&FolderView<'_>>,
    collapsed_groups: &HashSet<String>,
) -> Vec<(WorkspaceEntry, bool)> {
    let base = render::workspace_entries(snapshot, collapsed_groups);
    let Some(view) = view.filter(|view| view.has_folders()) else {
        return base.into_iter().map(|entry| (entry, false)).collect();
    };
    arrange(snapshot, view, base, false)
        .into_iter()
        .filter(|entry| entry.folder_header().is_none())
        .map(|entry| {
            let hidden = snapshot
                .workspaces
                .get(entry.index)
                .is_some_and(|workspace| view.is_hidden(&workspace.workspace_id));
            (entry, hidden)
        })
        .collect()
}

/// Every entry with folder headers, ignoring folder and worktree-group
/// collapse; the agents panel's folders tree walks this.
pub(in crate::client::shell) fn all_entries(
    snapshot: &ClientShellSnapshot,
    view: &FolderView<'_>,
) -> Vec<WorkspaceEntry> {
    let base = render::workspace_entries(snapshot, &HashSet::new());
    arrange(snapshot, view, base, false)
}

enum Top {
    Loose(Vec<WorkspaceEntry>),
    Folder(usize),
}

fn arrange(
    snapshot: &ClientShellSnapshot,
    view: &FolderView<'_>,
    base: Vec<WorkspaceEntry>,
    honor_collapse: bool,
) -> Vec<WorkspaceEntry> {
    let folders = &view.layout.folders;
    let mut buckets = vec![Vec::new(); folders.len()];
    let mut placed = vec![false; folders.len()];
    let mut top = Vec::<Top>::new();
    for entry in base {
        let Some(workspace) = snapshot.workspaces.get(entry.index) else {
            continue;
        };
        match view.folder_index_of(&workspace.workspace_id) {
            Some(folder) if folder < folders.len() => {
                buckets[folder].push(entry);
                if !placed[folder] {
                    placed[folder] = true;
                    top.push(Top::Folder(folder));
                }
            }
            _ => match top.last_mut() {
                Some(Top::Loose(unit)) if entry.indented => unit.push(entry),
                _ => top.push(Top::Loose(vec![entry])),
            },
        }
    }

    let folder_index = folders
        .iter()
        .enumerate()
        .map(|(index, folder)| (folder.id.as_str(), index))
        .collect::<HashMap<_, _>>();
    let mut anchors = HashMap::<&str, usize>::new();
    for (position, item) in top.iter().enumerate() {
        match item {
            Top::Loose(unit) => {
                for entry in unit {
                    if let Some(workspace) = snapshot.workspaces.get(entry.index) {
                        anchors.insert(workspace.workspace_id.as_str(), position);
                    }
                }
            }
            Top::Folder(folder) => {
                anchors.insert(folders[*folder].id.as_str(), position);
            }
        }
    }
    let mut following = vec![Vec::new(); top.len() + 1];
    let mut slot = 0;
    for item in &view.layout.order {
        if let Some(position) = anchors.get(item.as_str()) {
            slot = position + 1;
            continue;
        }
        let Some(&folder) = folder_index.get(item.as_str()) else {
            continue;
        };
        if !placed[folder] {
            placed[folder] = true;
            following[slot].push(folder);
        }
    }
    let mut following = following.into_iter();
    let mut ordered = following
        .next()
        .unwrap_or_default()
        .into_iter()
        .map(Top::Folder)
        .collect::<Vec<_>>();
    for (item, after) in top.into_iter().zip(following) {
        ordered.push(item);
        ordered.extend(after.into_iter().map(Top::Folder));
    }
    let mut top = ordered;
    for (folder, placed) in placed.iter().enumerate() {
        if !placed {
            top.push(Top::Folder(folder));
        }
    }

    let mut entries = Vec::new();
    for item in top {
        match item {
            Top::Loose(unit) => entries.extend(unit),
            Top::Folder(folder) => {
                let collapsed = view.is_collapsed(folder);
                entries.push(WorkspaceEntry::header(folder, collapsed));
                if !(honor_collapse && collapsed) {
                    entries.extend(std::mem::take(&mut buckets[folder]).into_iter().map(
                        |mut entry| {
                            entry.folder = FolderRow::Member;
                            entry
                        },
                    ));
                }
            }
        }
    }
    entries
}

/// Whether revealing the focused workspace should scroll to this entry: the
/// focused row itself, or the collapsed folder hiding it.
pub(in crate::client::shell) fn entry_reveals_focus(
    snapshot: &ClientShellSnapshot,
    view: Option<&FolderView<'_>>,
    entry: &WorkspaceEntry,
) -> bool {
    match entry.folder_header() {
        Some((folder, true)) => view.is_some_and(|view| {
            view.layout.folders.get(folder).is_some_and(|folder| {
                folder
                    .workspace_ids
                    .iter()
                    .any(|id| snapshot.focused_workspace_id.as_deref() == Some(id.as_str()))
            })
        }),
        Some(_) => false,
        None => snapshot
            .workspaces
            .get(entry.index)
            .is_some_and(|workspace| workspace.focused),
    }
}
