//! Keyboard navigation around collapsed folders. Workspaces inside a
//! collapsed folder are skipped where hidden worktree children are skipped,
//! plus multi-machine next/previous. A focused workspace hidden in a
//! collapsed folder counts as sitting at the folder's position.

use super::*;

/// Steps from `current` over the entries that are not hidden.
pub(super) fn step_visible(hidden: &[bool], current: usize, delta: isize) -> Option<usize> {
    let visible = (0..hidden.len())
        .filter(|index| !hidden[*index])
        .collect::<Vec<_>>();
    if visible.is_empty() {
        return None;
    }
    if !hidden.get(current).copied().unwrap_or(true) {
        let position = visible.iter().position(|index| *index == current)?;
        let next = (position as isize + delta).rem_euclid(visible.len() as isize) as usize;
        return Some(visible[next]);
    }
    if delta >= 0 {
        visible
            .iter()
            .find(|index| **index > current)
            .or_else(|| visible.first())
            .copied()
    } else {
        visible
            .iter()
            .rev()
            .find(|index| **index < current)
            .or_else(|| visible.last())
            .copied()
    }
}

/// Navigate-mode step over entries with hidden flags; a hidden selection
/// starts at its folder's position. `None` when nothing is hidden.
pub(in crate::client::shell) fn folder_navigate_step(
    hidden: &[bool],
    current: Option<usize>,
    delta: isize,
) -> Option<usize> {
    if !hidden.iter().any(|hidden| *hidden) {
        return None;
    }
    match current {
        Some(current) => step_visible(hidden, current, delta),
        None if delta < 0 => hidden.iter().rposition(|hidden| !hidden),
        None => hidden.iter().position(|hidden| !hidden),
    }
}

impl ClientShellState {
    /// Visible workspace entries in sidebar order on a machine with folders,
    /// for digits and single-machine next/previous. `None` defers to upstream.
    pub(in crate::client::shell) fn folder_navigation_entries(
        &self,
        snapshot: &ClientShellSnapshot,
        collapsed_groups: &HashSet<String>,
    ) -> Option<Vec<WorkspaceEntry>> {
        let view = self.active_folder_view().filter(FolderView::has_folders)?;
        Some(
            folder_sidebar_entries(snapshot, Some(&view), collapsed_groups)
                .into_iter()
                .filter(|entry| entry.folder_header().is_none())
                .collect(),
        )
    }

    /// Next/previous from a focused workspace hidden in a collapsed folder:
    /// the index into `entries` to move to, or `None` when not hidden.
    pub(in crate::client::shell) fn folder_hidden_focus_step(
        &self,
        snapshot: &ClientShellSnapshot,
        entries: &[WorkspaceEntry],
        focused_workspace: &str,
        delta: isize,
    ) -> Option<usize> {
        if self.sidebar_collapsed {
            return None;
        }
        let view = self.active_folder_view()?;
        if !view.is_hidden(focused_workspace) {
            return None;
        }
        let empty = HashSet::new();
        let collapsed_groups = self
            .collapsed_groups_for_endpoint(&self.active_endpoint_id)
            .unwrap_or(&empty);
        let all = entries_with_hidden(snapshot, Some(&view), collapsed_groups);
        let id_at = |entry: &WorkspaceEntry| {
            snapshot
                .workspaces
                .get(entry.index)
                .map(|workspace| workspace.workspace_id.as_str())
        };
        let current = all
            .iter()
            .position(|(entry, _)| id_at(entry) == Some(focused_workspace))?;
        let hidden = all.iter().map(|(_, hidden)| *hidden).collect::<Vec<_>>();
        let target = step_visible(&hidden, current, delta)?;
        let target_id = id_at(&all[target].0)?;
        entries
            .iter()
            .position(|entry| id_at(entry) == Some(target_id))
    }

    /// Multi-machine next/previous workspace when any machine has folders:
    /// like upstream, but skipping workspaces in collapsed folders.
    pub(in crate::client::shell) fn folder_multi_machine_step(
        &mut self,
        next: bool,
        outcome: &mut ClientShellInput,
    ) -> bool {
        if self.sidebar_collapsed {
            return false;
        }
        let any_folders = self.endpoints.iter().any(|endpoint| {
            self.folders
                .view(endpoint)
                .is_some_and(|view| view.has_folders())
        });
        if !any_folders {
            return false;
        }
        let empty = HashSet::new();
        let mut workspaces = Vec::<(ClientEndpointId, String, bool)>::new();
        for endpoint in &self.endpoints {
            if endpoint.status != ClientEndpointStatus::Online {
                continue;
            }
            let Some(snapshot) = endpoint.snapshot.as_deref() else {
                continue;
            };
            let view = self.folders.view(endpoint);
            for (entry, hidden) in entries_with_hidden(snapshot, view.as_ref(), &empty) {
                if let Some(workspace) = snapshot.workspaces.get(entry.index) {
                    workspaces.push((
                        endpoint.endpoint_id.clone(),
                        workspace.workspace_id.clone(),
                        hidden,
                    ));
                }
            }
        }
        if workspaces.is_empty() {
            return true;
        }
        let focused = self
            .snapshot
            .as_deref()
            .and_then(|snapshot| snapshot.focused_workspace_id.as_deref());
        let current = workspaces
            .iter()
            .position(|(endpoint_id, workspace_id, _)| {
                endpoint_id == &self.active_endpoint_id && Some(workspace_id.as_str()) == focused
            });
        let hidden = workspaces
            .iter()
            .map(|(_, _, hidden)| *hidden)
            .collect::<Vec<_>>();
        let delta = if next { 1 } else { -1 };
        let target = match current {
            Some(current) => step_visible(&hidden, current, delta),
            None if next => hidden.iter().position(|hidden| !hidden),
            None => hidden.iter().rposition(|hidden| !hidden),
        };
        let Some((endpoint_id, workspace_id, _)) = target.map(|index| workspaces[index].clone())
        else {
            return true;
        };
        self.focus_or_activate(
            endpoint_id,
            ClientEndpointFocusTarget::Workspace(workspace_id),
            outcome,
        );
        true
    }

    /// Navigate-mode entries for one machine in the expanded sidebar, with
    /// hidden flags. `None` defers to upstream.
    pub(in crate::client::shell) fn folder_navigate_entries(
        &self,
        endpoint: &ClientShellEndpoint,
        snapshot: &ClientShellSnapshot,
        collapsed_groups: &HashSet<String>,
    ) -> Option<Vec<(WorkspaceEntry, bool)>> {
        let view = self
            .folders
            .view(endpoint)
            .filter(FolderView::has_folders)?;
        Some(entries_with_hidden(snapshot, Some(&view), collapsed_groups))
    }

    /// Scroll position of a workspace in the single-machine sidebar, counting
    /// folder headers.
    pub(in crate::client::shell) fn folder_reveal_position(
        &self,
        workspace_id: &str,
    ) -> Option<usize> {
        let snapshot = self.snapshot.as_deref()?;
        let view = self.active_folder_view().filter(FolderView::has_folders)?;
        let empty = HashSet::new();
        let collapsed_groups = self
            .collapsed_groups_for_endpoint(&self.active_endpoint_id)
            .unwrap_or(&empty);
        let entries = folder_sidebar_entries(snapshot, Some(&view), collapsed_groups);
        entries.iter().position(|entry| {
            snapshot
                .workspaces
                .get(entry.index)
                .is_some_and(|workspace| workspace.workspace_id == workspace_id)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::step_visible;

    #[test]
    fn hidden_current_moves_to_the_neighbours_of_its_folder() {
        let hidden = [false, false, true, true, false];
        assert_eq!(step_visible(&hidden, 3, 1), Some(4));
        assert_eq!(step_visible(&hidden, 3, -1), Some(1));
        assert_eq!(step_visible(&hidden, 1, 1), Some(4));
        assert_eq!(step_visible(&hidden, 4, 1), Some(0));
        let hidden = [true, false];
        assert_eq!(step_visible(&hidden, 0, -1), Some(1));
    }
}
