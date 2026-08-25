//! Folder hit testing and drag-and-drop input for the sidebar.

use ratatui::layout::Rect;

use crate::app::state::AppState;

use super::sidebar::AgentPanelCollapseTarget;

impl AppState {
    /// The folder whose spaces-panel header row is at `row`, if any.
    pub(super) fn folder_header_at(&self, row: u16) -> Option<String> {
        let footer = self.sidebar_footer_rect();
        if footer == Rect::default() {
            return None;
        }

        // The view caches card and header areas together; recompute both when
        // the cache is cold (mirrors `workspace_at_row`).
        let headers = if self.view.workspace_card_areas.is_empty() {
            crate::ui::compute_workspace_list_areas(self, self.view.sidebar_rect).1
        } else {
            self.view.folder_header_areas.clone()
        };

        headers.iter().find_map(|header| {
            (row >= header.rect.y && row < header.rect.y + header.rect.height)
                .then(|| header.folder_id.clone())
        })
    }

    /// The drop target for an in-flight folder drag. Folders only reorder at
    /// the top level: in-folder slots and header-append targets are excluded,
    /// so a folder can never be dropped into another folder.
    pub(super) fn folder_drop_target_at_row(
        &self,
        row: u16,
    ) -> Option<crate::app::state::WorkspaceDropTarget> {
        self.nearest_drop_slot_at_row(row, true)
    }

    pub(super) fn workspace_list_row_in_range(&self, row: u16) -> bool {
        let area = self.workspace_list_rect();
        let footer = self.sidebar_footer_rect();
        area != Rect::default() && row >= area.y && row < footer.y
    }

    pub(super) fn nearest_drop_slot_at_row(
        &self,
        row: u16,
        top_level_only: bool,
    ) -> Option<crate::app::state::WorkspaceDropTarget> {
        if !self.workspace_list_row_in_range(row) {
            return None;
        }

        let (cards, headers) = self.drop_slot_areas();
        let area = self.workspace_list_rect();
        crate::ui::workspace_drop_slots(self, &cards, &headers, area)
            .into_iter()
            .filter(|(target, _)| !top_level_only || crate::ui::is_top_level_drop_target(target))
            .enumerate()
            .min_by_key(|(slot_idx, (_, slot_row))| (row.abs_diff(*slot_row), *slot_idx))
            .map(|(_, (target, _))| target)
    }

    /// Card and header geometry for drop-slot computation, recomputed when the
    /// view cache is cold (mirrors `workspace_at_row` / `folder_header_at`).
    fn drop_slot_areas(
        &self,
    ) -> (
        Vec<crate::app::state::WorkspaceCardArea>,
        Vec<crate::app::state::FolderHeaderArea>,
    ) {
        if self.view.workspace_card_areas.is_empty() {
            crate::ui::compute_workspace_list_areas(self, self.view.sidebar_rect)
        } else {
            (
                self.view.workspace_card_areas.clone(),
                self.view.folder_header_areas.clone(),
            )
        }
    }

    /// Resolve a finished space drag into positional `folder.assign` params:
    /// membership and position in one gesture. Returns `None` when the drop
    /// changes nothing. Family atomicity is guaranteed twice over: slots are
    /// only emitted at block boundaries here, and the assign mutation moves
    /// the source's whole worktree family as one block.
    pub(super) fn workspace_drop_assign_params(
        &self,
        source_ws_idx: usize,
        drop_target: &crate::app::state::WorkspaceDropTarget,
    ) -> Option<crate::api::schema::FolderAssignParams> {
        use crate::app::state::WorkspaceDropTarget;
        use crate::folder::SpaceOrderEntry;

        let source = self.workspaces.get(source_ws_idx)?;
        // The drop moves the source's whole worktree family, in workspace
        // vec order — mirroring `assign_workspace_to_folder`.
        let affected = self.block_workspace_ids(source_ws_idx)?;
        let affected_set: std::collections::HashSet<&str> =
            affected.iter().map(String::as_str).collect();

        let ids: Vec<&str> = self.workspaces.iter().map(|ws| ws.id.as_str()).collect();
        let entries = crate::folder::normalized_space_order(&self.space_order, &ids);

        // Count the surviving entries before an anchor: `position` addresses
        // the target container after the affected block is removed from it.
        let surviving_before = |anchor_idx: usize| {
            entries[..anchor_idx]
                .iter()
                .filter(|entry| match entry {
                    SpaceOrderEntry::Workspace(id) => !affected_set.contains(id.as_str()),
                    SpaceOrderEntry::Folder(_) => true,
                })
                .count()
        };

        let (folder_id, position) = match drop_target {
            WorkspaceDropTarget::Before(anchor_ws_idx) => {
                let anchor_idx = self.top_level_block_start_index(&entries, *anchor_ws_idx)?;
                (None, Some(surviving_before(anchor_idx)))
            }
            WorkspaceDropTarget::BeforeFolder(folder_id) => {
                let anchor_idx = entries.iter().position(|entry| {
                    matches!(entry, SpaceOrderEntry::Folder(folder) if folder.id == *folder_id)
                })?;
                (None, Some(surviving_before(anchor_idx)))
            }
            WorkspaceDropTarget::End => (None, Some(surviving_before(entries.len()))),
            WorkspaceDropTarget::InFolderBefore { folder_id, ws_idx } => {
                let members = entries.iter().find_map(|entry| match entry {
                    SpaceOrderEntry::Folder(folder) if folder.id == *folder_id => {
                        Some(&folder.members)
                    }
                    _ => None,
                })?;
                let anchor_block = self.block_workspace_ids(*ws_idx)?;
                let anchor_pos = members
                    .iter()
                    .position(|member| anchor_block.contains(member))?;
                let position = members[..anchor_pos]
                    .iter()
                    .filter(|member| !affected_set.contains(member.as_str()))
                    .count();
                (Some(folder_id.clone()), Some(position))
            }
            WorkspaceDropTarget::InFolderEnd { folder_id }
            | WorkspaceDropTarget::IntoFolder(folder_id) => (Some(folder_id.clone()), None),
        };

        // Suppress no-op drops by simulating the assign against the same
        // normalized order the mutation operates on.
        let mut final_entries = entries.clone();
        for entry in &mut final_entries {
            if let SpaceOrderEntry::Folder(folder) = entry {
                folder
                    .members
                    .retain(|member| !affected_set.contains(member.as_str()));
            }
        }
        final_entries.retain(|entry| match entry {
            SpaceOrderEntry::Workspace(id) => !affected_set.contains(id.as_str()),
            SpaceOrderEntry::Folder(_) => true,
        });
        match &folder_id {
            Some(folder_id) => {
                let folder = final_entries.iter_mut().find_map(|entry| match entry {
                    SpaceOrderEntry::Folder(folder) if folder.id == *folder_id => Some(folder),
                    _ => None,
                })?;
                let index = position
                    .unwrap_or(folder.members.len())
                    .min(folder.members.len());
                folder
                    .members
                    .splice(index..index, affected.iter().cloned());
            }
            None => {
                let index = position
                    .unwrap_or(final_entries.len())
                    .min(final_entries.len());
                final_entries.splice(
                    index..index,
                    affected
                        .iter()
                        .map(|id| SpaceOrderEntry::Workspace(id.clone())),
                );
            }
        }
        if final_entries == entries {
            return None;
        }

        Some(crate::api::schema::FolderAssignParams {
            workspace_id: source.id.clone(),
            folder_id,
            position,
        })
    }

    /// Resolve a finished folder drag into `folder.move` params. Returns
    /// `None` for no-op drops (including dropping a folder before itself)
    /// and for targets that are not top-level.
    pub(super) fn folder_drop_move_params(
        &self,
        folder_id: &str,
        drop_target: &crate::app::state::WorkspaceDropTarget,
    ) -> Option<crate::api::schema::FolderMoveParams> {
        use crate::app::state::WorkspaceDropTarget;
        use crate::folder::SpaceOrderEntry;

        let ids: Vec<&str> = self.workspaces.iter().map(|ws| ws.id.as_str()).collect();
        let entries = crate::folder::normalized_space_order(&self.space_order, &ids);
        let current = entries.iter().position(
            |entry| matches!(entry, SpaceOrderEntry::Folder(folder) if folder.id == folder_id),
        )?;

        // `position` is the folder's final index after removal: anchors past
        // the folder's current slot shift down by one once it is taken out.
        let final_index_before = |anchor_idx: usize| {
            if anchor_idx > current {
                anchor_idx - 1
            } else {
                anchor_idx
            }
        };
        let position = match drop_target {
            WorkspaceDropTarget::Before(anchor_ws_idx) => {
                let anchor_idx = self.top_level_block_start_index(&entries, *anchor_ws_idx)?;
                final_index_before(anchor_idx)
            }
            WorkspaceDropTarget::BeforeFolder(other_folder_id) => {
                if other_folder_id == folder_id {
                    return None;
                }
                let anchor_idx = entries.iter().position(|entry| {
                    matches!(
                        entry,
                        SpaceOrderEntry::Folder(folder) if folder.id == *other_folder_id
                    )
                })?;
                final_index_before(anchor_idx)
            }
            WorkspaceDropTarget::End => entries.len().saturating_sub(1),
            // A folder can never be dropped into another folder.
            WorkspaceDropTarget::InFolderBefore { .. }
            | WorkspaceDropTarget::InFolderEnd { .. }
            | WorkspaceDropTarget::IntoFolder(_) => {
                return None;
            }
        };
        if position == current {
            return None;
        }

        Some(crate::api::schema::FolderMoveParams {
            folder_id: folder_id.to_string(),
            position,
        })
    }

    /// The index in the top-level order of the first entry belonging to the
    /// block anchored at `anchor_ws_idx` — the anchor's whole worktree family
    /// when it has one, else the anchor itself.
    fn top_level_block_start_index(
        &self,
        entries: &[crate::folder::SpaceOrderEntry],
        anchor_ws_idx: usize,
    ) -> Option<usize> {
        let block = self.block_workspace_ids(anchor_ws_idx)?;
        entries.iter().position(|entry| {
            matches!(
                entry,
                crate::folder::SpaceOrderEntry::Workspace(id) if block.contains(id)
            )
        })
    }

    /// The workspace ids forming the block anchored at `ws_idx`, in workspace
    /// vec order: the whole worktree family when the workspace belongs to
    /// one, else just itself.
    fn block_workspace_ids(&self, ws_idx: usize) -> Option<Vec<String>> {
        let anchor = self.workspaces.get(ws_idx)?;
        Some(match anchor.worktree_space() {
            Some(space) => self
                .workspaces
                .iter()
                .filter(|ws| {
                    ws.worktree_space()
                        .is_some_and(|member| member.key == space.key)
                })
                .map(|ws| ws.id.clone())
                .collect(),
            None => vec![anchor.id.clone()],
        })
    }

    /// The collapse hit (if any) at this cell of the agents panel: anywhere
    /// on a folder header row toggles the shared folder collapse, anywhere
    /// on a space header row toggles that space's agent-list collapse. Thin
    /// ancestor headers have no agents of their own and stay inert.
    pub(super) fn agent_panel_collapse_target_at(
        &self,
        col: u16,
        row: u16,
    ) -> Option<AgentPanelCollapseTarget> {
        let (list_row, _row_y, body) = self.agent_panel_row_hit(row)?;
        if col < body.x || col >= body.x + body.width {
            return None;
        }
        match &list_row {
            crate::ui::AgentPanelListEntry::FolderHeader { order_idx } => {
                match self.space_order.get(*order_idx) {
                    Some(crate::folder::SpaceOrderEntry::Folder(folder)) => {
                        Some(AgentPanelCollapseTarget::Folder(folder.id.clone()))
                    }
                    _ => None,
                }
            }
            crate::ui::AgentPanelListEntry::SpaceHeader {
                ws_idx,
                thin: false,
                ..
            } => {
                let ws = self.workspaces.get(*ws_idx)?;
                Some(AgentPanelCollapseTarget::Space(ws.id.clone()))
            }
            _ => None,
        }
    }

    /// The agents-panel display row under `row`, with the row's top y and the
    /// panel body rect. Shared row-walk for agent and chevron hit-testing.
    pub(super) fn agent_panel_row_hit(
        &self,
        row: u16,
    ) -> Option<(crate::ui::AgentPanelListEntry, u16, Rect)> {
        if self.sidebar_collapsed {
            return None;
        }

        let detail_area = self.agent_panel_rect();
        let metrics = crate::ui::agent_panel_scroll_metrics(self, detail_area);
        let body = crate::ui::agent_panel_body_rect(
            detail_area,
            crate::ui::should_show_scrollbar(metrics),
        );
        if body.height == 0 || row < body.y || row >= body.y + body.height {
            return None;
        }

        let mut row_y = body.y;
        let body_bottom = body.y + body.height;
        let entries = crate::ui::agent_panel_entries(self);
        let rows = crate::ui::agent_panel_list_entries(self, &entries);
        let scroll = self.agent_panel_scroll.min(metrics.max_offset_from_bottom);
        for (index, list_row) in rows.iter().enumerate().skip(scroll) {
            let height = crate::ui::agent_row_height_in_body(self, &entries, list_row, body.height);
            if row_y.saturating_add(height) > body_bottom {
                break;
            }
            if row >= row_y && row < row_y.saturating_add(height) {
                return Some((list_row.clone(), row_y, body));
            }
            row_y = row_y
                .saturating_add(height)
                .saturating_add(crate::ui::agent_row_gap(self, &rows, index))
                .min(body_bottom);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{MouseButton, MouseEventKind};
    use ratatui::layout::Rect;

    use super::super::sidebar::tests::workspace_with_space;
    use super::super::{app_for_mouse_test, capture_snapshot, mouse};
    use crate::{
        app::state::{AgentPanelSort, DragTarget, Mode},
        app::App,
        detect::Agent,
        workspace::Workspace,
    };

    #[test]
    fn folder_view_agent_hit_testing_skips_headers_and_targets_agents() {
        let mut app = app_for_mouse_test();
        let first = Workspace::test_new("one");
        let first_pane = first.tabs[0].root_pane;
        let second = Workspace::test_new("two");
        let second_pane = second.tabs[0].root_pane;
        app.state.workspaces = vec![first, second];
        app.state.ensure_test_terminals();
        app.state.active = Some(0);
        app.state.selected = 0;
        for (ws_idx, pane_id) in [(0, first_pane), (1, second_pane)] {
            let terminal_id = app.state.workspaces[ws_idx].tabs[0].panes[&pane_id]
                .attached_terminal_id
                .clone();
            app.state
                .terminals
                .get_mut(&terminal_id)
                .unwrap()
                .detected_agent = Some(Agent::Claude);
        }
        app.state.sidebar_agents.rows = vec![vec![crate::config::AgentSidebarToken::StateIcon]];
        app.state.sidebar_agents.row_gap = 0;
        app.state.agent_panel_sort = AgentPanelSort::Folders;

        let detail_area = app.state.agent_panel_rect();
        let metrics = crate::ui::agent_panel_scroll_metrics(&app.state, detail_area);
        let body = crate::ui::agent_panel_body_rect(
            detail_area,
            crate::ui::should_show_scrollbar(metrics),
        );

        // Rows: header(one), agent, header(two), agent.
        assert_eq!(app.state.agent_detail_target_at(body.y), None);
        assert_eq!(
            app.state.agent_detail_target_at(body.y + 1),
            Some((0, 0, first_pane))
        );
        assert_eq!(app.state.agent_detail_target_at(body.y + 2), None);
        assert_eq!(
            app.state.agent_detail_target_at(body.y + 3),
            Some((1, 0, second_pane))
        );
    }

    #[test]
    fn clicking_agent_panel_toggle_cycles_grouped_priority_folders() {
        let mut app = app_for_mouse_test();
        app.state.workspaces = vec![Workspace::test_new("test")];
        app.state.active = Some(0);
        app.state.selected = 0;
        app.state.mode = Mode::Terminal;
        app.state.agent_panel_scroll = 3;

        let click_toggle = |app: &mut crate::app::App| {
            let (_, detail_area) = crate::ui::expanded_sidebar_sections(
                app.state.view.sidebar_rect,
                app.state.sidebar_section_split,
            );
            let toggle =
                crate::ui::agent_panel_toggle_rect(detail_area, app.state.agent_panel_sort);
            app.handle_mouse(mouse(
                MouseEventKind::Down(MouseButton::Left),
                toggle.x,
                toggle.y,
            ));
        };

        click_toggle(&mut app);
        assert_eq!(app.state.agent_panel_sort, AgentPanelSort::Priority);
        assert_eq!(app.state.agent_panel_scroll, 0);

        click_toggle(&mut app);
        assert_eq!(app.state.agent_panel_sort, AgentPanelSort::Folders);

        click_toggle(&mut app);
        assert_eq!(app.state.agent_panel_sort, AgentPanelSort::Spaces);
    }

    #[test]
    fn clicking_folder_header_chevron_toggles_folder_collapse() {
        let mut app = app_for_mouse_test();
        app.state.workspaces = vec![Workspace::test_new("one"), Workspace::test_new("two")];
        let member = app.state.workspaces[1].id.clone();
        let folder_id = app.state.create_folder("work").expect("create folder");
        app.state
            .assign_workspace_to_folder(&member, Some(&folder_id), None)
            .expect("assign");
        app.state.active = None;
        app.state.mode = Mode::Terminal;
        crate::ui::compute_view(&mut app.state, Rect::new(0, 0, 106, 20));
        let header = app.state.view.folder_header_areas[0].clone();
        let chevron = crate::ui::folder_header_chevron_rect(&header);

        app.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            chevron.x,
            chevron.y,
        ));

        assert_eq!(app.state.active, None);
        assert!(app.state.workspace_presses.is_empty());
        assert!(app.state.collapsed_folder_ids.contains(&folder_id));
        // Collapse is purely visual: nothing closed, moved, or reordered.
        assert_eq!(app.state.workspaces.len(), 2);
        assert_eq!(
            app.state.workspace_folder_id(&member),
            Some(folder_id.as_str())
        );
        let snapshot = capture_snapshot(&app.state);
        assert!(snapshot.collapsed_folder_ids.contains(&folder_id));

        app.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            chevron.x,
            chevron.y,
        ));

        assert!(!app.state.collapsed_folder_ids.contains(&folder_id));
    }

    #[test]
    fn clicking_folder_header_row_toggles_collapse() {
        let mut app = app_for_mouse_test();
        app.state.workspaces = vec![Workspace::test_new("one"), Workspace::test_new("two")];
        let member = app.state.workspaces[1].id.clone();
        let folder_id = app.state.create_folder("work").expect("create folder");
        app.state
            .assign_workspace_to_folder(&member, Some(&folder_id), None)
            .expect("assign");
        app.state.active = None;
        app.state.mode = Mode::Terminal;
        crate::ui::compute_view(&mut app.state, Rect::new(0, 0, 106, 20));
        let header = app.state.view.folder_header_areas[0].clone();
        let (col, row) = (header.rect.x + 2, header.rect.y);

        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), col, row));

        // The press alone toggles nothing: it may still become a drag.
        assert!(!app.state.collapsed_folder_ids.contains(&folder_id));
        assert!(app.state.workspace_presses.is_empty());

        app.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), col, row));

        assert!(app.state.collapsed_folder_ids.contains(&folder_id));
        assert!(app.state.folder_presses.is_empty());
        // Collapse is purely visual: nothing closed, moved, or reordered.
        assert_eq!(app.state.workspaces.len(), 2);
        assert_eq!(
            app.state.workspace_folder_id(&member),
            Some(folder_id.as_str())
        );
        let snapshot = capture_snapshot(&app.state);
        assert!(snapshot.collapsed_folder_ids.contains(&folder_id));

        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), col, row));
        app.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), col, row));

        assert!(!app.state.collapsed_folder_ids.contains(&folder_id));
    }

    /// Folder-view agents panel: loose "one" then folder "work" containing
    /// "two", one agent per space, single-row agent entries with no gap.
    /// Rows: header(one), agent, folder(work), header(two), agent.
    fn folder_view_collapse_mouse_app() -> (crate::app::App, String) {
        let mut app = app_for_mouse_test();
        app.state.workspaces = vec![Workspace::test_new("one"), Workspace::test_new("two")];
        let member = app.state.workspaces[1].id.clone();
        let folder_id = app.state.create_folder("work").expect("create folder");
        app.state
            .assign_workspace_to_folder(&member, Some(&folder_id), None)
            .expect("assign");
        app.state.ensure_test_terminals();
        for ws_idx in 0..app.state.workspaces.len() {
            let pane_id = app.state.workspaces[ws_idx].tabs[0].root_pane;
            let terminal_id = app.state.workspaces[ws_idx].tabs[0].panes[&pane_id]
                .attached_terminal_id
                .clone();
            app.state
                .terminals
                .get_mut(&terminal_id)
                .unwrap()
                .detected_agent = Some(Agent::Claude);
        }
        app.state.active = Some(0);
        app.state.selected = 0;
        app.state.mode = Mode::Terminal;
        app.state.sidebar_agents.rows = vec![vec![crate::config::AgentSidebarToken::StateIcon]];
        app.state.sidebar_agents.row_gap = 0;
        app.state.agent_panel_sort = AgentPanelSort::Folders;
        (app, folder_id)
    }

    fn agent_panel_body(app: &crate::app::App) -> Rect {
        let detail_area = app.state.agent_panel_rect();
        let metrics = crate::ui::agent_panel_scroll_metrics(&app.state, detail_area);
        crate::ui::agent_panel_body_rect(detail_area, crate::ui::should_show_scrollbar(metrics))
    }

    #[test]
    fn clicking_agents_panel_folder_chevron_toggles_shared_folder_collapse() {
        let (mut app, folder_id) = folder_view_collapse_mouse_app();
        let body = agent_panel_body(&app);
        // Folder headers lead with their chevron after the 1-cell gutter.
        let chevron_col = body.x + 1;
        let folder_row = body.y + 2;

        app.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            chevron_col,
            folder_row,
        ));

        assert!(app.state.collapsed_folder_ids.contains(&folder_id));
        assert_eq!(app.state.active, Some(0), "collapse never changes focus");
        // One shared state: the spaces panel hides the member too.
        assert!(
            !crate::ui::workspace_list_entries(&app.state)
                .iter()
                .any(|entry| matches!(
                    entry,
                    crate::ui::WorkspaceListEntry::Workspace { ws_idx: 1, .. }
                )),
            "collapsing from the agents panel collapses the spaces panel folder"
        );
        let snapshot = capture_snapshot(&app.state);
        assert!(snapshot.collapsed_folder_ids.contains(&folder_id));

        // The collapsed folder header stays at the same row: toggle back.
        app.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            chevron_col,
            folder_row,
        ));
        assert!(!app.state.collapsed_folder_ids.contains(&folder_id));
    }

    #[test]
    fn clicking_agents_panel_space_chevron_toggles_agent_list_collapse() {
        let (mut app, folder_id) = folder_view_collapse_mouse_app();
        let two_id = app.state.workspaces[1].id.clone();
        let body = agent_panel_body(&app);
        // The foldered space header's chevron sits past the gutter and the
        // folder margin.
        let chevron_col = body.x + 3;
        let space_row = body.y + 3;

        app.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            chevron_col,
            space_row,
        ));

        assert!(app.state.collapsed_agent_space_ids.contains(&two_id));
        assert!(
            !app.state.collapsed_folder_ids.contains(&folder_id),
            "agent-list collapse is independent of folder collapse"
        );
        assert_eq!(
            crate::ui::agent_panel_entries(&app.state).len(),
            2,
            "collapse never filters the flat agent sequence"
        );
        let snapshot = capture_snapshot(&app.state);
        assert!(snapshot.collapsed_agent_space_ids.contains(&two_id));

        // The collapsed space header stays at the same row: toggle back.
        app.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            chevron_col,
            space_row,
        ));
        assert!(!app.state.collapsed_agent_space_ids.contains(&two_id));
    }

    #[test]
    fn clicking_spaces_panel_folder_chevron_collapses_agents_panel_folder_view() {
        let (mut app, folder_id) = folder_view_collapse_mouse_app();
        crate::ui::compute_view(&mut app.state, Rect::new(0, 0, 106, 20));
        let header = app.state.view.folder_header_areas[0].clone();
        let chevron = crate::ui::folder_header_chevron_rect(&header);

        app.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            chevron.x,
            chevron.y,
        ));

        assert!(app.state.collapsed_folder_ids.contains(&folder_id));
        // One shared state: the agents-panel folder view hides the member's
        // space header and agents, keeping the folder header.
        let entries = crate::ui::agent_panel_entries(&app.state);
        let rows = crate::ui::agent_panel_list_entries(&app.state, &entries);
        assert!(
            !rows.iter().any(|row| matches!(
                row,
                crate::ui::AgentPanelListEntry::SpaceHeader { ws_idx: 1, .. }
                    | crate::ui::AgentPanelListEntry::Agent { entry_idx: 1 }
            )),
            "collapsing from the spaces panel collapses the folder view: {rows:?}"
        );
        assert!(rows
            .iter()
            .any(|row| matches!(row, crate::ui::AgentPanelListEntry::FolderHeader { .. })));
    }

    #[test]
    fn clicking_agents_panel_header_row_off_chevron_toggles() {
        let (mut app, folder_id) = folder_view_collapse_mouse_app();
        let two_id = app.state.workspaces[1].id.clone();
        let body = agent_panel_body(&app);
        let folder_row = body.y + 2;

        // Anywhere on the folder header row (chevron at x+1) toggles the
        // shared folder collapse, out to the row's last body cell.
        app.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            body.x + body.width - 1,
            folder_row,
        ));
        assert!(app.state.collapsed_folder_ids.contains(&folder_id));

        // The collapsed folder header stays at the same row: toggle back.
        app.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            body.x + 2,
            folder_row,
        ));
        assert!(!app.state.collapsed_folder_ids.contains(&folder_id));

        // Anywhere on the foldered space header row (chevron at x+3) toggles
        // that space's agent-list collapse.
        app.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            body.x + 4,
            body.y + 3,
        ));
        assert!(app.state.collapsed_agent_space_ids.contains(&two_id));
        assert!(
            !app.state.collapsed_folder_ids.contains(&folder_id),
            "agent-list collapse is independent of folder collapse"
        );
    }

    #[test]
    fn thin_ancestor_header_row_stays_inert() {
        let mut app = app_for_mouse_test();
        app.state.workspaces = vec![Workspace::test_new("main"), Workspace::test_new("issue")];
        for (idx, checkout_path) in ["/repo/herdr", "/repo/herdr-issue"].into_iter().enumerate() {
            app.state.workspaces[idx].worktree_space =
                Some(crate::workspace::WorktreeSpaceMembership {
                    key: "repo-key".into(),
                    label: "herdr".into(),
                    repo_root: "/repo/herdr".into(),
                    checkout_path: checkout_path.into(),
                    is_linked_worktree: idx > 0,
                });
        }
        app.state.ensure_test_terminals();
        // Only the child has an agent: the parent renders as a thin header.
        let pane_id = app.state.workspaces[1].tabs[0].root_pane;
        let terminal_id = app.state.workspaces[1].tabs[0].panes[&pane_id]
            .attached_terminal_id
            .clone();
        app.state
            .terminals
            .get_mut(&terminal_id)
            .unwrap()
            .detected_agent = Some(Agent::Claude);
        app.state.active = Some(0);
        app.state.selected = 0;
        app.state.mode = Mode::Terminal;
        app.state.sidebar_agents.rows = vec![vec![crate::config::AgentSidebarToken::StateIcon]];
        app.state.sidebar_agents.row_gap = 0;
        app.state.agent_panel_sort = AgentPanelSort::Folders;
        let body = agent_panel_body(&app);
        // The cell a loose header's leading chevron would occupy (after the
        // gutter), plus a cell further along the row.
        // Rows: thin header(main), header(issue), agent.
        for col in [body.x + 1, body.x + 5] {
            app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), col, body.y));
        }

        assert!(
            app.state.collapsed_agent_space_ids.is_empty(),
            "a thin ancestor header exposes no agent-list collapse"
        );
    }

    #[test]
    fn plain_drag_before_a_parentless_linked_family_lands_before_the_whole_family() {
        let mut app = app_for_mouse_test();
        app.state.workspaces = vec![
            workspace_with_space("one", "repo-key"),
            workspace_with_space("two", "repo-key"),
            Workspace::test_new("normal"),
        ];

        // "one" and "two" share a worktree family: dropping before either
        // member anchors to the family block, never inside it.
        let params = app
            .state
            .workspace_drop_assign_params(2, &crate::app::state::WorkspaceDropTarget::Before(1))
            .unwrap();

        assert_eq!(params.workspace_id, app.state.workspaces[2].id);
        assert_eq!(params.folder_id, None);
        assert_eq!(params.position, Some(0));
    }

    #[test]
    fn dragging_worktree_family_member_moves_the_whole_family() {
        let mut app = app_for_mouse_test();
        app.state.workspaces = vec![
            workspace_with_space("main", "repo-key"),
            Workspace::test_new("normal"),
            workspace_with_space("issue", "repo-key"),
        ];
        app.state.ensure_test_terminals();
        app.state.active = Some(0);
        app.state.selected = 0;
        crate::ui::compute_view(&mut app.state, Rect::new(0, 0, 106, 40));

        let source = app
            .state
            .view
            .workspace_card_areas
            .iter()
            .find(|card| card.ws_idx == 2)
            .unwrap()
            .rect;
        let target_row = crate::ui::workspace_drop_indicator_row(
            &app.state,
            &app.state.view.workspace_card_areas,
            &app.state.view.folder_header_areas,
            app.state.workspace_list_rect(),
            &crate::app::state::WorkspaceDropTarget::End,
        )
        .unwrap();

        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), 2, source.y));
        app.handle_mouse(mouse(
            MouseEventKind::Drag(MouseButton::Left),
            2,
            target_row,
        ));
        // Grabbing the linked "issue" member starts a drag for the family.
        assert!(matches!(
            app.state.drag.as_ref().map(|drag| &drag.target),
            Some(DragTarget::WorkspaceReorder {
                source_ws_idx: 2,
                drop_target: Some(crate::app::state::WorkspaceDropTarget::End),
                ..
            })
        ));
        app.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), 2, target_row));

        let names = app
            .state
            .workspaces
            .iter()
            .map(|ws| ws.display_name())
            .collect::<Vec<_>>();
        assert_eq!(names, vec!["normal", "main", "issue"]);
        app.state.assert_invariants_for_test();
    }

    /// Two loose spaces around a folder holding two members: top level ends
    /// up as `[a, d, work{b, c}]`, workspace vec `[a, d, b, c]`.
    fn app_with_folder() -> (App, String) {
        let mut app = app_for_mouse_test();
        app.state.workspaces = vec![
            Workspace::test_new("a"),
            Workspace::test_new("b"),
            Workspace::test_new("c"),
            Workspace::test_new("d"),
        ];
        app.state.ensure_test_terminals();
        let folder_id = app.state.create_folder("work").expect("create folder");
        for name in ["b", "c"] {
            let id = ws_id(&app, name);
            app.state
                .assign_workspace_to_folder(&id, Some(&folder_id), None)
                .expect("assign member");
        }
        app.state.active = Some(0);
        app.state.selected = 0;
        crate::ui::compute_view(&mut app.state, Rect::new(0, 0, 106, 40));
        (app, folder_id)
    }

    fn ws_id(app: &App, name: &str) -> String {
        app.state
            .workspaces
            .iter()
            .find(|ws| ws.display_name() == name)
            .unwrap_or_else(|| panic!("workspace {name} not found"))
            .id
            .clone()
    }

    fn ws_idx(app: &App, name: &str) -> usize {
        app.state
            .workspaces
            .iter()
            .position(|ws| ws.display_name() == name)
            .unwrap_or_else(|| panic!("workspace {name} not found"))
    }

    fn card_rect(app: &App, name: &str) -> Rect {
        let idx = ws_idx(app, name);
        app.state
            .view
            .workspace_card_areas
            .iter()
            .find(|card| card.ws_idx == idx)
            .unwrap_or_else(|| panic!("no visible card for {name}"))
            .rect
    }

    fn indicator_row(app: &App, target: &crate::app::state::WorkspaceDropTarget) -> u16 {
        crate::ui::workspace_drop_indicator_row(
            &app.state,
            &app.state.view.workspace_card_areas,
            &app.state.view.folder_header_areas,
            app.state.workspace_list_rect(),
            target,
        )
        .expect("drop slot exists")
    }

    fn drag_from_to(app: &mut App, from_row: u16, to_row: u16) {
        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), 2, from_row));
        app.handle_mouse(mouse(MouseEventKind::Drag(MouseButton::Left), 2, to_row));
        app.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), 2, to_row));
    }

    fn folder_events(app: &App) -> Vec<crate::api::schema::EventData> {
        app.event_hub
            .events_after(0)
            .into_iter()
            .map(|(_, event)| event.data)
            .filter(|data| {
                matches!(
                    data,
                    crate::api::schema::EventData::FolderAssigned { .. }
                        | crate::api::schema::EventData::FolderUpdated { .. }
                        | crate::api::schema::EventData::FolderMoved { .. }
                )
            })
            .collect()
    }

    #[test]
    fn dropping_space_between_folder_members_joins_folder_at_that_position() {
        let (mut app, folder_id) = app_with_folder();
        let a_id = ws_id(&app, "a");
        let target_row = indicator_row(
            &app,
            &crate::app::state::WorkspaceDropTarget::InFolderBefore {
                folder_id: folder_id.clone(),
                ws_idx: ws_idx(&app, "c"),
            },
        );

        let source_row = card_rect(&app, "a").y;
        drag_from_to(&mut app, source_row, target_row);

        assert_eq!(
            app.state.folder(&folder_id).unwrap().members,
            [ws_id(&app, "b"), a_id.clone(), ws_id(&app, "c")]
        );
        assert_eq!(
            app.state.workspace_folder_id(&a_id),
            Some(folder_id.as_str())
        );
        assert!(matches!(
            folder_events(&app).as_slice(),
            [crate::api::schema::EventData::FolderAssigned {
                folder_id: Some(_),
                ..
            }]
        ));
        app.state.assert_invariants_for_test();
    }

    #[test]
    fn dropping_space_onto_folder_header_appends_to_that_folder() {
        let (mut app, folder_id) = app_with_folder();
        let d_id = ws_id(&app, "d");
        let header_row = app.state.view.folder_header_areas[0].rect.y;

        app.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            2,
            card_rect(&app, "d").y,
        ));
        app.handle_mouse(mouse(
            MouseEventKind::Drag(MouseButton::Left),
            2,
            header_row,
        ));
        assert!(matches!(
            app.state.drag.as_ref().map(|drag| &drag.target),
            Some(DragTarget::WorkspaceReorder {
                drop_target: Some(crate::app::state::WorkspaceDropTarget::IntoFolder(id)),
                ..
            }) if *id == folder_id
        ));
        app.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), 2, header_row));

        assert_eq!(
            app.state.folder(&folder_id).unwrap().members,
            [ws_id(&app, "b"), ws_id(&app, "c"), d_id.clone()]
        );
        assert_eq!(
            app.state.workspace_folder_id(&d_id),
            Some(folder_id.as_str())
        );
        app.state.assert_invariants_for_test();
    }

    #[test]
    fn dropping_foldered_space_between_top_level_entries_leaves_the_folder() {
        let (mut app, folder_id) = app_with_folder();
        let b_id = ws_id(&app, "b");
        let target_row = indicator_row(
            &app,
            &crate::app::state::WorkspaceDropTarget::Before(ws_idx(&app, "a")),
        );

        let source_row = card_rect(&app, "b").y;
        drag_from_to(&mut app, source_row, target_row);

        assert_eq!(app.state.workspace_folder_id(&b_id), None);
        assert_eq!(
            app.state.folder(&folder_id).unwrap().members,
            [ws_id(&app, "c")]
        );
        assert_eq!(
            app.state
                .workspaces
                .iter()
                .map(|ws| ws.display_name())
                .collect::<Vec<_>>(),
            ["b", "a", "d", "c"]
        );
        assert!(matches!(
            folder_events(&app).as_slice(),
            [crate::api::schema::EventData::FolderAssigned {
                folder_id: None,
                ..
            }]
        ));
        app.state.assert_invariants_for_test();
    }

    #[test]
    fn dropping_space_before_a_folder_header_lands_at_top_level_not_inside() {
        let (mut app, folder_id) = app_with_folder();
        let a_id = ws_id(&app, "a");
        let target_row = indicator_row(
            &app,
            &crate::app::state::WorkspaceDropTarget::BeforeFolder(folder_id.clone()),
        );

        let source_row = card_rect(&app, "a").y;
        drag_from_to(&mut app, source_row, target_row);

        assert_eq!(app.state.workspace_folder_id(&a_id), None);
        assert_eq!(
            app.state
                .workspaces
                .iter()
                .map(|ws| ws.display_name())
                .collect::<Vec<_>>(),
            ["d", "a", "b", "c"]
        );
        app.state.assert_invariants_for_test();
    }

    #[test]
    fn dragging_space_within_its_folder_reorders_members_in_place() {
        let (mut app, folder_id) = app_with_folder();
        // The folder header hugs its first member, so the position-0 slot
        // sits on the first member's own top row.
        let target_row = indicator_row(
            &app,
            &crate::app::state::WorkspaceDropTarget::InFolderBefore {
                folder_id: folder_id.clone(),
                ws_idx: ws_idx(&app, "b"),
            },
        );
        assert_eq!(target_row, card_rect(&app, "b").y);

        let source_row = card_rect(&app, "c").y;
        drag_from_to(&mut app, source_row, target_row);

        assert_eq!(
            app.state.folder(&folder_id).unwrap().members,
            [ws_id(&app, "c"), ws_id(&app, "b")]
        );
        // A same-folder positional drop is a member-order change:
        // `folder.updated`, never `folder.assigned`.
        assert!(matches!(
            folder_events(&app).as_slice(),
            [crate::api::schema::EventData::FolderUpdated { .. }]
        ));
        app.state.assert_invariants_for_test();
    }

    #[test]
    fn dropping_family_member_onto_folder_header_files_the_whole_family() {
        let mut app = app_for_mouse_test();
        app.state.workspaces = vec![
            workspace_with_space("main", "repo-key"),
            workspace_with_space("issue", "repo-key"),
            Workspace::test_new("x"),
        ];
        app.state.ensure_test_terminals();
        let folder_id = app.state.create_folder("work").expect("create folder");
        let x_id = ws_id(&app, "x");
        app.state
            .assign_workspace_to_folder(&x_id, Some(&folder_id), None)
            .expect("assign member");
        app.state.active = Some(0);
        app.state.selected = 0;
        crate::ui::compute_view(&mut app.state, Rect::new(0, 0, 106, 40));
        let header_row = app.state.view.folder_header_areas[0].rect.y;

        // Grab the linked "issue" child, not the parent.
        let source_row = card_rect(&app, "issue").y;
        drag_from_to(&mut app, source_row, header_row);

        assert_eq!(
            app.state.folder(&folder_id).unwrap().members,
            [x_id, ws_id(&app, "main"), ws_id(&app, "issue")]
        );
        assert!(matches!(
            folder_events(&app).as_slice(),
            [crate::api::schema::EventData::FolderAssigned {
                folder_id: Some(_),
                workspace_ids,
            }] if workspace_ids.len() == 2
        ));
        app.state.assert_invariants_for_test();
    }

    #[test]
    fn dropping_family_member_between_folder_members_keeps_the_family_together() {
        let mut app = app_for_mouse_test();
        app.state.workspaces = vec![
            workspace_with_space("main", "repo-key"),
            workspace_with_space("issue", "repo-key"),
            Workspace::test_new("x"),
            Workspace::test_new("y"),
        ];
        app.state.ensure_test_terminals();
        let folder_id = app.state.create_folder("work").expect("create folder");
        for name in ["x", "y"] {
            let id = ws_id(&app, name);
            app.state
                .assign_workspace_to_folder(&id, Some(&folder_id), None)
                .expect("assign member");
        }
        app.state.active = Some(0);
        app.state.selected = 0;
        crate::ui::compute_view(&mut app.state, Rect::new(0, 0, 106, 40));
        let target_row = indicator_row(
            &app,
            &crate::app::state::WorkspaceDropTarget::InFolderBefore {
                folder_id: folder_id.clone(),
                ws_idx: ws_idx(&app, "y"),
            },
        );

        // Grab the linked "issue" child: the whole family lands between the
        // folder's members as one block.
        let source_row = card_rect(&app, "issue").y;
        drag_from_to(&mut app, source_row, target_row);

        assert_eq!(
            app.state.folder(&folder_id).unwrap().members,
            [
                ws_id(&app, "x"),
                ws_id(&app, "main"),
                ws_id(&app, "issue"),
                ws_id(&app, "y"),
            ]
        );
        assert!(matches!(
            folder_events(&app).as_slice(),
            [crate::api::schema::EventData::FolderAssigned {
                folder_id: Some(_),
                workspace_ids,
            }] if workspace_ids.len() == 2
        ));
        app.state.assert_invariants_for_test();
    }

    #[test]
    fn dropping_family_member_on_end_of_folder_slot_appends_whole_family_at_end() {
        let mut app = app_for_mouse_test();
        app.state.workspaces = vec![
            workspace_with_space("main", "repo-key"),
            workspace_with_space("issue", "repo-key"),
            Workspace::test_new("x"),
            Workspace::test_new("y"),
        ];
        app.state.ensure_test_terminals();
        let folder_id = app.state.create_folder("work").expect("create folder");
        for name in ["x", "y"] {
            let id = ws_id(&app, name);
            app.state
                .assign_workspace_to_folder(&id, Some(&folder_id), None)
                .expect("assign member");
        }
        app.state.active = Some(0);
        app.state.selected = 0;
        crate::ui::compute_view(&mut app.state, Rect::new(0, 0, 106, 40));
        let target_row = indicator_row(
            &app,
            &crate::app::state::WorkspaceDropTarget::InFolderEnd {
                folder_id: folder_id.clone(),
            },
        );

        // Grab the linked "issue" child: the whole family lands at the end
        // of the folder as one block.
        let source_row = card_rect(&app, "issue").y;
        drag_from_to(&mut app, source_row, target_row);

        assert_eq!(
            app.state.folder(&folder_id).unwrap().members,
            [
                ws_id(&app, "x"),
                ws_id(&app, "y"),
                ws_id(&app, "main"),
                ws_id(&app, "issue"),
            ]
        );
        assert!(matches!(
            folder_events(&app).as_slice(),
            [crate::api::schema::EventData::FolderAssigned {
                folder_id: Some(_),
                workspace_ids,
            }] if workspace_ids.len() == 2
        ));
        app.state.assert_invariants_for_test();
    }

    #[test]
    fn dropping_space_onto_collapsed_folder_header_still_appends() {
        let (mut app, folder_id) = app_with_folder();
        app.state.collapsed_folder_ids.insert(folder_id.clone());
        crate::ui::compute_view(&mut app.state, Rect::new(0, 0, 106, 40));
        let d_id = ws_id(&app, "d");
        let header_row = app.state.view.folder_header_areas[0].rect.y;

        let source_row = card_rect(&app, "d").y;
        drag_from_to(&mut app, source_row, header_row);

        assert_eq!(
            app.state.folder(&folder_id).unwrap().members,
            [ws_id(&app, "b"), ws_id(&app, "c"), d_id]
        );
        assert!(app.state.collapsed_folder_ids.contains(&folder_id));
        app.state.assert_invariants_for_test();
    }

    #[test]
    fn dragging_folder_header_repositions_folder_at_top_level() {
        let (mut app, folder_id) = app_with_folder();
        let header_row = app.state.view.folder_header_areas[0].rect.y;
        let target_row = indicator_row(
            &app,
            &crate::app::state::WorkspaceDropTarget::Before(ws_idx(&app, "a")),
        );

        app.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            2,
            header_row,
        ));
        app.handle_mouse(mouse(
            MouseEventKind::Drag(MouseButton::Left),
            2,
            target_row,
        ));
        assert!(matches!(
            app.state.drag.as_ref().map(|drag| &drag.target),
            Some(DragTarget::FolderReorder {
                folder_id: dragged,
                drop_target: Some(crate::app::state::WorkspaceDropTarget::Before(_)),
                ..
            }) if *dragged == folder_id
        ));
        app.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), 2, target_row));

        assert!(matches!(
            app.state.space_order.first(),
            Some(crate::folder::SpaceOrderEntry::Folder(folder)) if folder.id == folder_id
        ));
        assert_eq!(
            app.state.folder(&folder_id).unwrap().members,
            [ws_id(&app, "b"), ws_id(&app, "c")]
        );
        assert!(matches!(
            folder_events(&app).as_slice(),
            [crate::api::schema::EventData::FolderMoved { position: 0, .. }]
        ));
        app.state.assert_invariants_for_test();
    }

    #[test]
    fn dragging_collapsed_folder_moves_it_with_its_hidden_contents() {
        let (mut app, folder_id) = app_with_folder();
        app.state.collapsed_folder_ids.insert(folder_id.clone());
        crate::ui::compute_view(&mut app.state, Rect::new(0, 0, 106, 40));
        let header_row = app.state.view.folder_header_areas[0].rect.y;
        let target_row = indicator_row(
            &app,
            &crate::app::state::WorkspaceDropTarget::Before(ws_idx(&app, "a")),
        );

        app.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            2,
            header_row,
        ));
        app.handle_mouse(mouse(
            MouseEventKind::Drag(MouseButton::Left),
            2,
            target_row,
        ));
        app.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), 2, target_row));

        assert!(matches!(
            app.state.space_order.first(),
            Some(crate::folder::SpaceOrderEntry::Folder(folder)) if folder.id == folder_id
        ));
        assert_eq!(
            app.state.folder(&folder_id).unwrap().members,
            [ws_id(&app, "b"), ws_id(&app, "c")]
        );
        assert!(app.state.collapsed_folder_ids.contains(&folder_id));
        app.state.assert_invariants_for_test();
    }

    #[test]
    fn folder_drag_never_targets_the_inside_of_another_folder() {
        let mut app = app_for_mouse_test();
        app.state.workspaces = vec![
            Workspace::test_new("a"),
            Workspace::test_new("b"),
            Workspace::test_new("c"),
        ];
        app.state.ensure_test_terminals();
        let first = app.state.create_folder("first").expect("create folder");
        let second = app.state.create_folder("second").expect("create folder");
        let b_id = ws_id(&app, "b");
        let c_id = ws_id(&app, "c");
        app.state
            .assign_workspace_to_folder(&b_id, Some(&first), None)
            .expect("assign member");
        app.state
            .assign_workspace_to_folder(&c_id, Some(&second), None)
            .expect("assign member");
        app.state.active = Some(0);
        app.state.selected = 0;
        crate::ui::compute_view(&mut app.state, Rect::new(0, 0, 106, 40));

        let first_header_row = app
            .state
            .view
            .folder_header_areas
            .iter()
            .find(|header| header.folder_id == first)
            .unwrap()
            .rect
            .y;
        let second_header_row = app
            .state
            .view
            .folder_header_areas
            .iter()
            .find(|header| header.folder_id == second)
            .unwrap()
            .rect
            .y;

        app.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            2,
            first_header_row,
        ));
        // Hovering the other folder's header and member rows must resolve to
        // top-level slots only: a folder can never enter another folder.
        let c_row = card_rect(&app, "c").y;
        for row in [second_header_row, c_row] {
            app.handle_mouse(mouse(MouseEventKind::Drag(MouseButton::Left), 2, row));
            match app.state.drag.as_ref().map(|drag| &drag.target) {
                Some(DragTarget::FolderReorder {
                    drop_target: Some(target),
                    ..
                }) => assert!(
                    crate::ui::is_top_level_drop_target(target),
                    "folder drag targeted {target:?}"
                ),
                other => panic!("expected folder drag, got {:?}", other.is_some()),
            }
        }
        app.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), 2, c_row));

        assert_eq!(app.state.folder(&first).unwrap().members, [b_id]);
        assert_eq!(app.state.folder(&second).unwrap().members, [c_id]);
        app.state.assert_invariants_for_test();
    }

    #[test]
    fn dropping_space_back_in_place_changes_nothing_and_emits_nothing() {
        let (mut app, folder_id) = app_with_folder();
        let before_order = app.state.space_order.clone();
        let target_row = indicator_row(
            &app,
            &crate::app::state::WorkspaceDropTarget::Before(ws_idx(&app, "a")),
        );

        let source_row = card_rect(&app, "a").y;
        drag_from_to(&mut app, source_row, target_row);

        assert_eq!(app.state.space_order, before_order);
        assert!(folder_events(&app).is_empty());
        assert_eq!(
            app.state.folder(&folder_id).unwrap().members,
            [ws_id(&app, "b"), ws_id(&app, "c")]
        );
        app.state.assert_invariants_for_test();
    }

    #[test]
    fn dropping_folder_back_in_place_changes_nothing_and_emits_nothing() {
        let (mut app, folder_id) = app_with_folder();
        let before_order = app.state.space_order.clone();
        let header_row = app.state.view.folder_header_areas[0].rect.y;
        let target_row = indicator_row(
            &app,
            &crate::app::state::WorkspaceDropTarget::BeforeFolder(folder_id.clone()),
        );

        app.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            2,
            header_row,
        ));
        app.handle_mouse(mouse(
            MouseEventKind::Drag(MouseButton::Left),
            2,
            target_row,
        ));
        app.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), 2, target_row));

        assert_eq!(app.state.space_order, before_order);
        assert!(folder_events(&app).is_empty());
        app.state.assert_invariants_for_test();
    }

    #[test]
    fn dropping_foldered_space_on_gap_below_last_member_appends_at_folder_end() {
        let (mut app, folder_id) = app_with_folder();
        let target_row = indicator_row(
            &app,
            &crate::app::state::WorkspaceDropTarget::InFolderEnd {
                folder_id: folder_id.clone(),
            },
        );
        // The end-of-folder slot sits on the row directly below the last
        // member.
        let last = card_rect(&app, "c");
        assert_eq!(target_row, last.y + last.height);

        let source_row = card_rect(&app, "b").y;
        drag_from_to(&mut app, source_row, target_row);

        assert_eq!(
            app.state.folder(&folder_id).unwrap().members,
            [ws_id(&app, "c"), ws_id(&app, "b")]
        );
        // A same-folder drop to the end is a member-order change.
        assert!(matches!(
            folder_events(&app).as_slice(),
            [crate::api::schema::EventData::FolderUpdated { .. }]
        ));
        app.state.assert_invariants_for_test();
    }

    #[test]
    fn dropping_loose_space_on_gap_below_last_member_joins_folder_at_end() {
        let (mut app, folder_id) = app_with_folder();
        let a_id = ws_id(&app, "a");
        let target_row = indicator_row(
            &app,
            &crate::app::state::WorkspaceDropTarget::InFolderEnd {
                folder_id: folder_id.clone(),
            },
        );

        let source_row = card_rect(&app, "a").y;
        drag_from_to(&mut app, source_row, target_row);

        assert_eq!(
            app.state.folder(&folder_id).unwrap().members,
            [ws_id(&app, "b"), ws_id(&app, "c"), a_id.clone()]
        );
        assert_eq!(
            app.state.workspace_folder_id(&a_id),
            Some(folder_id.as_str())
        );
        assert!(matches!(
            folder_events(&app).as_slice(),
            [crate::api::schema::EventData::FolderAssigned {
                folder_id: Some(_),
                ..
            }]
        ));
        app.state.assert_invariants_for_test();
    }

    #[test]
    fn dropping_one_zone_below_end_of_folder_slot_ejects_to_top_level() {
        let (mut app, folder_id) = app_with_folder();
        let b_id = ws_id(&app, "b");
        let end_of_folder_row = indicator_row(
            &app,
            &crate::app::state::WorkspaceDropTarget::InFolderEnd {
                folder_id: folder_id.clone(),
            },
        );
        // The folder is the last entry in the panel: the trailing top-level
        // slot stays reachable one row below the end-of-folder slot.
        let top_level_row = indicator_row(&app, &crate::app::state::WorkspaceDropTarget::End);
        assert_eq!(top_level_row, end_of_folder_row + 1);

        let source_row = card_rect(&app, "b").y;
        drag_from_to(&mut app, source_row, top_level_row);

        assert_eq!(app.state.workspace_folder_id(&b_id), None);
        assert_eq!(
            app.state.folder(&folder_id).unwrap().members,
            [ws_id(&app, "c")]
        );
        assert_eq!(
            app.state
                .workspaces
                .iter()
                .map(|ws| ws.display_name())
                .collect::<Vec<_>>(),
            ["a", "d", "c", "b"]
        );
        app.state.assert_invariants_for_test();
    }

    #[test]
    fn end_of_folder_slot_reachable_when_folder_is_followed_by_loose_space() {
        let (mut app, folder_id) = app_with_folder();
        // Reposition the folder above the loose spaces: [work{b, c}, a, d].
        app.state.move_folder(&folder_id, 0).expect("move folder");
        crate::ui::compute_view(&mut app.state, Rect::new(0, 0, 106, 40));

        let end_of_folder_row = indicator_row(
            &app,
            &crate::app::state::WorkspaceDropTarget::InFolderEnd {
                folder_id: folder_id.clone(),
            },
        );
        // The following top-level slot keeps a distinguishable row below the
        // end-of-folder slot.
        let before_a_row = indicator_row(
            &app,
            &crate::app::state::WorkspaceDropTarget::Before(ws_idx(&app, "a")),
        );
        assert!(before_a_row > end_of_folder_row);

        let source_row = card_rect(&app, "b").y;
        drag_from_to(&mut app, source_row, end_of_folder_row);

        assert_eq!(
            app.state.folder(&folder_id).unwrap().members,
            [ws_id(&app, "c"), ws_id(&app, "b")]
        );
        assert_eq!(app.state.workspace_folder_id(&ws_id(&app, "a")), None);
        app.state.assert_invariants_for_test();
    }

    #[test]
    fn dragging_last_member_to_end_of_folder_slot_changes_nothing() {
        let (mut app, folder_id) = app_with_folder();
        let before_order = app.state.space_order.clone();
        let target_row = indicator_row(
            &app,
            &crate::app::state::WorkspaceDropTarget::InFolderEnd {
                folder_id: folder_id.clone(),
            },
        );

        let source_row = card_rect(&app, "c").y;
        drag_from_to(&mut app, source_row, target_row);

        assert_eq!(app.state.space_order, before_order);
        assert!(folder_events(&app).is_empty());
        app.state.assert_invariants_for_test();
    }

    #[test]
    fn folder_drag_resolves_end_of_folder_row_to_top_level_slot() {
        let (mut app, folder_id) = app_with_folder();
        let end_of_folder_row = indicator_row(
            &app,
            &crate::app::state::WorkspaceDropTarget::InFolderEnd {
                folder_id: folder_id.clone(),
            },
        );
        let header_row = app.state.view.folder_header_areas[0].rect.y;

        app.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            2,
            header_row,
        ));
        app.handle_mouse(mouse(
            MouseEventKind::Drag(MouseButton::Left),
            2,
            end_of_folder_row,
        ));
        match app.state.drag.as_ref().map(|drag| &drag.target) {
            Some(DragTarget::FolderReorder {
                drop_target: Some(target),
                ..
            }) => assert!(
                crate::ui::is_top_level_drop_target(target),
                "folder drag targeted {target:?}"
            ),
            other => panic!("expected folder drag, got {:?}", other.is_some()),
        }
        app.handle_mouse(mouse(
            MouseEventKind::Up(MouseButton::Left),
            2,
            end_of_folder_row,
        ));

        // The resolution seam also rejects the in-folder target outright.
        assert!(app
            .state
            .folder_drop_move_params(
                &folder_id,
                &crate::app::state::WorkspaceDropTarget::InFolderEnd {
                    folder_id: folder_id.clone(),
                },
            )
            .is_none());
        app.state.assert_invariants_for_test();
    }

    #[test]
    fn gap_before_a_following_folder_header_stays_a_top_level_slot() {
        let mut app = app_for_mouse_test();
        app.state.workspaces = vec![
            Workspace::test_new("a"),
            Workspace::test_new("b"),
            Workspace::test_new("c"),
        ];
        app.state.ensure_test_terminals();
        let first = app.state.create_folder("first").expect("create folder");
        let second = app.state.create_folder("second").expect("create folder");
        let b_id = ws_id(&app, "b");
        let c_id = ws_id(&app, "c");
        app.state
            .assign_workspace_to_folder(&b_id, Some(&first), None)
            .expect("assign member");
        app.state
            .assign_workspace_to_folder(&c_id, Some(&second), None)
            .expect("assign member");
        app.state.active = Some(0);
        app.state.selected = 0;
        crate::ui::compute_view(&mut app.state, Rect::new(0, 0, 106, 40));

        // The single gap between the first folder's last member and the
        // second folder's header keeps its top-level meaning: a space must
        // still be able to land between the folders (appending to the first
        // folder stays available via its header drop).
        assert!(crate::ui::workspace_drop_indicator_row(
            &app.state,
            &app.state.view.workspace_card_areas,
            &app.state.view.folder_header_areas,
            app.state.workspace_list_rect(),
            &crate::app::state::WorkspaceDropTarget::InFolderEnd {
                folder_id: first.clone(),
            },
        )
        .is_none());
        let before_second_row = indicator_row(
            &app,
            &crate::app::state::WorkspaceDropTarget::BeforeFolder(second.clone()),
        );
        assert_eq!(
            app.state.workspace_drop_target_at_row(before_second_row),
            Some(crate::app::state::WorkspaceDropTarget::BeforeFolder(second))
        );
        app.state.assert_invariants_for_test();
    }
}
