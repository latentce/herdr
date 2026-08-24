use ratatui::layout::Rect;

use crate::app::state::{AppState, ViewLayout};

use super::ScrollbarClickTarget;

impl AppState {
    pub(super) fn workspace_list_rect(&self) -> Rect {
        let sidebar = self.view.sidebar_rect;
        if self.sidebar_collapsed || sidebar.width <= 1 || sidebar.height == 0 {
            return Rect::default();
        }
        crate::ui::workspace_list_rect(sidebar, self.sidebar_section_split)
    }

    pub(super) fn agent_panel_rect(&self) -> Rect {
        let sidebar = self.view.sidebar_rect;
        if self.sidebar_collapsed || sidebar.width <= 1 || sidebar.height == 0 {
            return Rect::default();
        }
        let (_, detail_area) =
            crate::ui::expanded_sidebar_sections(sidebar, self.sidebar_section_split);
        detail_area
    }

    pub(super) fn workspace_list_scrollbar_target_at(
        &self,
        col: u16,
        row: u16,
    ) -> Option<ScrollbarClickTarget> {
        let area = self.workspace_list_rect();
        let metrics = crate::ui::workspace_list_scroll_metrics(self, area);
        let track = crate::ui::workspace_list_scrollbar_rect(self, area)?;
        if col < track.x
            || col >= track.x + track.width
            || row < track.y
            || row >= track.y + track.height
        {
            return None;
        }
        if let Some(grab_row_offset) = crate::ui::scrollbar_thumb_grab_offset(metrics, track, row) {
            Some(ScrollbarClickTarget::Thumb { grab_row_offset })
        } else {
            Some(ScrollbarClickTarget::Track {
                offset_from_bottom: crate::ui::scrollbar_offset_from_row(metrics, track, row),
            })
        }
    }

    pub(super) fn workspace_list_offset_for_drag_row(
        &self,
        row: u16,
        grab_row_offset: u16,
    ) -> Option<usize> {
        let area = self.workspace_list_rect();
        let metrics = crate::ui::workspace_list_scroll_metrics(self, area);
        let track = crate::ui::workspace_list_scrollbar_rect(self, area)?;
        Some(crate::ui::scrollbar_offset_from_drag_row(
            metrics,
            track,
            row,
            grab_row_offset,
        ))
    }

    pub(super) fn set_workspace_list_offset_from_bottom(&mut self, offset_from_bottom: usize) {
        let area = self.workspace_list_rect();
        let metrics = crate::ui::workspace_list_scroll_metrics(self, area);
        self.workspace_scroll = metrics
            .max_offset_from_bottom
            .saturating_sub(offset_from_bottom);
        self.workspace_scroll = crate::ui::normalized_workspace_scroll(
            self,
            self.view.sidebar_rect,
            self.workspace_scroll,
        );
    }

    pub(super) fn scroll_workspace_list(&mut self, delta: i16) {
        if delta.is_negative() {
            self.workspace_scroll = self
                .workspace_scroll
                .saturating_sub(delta.unsigned_abs() as usize);
            self.workspace_scroll = crate::ui::normalized_workspace_scroll(
                self,
                self.view.sidebar_rect,
                self.workspace_scroll,
            );
            return;
        }

        let area = self.workspace_list_rect();
        let metrics = crate::ui::workspace_list_scroll_metrics(self, area);
        self.workspace_scroll = self
            .workspace_scroll
            .saturating_add(delta as usize)
            .min(metrics.max_offset_from_bottom);
        self.workspace_scroll = crate::ui::normalized_workspace_scroll(
            self,
            self.view.sidebar_rect,
            self.workspace_scroll,
        );
    }

    pub(super) fn agent_panel_scrollbar_target_at(
        &self,
        col: u16,
        row: u16,
    ) -> Option<ScrollbarClickTarget> {
        let area = self.agent_panel_rect();
        let metrics = crate::ui::agent_panel_scroll_metrics(self, area);
        let track = crate::ui::agent_panel_scrollbar_rect(self, area)?;
        if col < track.x
            || col >= track.x + track.width
            || row < track.y
            || row >= track.y + track.height
        {
            return None;
        }
        if let Some(grab_row_offset) = crate::ui::scrollbar_thumb_grab_offset(metrics, track, row) {
            Some(ScrollbarClickTarget::Thumb { grab_row_offset })
        } else {
            Some(ScrollbarClickTarget::Track {
                offset_from_bottom: crate::ui::scrollbar_offset_from_row(metrics, track, row),
            })
        }
    }

    pub(super) fn agent_panel_offset_for_drag_row(
        &self,
        row: u16,
        grab_row_offset: u16,
    ) -> Option<usize> {
        let area = self.agent_panel_rect();
        let metrics = crate::ui::agent_panel_scroll_metrics(self, area);
        let track = crate::ui::agent_panel_scrollbar_rect(self, area)?;
        Some(crate::ui::scrollbar_offset_from_drag_row(
            metrics,
            track,
            row,
            grab_row_offset,
        ))
    }

    pub(super) fn set_agent_panel_offset_from_bottom(&mut self, offset_from_bottom: usize) {
        let area = self.agent_panel_rect();
        let metrics = crate::ui::agent_panel_scroll_metrics(self, area);
        self.agent_panel_scroll = metrics
            .max_offset_from_bottom
            .saturating_sub(offset_from_bottom);
    }

    pub(super) fn scroll_agent_panel(&mut self, delta: i16) {
        let area = self.agent_panel_rect();
        let max_scroll = crate::ui::agent_panel_scroll_metrics(self, area).max_offset_from_bottom;
        if delta.is_negative() {
            self.agent_panel_scroll = self
                .agent_panel_scroll
                .saturating_sub(delta.unsigned_abs() as usize);
        } else {
            self.agent_panel_scroll = self
                .agent_panel_scroll
                .saturating_add(delta as usize)
                .min(max_scroll);
        }
    }

    pub(crate) fn sidebar_footer_rect(&self) -> Rect {
        let ws_area = self.workspace_list_rect();
        if ws_area == Rect::default() {
            return Rect::default();
        }
        let y = ws_area.y + ws_area.height.saturating_sub(1);
        Rect::new(ws_area.x, y, ws_area.width, 1)
    }

    pub(crate) fn sidebar_new_button_rect(&self) -> Rect {
        let footer = self.sidebar_footer_rect();
        let width = 5u16.min(footer.width.max(1));
        Rect::new(footer.x, footer.y, width, footer.height)
    }

    pub(crate) fn global_launcher_rect(&self) -> Rect {
        if self.view.layout == ViewLayout::Mobile {
            return self.view.mobile_menu_hit_area;
        }

        let footer = self.sidebar_footer_rect();
        let width = if self.global_menu_attention_badge_visible() {
            8
        } else {
            6
        }
        .min(footer.width.max(1));
        let x = footer.x + footer.width.saturating_sub(width);
        Rect::new(x, footer.y, width, footer.height)
    }

    pub(crate) fn global_menu_labels(&self) -> Vec<&'static str> {
        let mut labels = vec!["settings", "keybinds", "reload config"];
        if self.update_available.is_some() {
            labels.push("update ready");
        } else if self.latest_release_notes_available {
            labels.push("what's new");
        }
        labels.push("detach");
        labels
    }

    pub(crate) fn global_menu_rect(&self) -> Rect {
        let screen = self.screen_rect();
        let launcher = self.global_launcher_rect();
        let labels = self.global_menu_labels();
        let content_width = labels
            .iter()
            .map(|label| {
                let badge_width = if self.global_menu_item_has_badge(label) {
                    2
                } else {
                    0
                };
                label.chars().count() as u16 + badge_width
            })
            .max()
            .unwrap_or(8)
            .saturating_add(2);
        let menu_w = content_width.saturating_add(2).min(screen.width.max(1));
        let menu_h = (labels.len() as u16 + 2).min(screen.height.max(1));
        let max_x = screen.x + screen.width.saturating_sub(menu_w);
        let desired_x = launcher.x + launcher.width.saturating_sub(menu_w);
        let x = desired_x.min(max_x);
        let y = launcher.y.saturating_sub(menu_h);
        Rect::new(x, y, menu_w, menu_h)
    }

    pub(super) fn on_sidebar_divider(&self, col: u16, row: u16) -> bool {
        if self.sidebar_collapsed {
            return false;
        }
        let sidebar = self.view.sidebar_rect;
        let toggle = crate::ui::expanded_sidebar_toggle_rect(sidebar);
        let on_toggle = toggle.width > 0
            && col >= toggle.x
            && col < toggle.x + toggle.width
            && row >= toggle.y
            && row < toggle.y + toggle.height;
        sidebar.width > 0
            && !on_toggle
            && col == sidebar.x + sidebar.width.saturating_sub(1)
            && row >= sidebar.y
            && row < sidebar.y + sidebar.height
    }

    pub(super) fn on_sidebar_toggle(&self, col: u16, row: u16) -> bool {
        let rect = if self.sidebar_collapsed {
            crate::ui::collapsed_sidebar_toggle_rect(self.view.sidebar_rect)
        } else {
            crate::ui::expanded_sidebar_toggle_rect(self.view.sidebar_rect)
        };
        rect.width > 0
            && col >= rect.x
            && col < rect.x + rect.width
            && row >= rect.y
            && row < rect.y + rect.height
    }

    pub(super) fn set_manual_sidebar_width(&mut self, divider_col: u16) {
        let sidebar = self.view.sidebar_rect;
        let width = divider_col.saturating_sub(sidebar.x).saturating_add(1);
        self.sidebar_width = width.clamp(self.sidebar_min_width, self.sidebar_max_width);
        self.sidebar_width_source = crate::app::state::SidebarWidthSource::Manual;
        self.mark_session_dirty();
    }

    pub(super) fn on_sidebar_section_divider(&self, col: u16, row: u16) -> bool {
        if self.sidebar_collapsed {
            return false;
        }
        let rect = crate::ui::sidebar_section_divider_rect(
            self.view.sidebar_rect,
            self.sidebar_section_split,
        );
        rect.width > 0
            && col >= rect.x
            && col < rect.x + rect.width
            && row >= rect.y
            && row < rect.y + rect.height
    }

    pub(super) fn set_sidebar_section_split(&mut self, row: u16) {
        let sidebar = self.view.sidebar_rect;
        let content_height = sidebar.height;
        if content_height < 6 {
            return;
        }
        let relative_y = row.saturating_sub(sidebar.y);
        let ratio = (relative_y as f32) / (content_height as f32);
        self.sidebar_section_split = ratio.clamp(0.1, 0.9);
        self.mark_session_dirty();
    }

    pub(super) fn workspace_at_row(&self, row: u16) -> Option<usize> {
        let footer = self.sidebar_footer_rect();
        if footer == Rect::default() {
            return None;
        }

        let cards = if self.view.workspace_card_areas.is_empty() {
            crate::ui::compute_workspace_card_areas(self, self.view.sidebar_rect)
        } else {
            self.view.workspace_card_areas.clone()
        };

        cards.iter().find_map(|card| {
            (row >= card.rect.y && row < card.rect.y + card.rect.height).then_some(card.ws_idx)
        })
    }

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

    pub(super) fn collapsed_workspace_at_row(&self, row: u16) -> Option<usize> {
        if !self.sidebar_collapsed {
            return None;
        }

        let (ws_area, _, _) = crate::ui::collapsed_sidebar_sections(self.view.sidebar_rect);
        if ws_area == Rect::default() || row < ws_area.y || row >= ws_area.y + ws_area.height {
            return None;
        }

        let idx = (row - ws_area.y) as usize;
        (idx < self.workspaces.len()).then_some(idx)
    }

    pub(super) fn collapsed_agent_detail_target_at(
        &self,
        row: u16,
    ) -> Option<(usize, usize, crate::layout::PaneId)> {
        if !self.sidebar_collapsed {
            return None;
        }

        let (_, _, detail_area) = crate::ui::collapsed_sidebar_sections(self.view.sidebar_rect);
        let detail_content_area = Rect::new(
            detail_area.x,
            detail_area.y,
            detail_area.width,
            detail_area.height.saturating_sub(1),
        );
        if detail_content_area == Rect::default()
            || row < detail_content_area.y
            || row >= detail_content_area.y + detail_content_area.height
        {
            return None;
        }

        let detail_idx = (row - detail_content_area.y) as usize;
        let details = crate::ui::agent_panel_entries(self);
        let detail = details.get(detail_idx)?;
        Some((detail.ws_idx, detail.tab_idx, detail.pane_id))
    }

    pub(super) fn workspace_drop_target_at_row(
        &self,
        row: u16,
    ) -> Option<crate::app::state::WorkspaceDropTarget> {
        // Dropping a space onto a folder header appends it to that folder.
        if self.workspace_list_row_in_range(row) {
            if let Some(folder_id) = self.folder_header_at(row) {
                return Some(crate::app::state::WorkspaceDropTarget::IntoFolder(
                    folder_id,
                ));
            }
        }
        self.nearest_drop_slot_at_row(row, false)
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

    fn workspace_list_row_in_range(&self, row: u16) -> bool {
        let area = self.workspace_list_rect();
        let footer = self.sidebar_footer_rect();
        area != Rect::default() && row >= area.y && row < footer.y
    }

    fn nearest_drop_slot_at_row(
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
            WorkspaceDropTarget::IntoFolder(folder_id) => (Some(folder_id.clone()), None),
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
            WorkspaceDropTarget::InFolderBefore { .. } | WorkspaceDropTarget::IntoFolder(_) => {
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

    pub(super) fn on_agent_panel_sort_toggle(&self, col: u16, row: u16) -> bool {
        if self.sidebar_collapsed || self.agent_view_override.is_some() {
            return false;
        }

        let (_, detail_area) = crate::ui::expanded_sidebar_sections(
            self.view.sidebar_rect,
            self.sidebar_section_split,
        );
        let rect = crate::ui::agent_panel_toggle_rect(detail_area, self.agent_panel_sort);
        rect.width > 0
            && col >= rect.x
            && col < rect.x + rect.width
            && row >= rect.y
            && row < rect.y + rect.height
    }

    pub(super) fn agent_detail_target_at(
        &self,
        row: u16,
    ) -> Option<(usize, usize, crate::layout::PaneId)> {
        match self.agent_panel_row_hit(row)?.0 {
            crate::ui::AgentPanelListEntry::Agent { entry_idx } => {
                crate::ui::agent_panel_entries(self)
                    .get(entry_idx)
                    .map(|detail| (detail.ws_idx, detail.tab_idx, detail.pane_id))
            }
            _ => None,
        }
    }

    /// The collapse chevron hit (if any) at this cell of the agents panel:
    /// a folder header's chevron toggles the shared folder collapse, a space
    /// header's chevron toggles that space's agent-list collapse. Thin
    /// ancestor headers have no agents of their own and expose no chevron.
    pub(super) fn agent_panel_collapse_target_at(
        &self,
        col: u16,
        row: u16,
    ) -> Option<AgentPanelCollapseTarget> {
        let (list_row, row_y, body) = self.agent_panel_row_hit(row)?;
        // The chevron leads the header row after its indent prefix; the
        // indent depends on the header kind, so resolve it per row.
        let (target, indent) = match &list_row {
            crate::ui::AgentPanelListEntry::FolderHeader { order_idx } => {
                match self.space_order.get(*order_idx) {
                    Some(crate::folder::SpaceOrderEntry::Folder(folder)) => (
                        AgentPanelCollapseTarget::Folder(folder.id.clone()),
                        crate::ui::AGENT_PANEL_HEADER_GUTTER,
                    ),
                    _ => return None,
                }
            }
            crate::ui::AgentPanelListEntry::SpaceHeader {
                ws_idx,
                indented,
                foldered,
                thin: false,
            } => {
                let ws = self.workspaces.get(*ws_idx)?;
                (
                    AgentPanelCollapseTarget::Space(ws.id.clone()),
                    crate::ui::space_header_chevron_indent(*indented, *foldered),
                )
            }
            _ => return None,
        };
        let chevron = crate::ui::agent_panel_header_chevron_rect(body, row_y, indent);
        if chevron.width == 0 || row != chevron.y || col != chevron.x {
            return None;
        }
        Some(target)
    }

    /// The agents-panel display row under `row`, with the row's top y and the
    /// panel body rect. Shared row-walk for agent and chevron hit-testing.
    fn agent_panel_row_hit(&self, row: u16) -> Option<(crate::ui::AgentPanelListEntry, u16, Rect)> {
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

/// A collapse chevron hit in the agents panel folder view.
pub(super) enum AgentPanelCollapseTarget {
    /// A folder header's chevron: toggles the shared folder collapse.
    Folder(String),
    /// A space header's chevron: toggles that space's agent-list collapse.
    Space(String),
}

#[cfg(test)]
mod tests {
    use std::fs;

    use crossterm::event::{MouseButton, MouseEventKind};
    use ratatui::layout::Rect;

    use super::super::{app_for_mouse_test, capture_snapshot, mouse, unique_temp_path};
    use crate::{
        app::state::{AgentPanelSort, DragTarget, Mode},
        app::App,
        config::SidebarCollapsedModeConfig,
        detect::{Agent, AgentState},
        workspace::Workspace,
    };

    #[test]
    fn clicking_launcher_opens_global_menu() {
        let mut app = app_for_mouse_test();
        let rect = app.state.global_launcher_rect();

        app.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            rect.x + rect.width.saturating_sub(1),
            rect.y,
        ));

        assert_eq!(app.state.mode, Mode::GlobalMenu);
    }

    #[test]
    fn hovering_global_menu_updates_highlight() {
        let mut app = app_for_mouse_test();
        let launcher = app.state.global_launcher_rect();
        app.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            launcher.x,
            launcher.y,
        ));

        let menu = app.state.global_menu_rect();
        app.handle_mouse(mouse(MouseEventKind::Moved, menu.x + 2, menu.y + 2));

        assert_eq!(app.state.global_menu.highlighted, 1);
    }

    #[test]
    fn clicking_keybinds_menu_item_opens_help() {
        let mut app = app_for_mouse_test();
        let launcher = app.state.global_launcher_rect();
        app.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            launcher.x,
            launcher.y,
        ));

        let menu = app.state.global_menu_rect();
        app.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            menu.x + 2,
            menu.y + 2,
        ));

        assert_eq!(app.state.mode, Mode::KeybindHelp);
    }

    #[test]
    fn clicking_settings_menu_item_opens_settings() {
        let mut app = app_for_mouse_test();
        let launcher = app.state.global_launcher_rect();
        app.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            launcher.x,
            launcher.y,
        ));

        let menu = app.state.global_menu_rect();
        app.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            menu.x + 2,
            menu.y + 1,
        ));

        assert_eq!(app.state.mode, Mode::Settings);
    }

    #[test]
    fn clicking_reload_config_menu_item_requests_reload() {
        let mut app = app_for_mouse_test();
        let launcher = app.state.global_launcher_rect();
        app.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            launcher.x,
            launcher.y,
        ));

        let menu = app.state.global_menu_rect();
        app.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            menu.x + 2,
            menu.y + 3,
        ));

        assert!(app.state.request_reload_config);
        assert_eq!(app.state.mode, Mode::Navigate);
    }

    #[test]
    fn update_pending_menu_surfaces_update_ready_entry() {
        let mut app = app_for_mouse_test();
        app.state.update_available = Some("0.3.2".into());
        app.state.latest_release_notes_available = true;

        let launcher = app.state.global_launcher_rect();
        app.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            launcher.x,
            launcher.y,
        ));

        assert_eq!(
            app.state.global_menu_labels(),
            vec![
                "settings",
                "keybinds",
                "reload config",
                "update ready",
                "detach"
            ]
        );
        assert!(!app.state.should_quit);
    }

    #[test]
    fn persistence_mode_menu_surfaces_detach_action() {
        let mut app = app_for_mouse_test();
        app.state.detach_exits = false;

        let launcher = app.state.global_launcher_rect();
        app.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            launcher.x,
            launcher.y,
        ));

        assert_eq!(
            app.state.global_menu_labels(),
            vec!["settings", "keybinds", "reload config", "detach"]
        );

        let menu = app.state.global_menu_rect();
        app.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            menu.x + 2,
            menu.y + 4,
        ));

        assert!(app.state.detach_requested);
        assert!(!app.state.should_quit);
        assert_ne!(app.state.mode, Mode::GlobalMenu);
    }

    #[test]
    fn whats_new_remains_in_menu_for_latest_installed_release_notes() {
        let mut app = app_for_mouse_test();
        app.state.latest_release_notes_available = true;

        assert_eq!(
            app.state.global_menu_labels(),
            vec![
                "settings",
                "keybinds",
                "reload config",
                "what's new",
                "detach"
            ]
        );
    }

    #[test]
    fn clicking_agent_detail_row_switches_to_correct_tab_and_pane() {
        let mut app = app_for_mouse_test();
        let mut ws = Workspace::test_new("test");
        ws.tabs[0].set_custom_name("main".into());
        let first_pane = ws.tabs[0].root_pane;
        let first_tab = ws.test_add_tab(Some("logs"));
        let second_pane = ws.tabs[first_tab].root_pane;
        app.state.workspaces = vec![ws];
        app.state.ensure_test_terminals();
        let first_terminal_id = app.state.workspaces[0].tabs[0].panes[&first_pane]
            .attached_terminal_id
            .clone();
        app.state
            .terminals
            .get_mut(&first_terminal_id)
            .unwrap()
            .detected_agent = Some(Agent::Pi);
        let second_terminal_id = app.state.workspaces[0].tabs[first_tab].panes[&second_pane]
            .attached_terminal_id
            .clone();
        app.state
            .terminals
            .get_mut(&second_terminal_id)
            .unwrap()
            .detected_agent = Some(Agent::Claude);
        app.state.active = Some(0);
        app.state.selected = 0;
        app.state.mode = Mode::Terminal;

        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), 2, 16));

        assert_eq!(app.state.workspaces[0].active_tab, 1);
        assert_eq!(
            app.state.workspaces[0].tabs[1].layout.focused(),
            second_pane
        );
        assert_eq!(app.state.mode, Mode::Terminal);
        let snapshot = capture_snapshot(&app.state);
        assert_eq!(snapshot.workspaces[0].active_tab, first_tab);
        assert_eq!(
            snapshot.workspaces[0].tabs[first_tab].focused,
            Some(second_pane.raw())
        );
    }

    #[test]
    fn per_agent_row_heights_preserve_card_gaps_and_trailing_mouse_targets() {
        let mut app = app_for_mouse_test();
        let first = Workspace::test_new("one");
        let first_pane = first.tabs[0].root_pane;
        let second = Workspace::test_new("two");
        let second_pane = second.tabs[0].root_pane;
        app.state.workspaces = vec![first, second];
        app.state.ensure_test_terminals();
        for (ws_idx, pane_id, agent) in
            [(0, first_pane, Agent::Pi), (1, second_pane, Agent::Claude)]
        {
            let terminal_id = app.state.workspaces[ws_idx].tabs[0].panes[&pane_id]
                .attached_terminal_id
                .clone();
            app.state
                .terminals
                .get_mut(&terminal_id)
                .unwrap()
                .detected_agent = Some(agent);
        }
        app.state.sidebar_agents.rows = vec![vec![crate::config::AgentSidebarToken::Agent]];
        app.state.sidebar_agents.rows_by_agent.insert(
            "claude".into(),
            vec![
                vec![crate::config::AgentSidebarToken::Agent],
                vec![crate::config::AgentSidebarToken::Workspace],
            ],
        );
        app.state.sidebar_agents.row_gap = 1;
        let detail_area = app.state.agent_panel_rect();
        let metrics = crate::ui::agent_panel_scroll_metrics(&app.state, detail_area);
        let body = crate::ui::agent_panel_body_rect(
            detail_area,
            crate::ui::should_show_scrollbar(metrics),
        );

        assert_eq!(
            app.state.agent_detail_target_at(body.y),
            Some((0, 0, first_pane))
        );
        assert_eq!(app.state.agent_detail_target_at(body.y + 1), None);
        assert_eq!(
            app.state.agent_detail_target_at(body.y + 3),
            Some((1, 0, second_pane))
        );

        app.state.sidebar_agents.row_gap = 0;
        assert_eq!(
            app.state.agent_detail_target_at(body.y + 1),
            Some((1, 0, second_pane))
        );
    }

    #[test]
    fn agent_hit_testing_clamps_scroll_after_dynamic_filter_shrink() {
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
        app.state.agent_view_override = Some(crate::api::schema::AgentViewSetParams {
            source: "example.views".to_string(),
            label: None,
            filter: Some(crate::api::schema::AgentViewFilter::Eq {
                field: crate::api::schema::AgentViewField::Builtin(
                    crate::api::schema::AgentViewBuiltinField::WorkspaceId,
                ),
                value: crate::api::schema::AgentViewValue::Context {
                    context: crate::api::schema::AgentViewContext::CurrentWorkspaceId,
                },
            }),
            sort: Vec::new(),
        });
        app.state.agent_panel_scroll = 10;
        let detail_area = app.state.agent_panel_rect();
        let body = crate::ui::agent_panel_body_rect(detail_area, false);

        assert_eq!(
            app.state.agent_detail_target_at(body.y),
            Some((0, 0, first_pane))
        );
    }

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
    fn clicking_all_workspaces_agent_row_switches_to_correct_workspace() {
        let mut app = app_for_mouse_test();
        let first = Workspace::test_new("one");
        let first_pane = first.tabs[0].root_pane;

        let second = Workspace::test_new("two");
        let second_pane = second.tabs[0].root_pane;

        app.state.workspaces = vec![first, second];
        app.state.ensure_test_terminals();
        let first_terminal_id = app.state.workspaces[0].tabs[0].panes[&first_pane]
            .attached_terminal_id
            .clone();
        app.state
            .terminals
            .get_mut(&first_terminal_id)
            .unwrap()
            .detected_agent = Some(Agent::Pi);
        let second_terminal_id = app.state.workspaces[1].tabs[0].panes[&second_pane]
            .attached_terminal_id
            .clone();
        app.state
            .terminals
            .get_mut(&second_terminal_id)
            .unwrap()
            .detected_agent = Some(Agent::Claude);
        app.state.active = Some(0);
        app.state.selected = 0;
        app.state.mode = Mode::Terminal;

        let (_, detail_area) = crate::ui::expanded_sidebar_sections(
            app.state.view.sidebar_rect,
            app.state.sidebar_section_split,
        );
        app.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            detail_area.x + 2,
            detail_area.y + 6,
        ));

        assert_eq!(app.state.active, Some(1));
        assert_eq!(app.state.selected, 1);
        assert_eq!(app.state.workspaces[1].active_tab, 0);
        assert_eq!(
            app.state.workspaces[1].tabs[0].layout.focused(),
            second_pane
        );
    }

    #[test]
    fn scrolling_agent_panel_with_wheel_updates_agent_panel_scroll() {
        let mut app = app_for_mouse_test();
        let mut ws = Workspace::test_new("test");
        let first_pane = ws.tabs[0].root_pane;

        let mut tabs = Vec::new();
        for (tab_name, agent) in [
            ("logs", Agent::Claude),
            ("review", Agent::Codex),
            ("ops", Agent::Gemini),
        ] {
            let tab_idx = ws.test_add_tab(Some(tab_name));
            let pane_id = ws.tabs[tab_idx].root_pane;
            tabs.push((tab_idx, pane_id, agent));
        }

        app.state.workspaces = vec![ws];
        app.state.ensure_test_terminals();
        let first_terminal_id = app.state.workspaces[0].tabs[0].panes[&first_pane]
            .attached_terminal_id
            .clone();
        app.state
            .terminals
            .get_mut(&first_terminal_id)
            .unwrap()
            .detected_agent = Some(Agent::Pi);
        for (tab_idx, pane_id, agent) in tabs {
            let terminal_id = app.state.workspaces[0].tabs[tab_idx].panes[&pane_id]
                .attached_terminal_id
                .clone();
            app.state
                .terminals
                .get_mut(&terminal_id)
                .unwrap()
                .detected_agent = Some(agent);
        }
        app.state.active = Some(0);
        app.state.selected = 0;
        app.state.mode = Mode::Terminal;

        let detail_area = app.state.agent_panel_rect();
        assert!(crate::ui::should_show_scrollbar(
            crate::ui::agent_panel_scroll_metrics(&app.state, detail_area)
        ));

        app.handle_mouse(mouse(
            MouseEventKind::ScrollDown,
            detail_area.x + 1,
            detail_area.y + 4,
        ));

        assert_eq!(app.state.agent_panel_scroll, 1);
        assert_eq!(app.state.selected, 0);
    }

    #[test]
    fn clicking_scrolled_agent_detail_row_switches_to_correct_tab_and_pane() {
        let mut app = app_for_mouse_test();
        let mut ws = Workspace::test_new("test");
        let first_pane = ws.tabs[0].root_pane;
        let second_tab = ws.test_add_tab(Some("logs"));
        let second_pane = ws.tabs[second_tab].root_pane;
        let mut extra_tabs = Vec::new();
        for (tab_name, agent) in [("review", Agent::Codex), ("ops", Agent::Gemini)] {
            let tab_idx = ws.test_add_tab(Some(tab_name));
            let pane_id = ws.tabs[tab_idx].root_pane;
            extra_tabs.push((tab_idx, pane_id, agent));
        }

        app.state.workspaces = vec![ws];
        app.state.ensure_test_terminals();
        let first_terminal_id = app.state.workspaces[0].tabs[0].panes[&first_pane]
            .attached_terminal_id
            .clone();
        app.state
            .terminals
            .get_mut(&first_terminal_id)
            .unwrap()
            .detected_agent = Some(Agent::Pi);
        let second_terminal_id = app.state.workspaces[0].tabs[second_tab].panes[&second_pane]
            .attached_terminal_id
            .clone();
        app.state
            .terminals
            .get_mut(&second_terminal_id)
            .unwrap()
            .detected_agent = Some(Agent::Claude);
        for (tab_idx, pane_id, agent) in extra_tabs {
            let terminal_id = app.state.workspaces[0].tabs[tab_idx].panes[&pane_id]
                .attached_terminal_id
                .clone();
            app.state
                .terminals
                .get_mut(&terminal_id)
                .unwrap()
                .detected_agent = Some(agent);
        }
        app.state.active = Some(0);
        app.state.selected = 0;
        app.state.mode = Mode::Terminal;
        app.state.sidebar_agents.rows = vec![vec![crate::config::AgentSidebarToken::Agent]];
        app.state.sidebar_agents.rows_by_agent.insert(
            "claude".into(),
            vec![
                vec![crate::config::AgentSidebarToken::Agent],
                vec![crate::config::AgentSidebarToken::Workspace],
            ],
        );
        app.state.agent_panel_scroll = 1;

        let detail_area = app.state.agent_panel_rect();
        let body = crate::ui::agent_panel_body_rect(detail_area, true);
        app.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            body.x + 1,
            body.y + 1,
        ));

        assert_eq!(app.state.workspaces[0].active_tab, second_tab);
        assert_eq!(
            app.state.workspaces[0].tabs[second_tab].layout.focused(),
            second_pane
        );
        assert_eq!(app.state.mode, Mode::Terminal);
    }

    #[test]
    fn clicking_collapsed_agent_row_switches_to_correct_tab_and_pane() {
        let mut app = app_for_mouse_test();
        let mut ws = Workspace::test_new("test");
        let first_pane = ws.tabs[0].root_pane;
        let second_tab = ws.test_add_tab(Some("logs"));
        let second_pane = ws.tabs[second_tab].root_pane;
        app.state.workspaces = vec![ws];
        app.state.ensure_test_terminals();
        let first_terminal_id = app.state.workspaces[0].tabs[0].panes[&first_pane]
            .attached_terminal_id
            .clone();
        app.state
            .terminals
            .get_mut(&first_terminal_id)
            .unwrap()
            .detected_agent = Some(Agent::Pi);
        let second_terminal_id = app.state.workspaces[0].tabs[second_tab].panes[&second_pane]
            .attached_terminal_id
            .clone();
        app.state
            .terminals
            .get_mut(&second_terminal_id)
            .unwrap()
            .detected_agent = Some(Agent::Claude);
        app.state.active = Some(0);
        app.state.selected = 0;
        app.state.mode = Mode::Terminal;
        app.state.sidebar_collapsed = true;
        app.state.view.sidebar_rect = Rect::new(0, 0, 4, 20);
        app.state.view.terminal_area = Rect::new(4, 0, 80, 20);

        let (_, _, detail_area) =
            crate::ui::collapsed_sidebar_sections(app.state.view.sidebar_rect);
        app.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            detail_area.x,
            detail_area.y + 1,
        ));

        assert_eq!(app.state.workspaces[0].active_tab, 1);
        assert_eq!(
            app.state.workspaces[0].tabs[1].layout.focused(),
            second_pane
        );
        assert_eq!(app.state.mode, Mode::Terminal);
    }

    #[test]
    fn clicking_collapsed_priority_agent_row_switches_to_matching_workspace() {
        let mut app = app_for_mouse_test();
        let first = Workspace::test_new("one");
        let first_pane = first.tabs[0].root_pane;
        let second = Workspace::test_new("two");
        let second_pane = second.tabs[0].root_pane;

        app.state.workspaces = vec![first, second];
        app.state.ensure_test_terminals();
        app.state.active = Some(0);
        app.state.selected = 0;
        app.state.mode = Mode::Terminal;
        app.state.sidebar_collapsed = true;
        app.state.agent_panel_sort = AgentPanelSort::Priority;
        app.state.view.sidebar_rect = Rect::new(0, 0, 4, 20);
        app.state.view.terminal_area = Rect::new(4, 0, 80, 20);

        let set_state = |app: &mut crate::app::App, ws_idx: usize, pane_id, state| {
            let terminal_id = app.state.workspaces[ws_idx].tabs[0].panes[&pane_id]
                .attached_terminal_id
                .clone();
            let terminal = app.state.terminals.get_mut(&terminal_id).unwrap();
            terminal.detected_agent = Some(Agent::Claude);
            terminal.state = state;
        };
        set_state(&mut app, 0, first_pane, AgentState::Working);
        set_state(&mut app, 1, second_pane, AgentState::Blocked);

        let (_, _, detail_area) =
            crate::ui::collapsed_sidebar_sections(app.state.view.sidebar_rect);
        app.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            detail_area.x,
            detail_area.y,
        ));

        assert_eq!(app.state.active, Some(1));
        assert_eq!(app.state.selected, 1);
        assert_eq!(
            app.state.workspaces[1].tabs[0].layout.focused(),
            second_pane
        );
    }

    #[test]
    fn clicking_collapsed_sidebar_toggle_expands_sidebar() {
        let mut app = app_for_mouse_test();
        app.state.sidebar_collapsed = true;
        app.state.view.sidebar_rect = Rect::new(0, 0, 4, 20);
        app.state.view.terminal_area = Rect::new(4, 0, 80, 20);

        let toggle = crate::ui::collapsed_sidebar_toggle_rect(app.state.view.sidebar_rect);
        app.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            toggle.x,
            toggle.y,
        ));

        assert!(!app.state.sidebar_collapsed);
    }

    #[test]
    fn hidden_collapsed_sidebar_has_no_mouse_expand_hotspot() {
        let mut app = app_for_mouse_test();
        app.state.sidebar_collapsed = true;
        app.state.sidebar_collapsed_mode = SidebarCollapsedModeConfig::Hidden;
        app.state.view.sidebar_rect = Rect::new(0, 0, 0, 20);
        app.state.view.terminal_area = Rect::new(0, 0, 80, 20);

        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), 0, 19));

        assert!(app.state.sidebar_collapsed);
    }

    #[test]
    fn clicking_expanded_sidebar_toggle_collapses_sidebar() {
        let mut app = app_for_mouse_test();
        app.state.sidebar_collapsed = false;
        app.state.view.sidebar_rect = Rect::new(0, 0, 26, 20);
        app.state.view.terminal_area = Rect::new(26, 0, 80, 20);

        let toggle = crate::ui::expanded_sidebar_toggle_rect(app.state.view.sidebar_rect);
        app.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            toggle.x,
            toggle.y,
        ));

        assert!(app.state.sidebar_collapsed);
        assert!(app.state.drag.is_none());
    }

    #[test]
    fn clicking_workspace_switches_on_mouse_up() {
        let mut app = app_for_mouse_test();
        app.state.workspaces = vec![Workspace::test_new("a"), Workspace::test_new("b")];
        app.state.active = Some(0);
        app.state.selected = 0;
        crate::ui::compute_view(&mut app.state, Rect::new(0, 0, 106, 20));
        let target_row = app.state.view.workspace_card_areas[1].rect.y;

        app.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            2,
            target_row,
        ));
        assert_eq!(app.state.active, Some(0));
        assert_eq!(app.state.workspace_presses.len(), 1);

        app.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), 2, target_row));
        assert_eq!(app.state.active, Some(1));
        assert_eq!(app.state.selected, 1);
        assert!(app.state.workspace_presses.is_empty());
        let snapshot = capture_snapshot(&app.state);
        assert_eq!(snapshot.active, Some(1));
        assert_eq!(snapshot.selected, 1);
    }

    #[test]
    fn clicking_worktree_parent_row_focuses_workspace_without_toggling() {
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
        app.state.active = None;
        app.state.mode = Mode::Terminal;
        crate::ui::compute_view(&mut app.state, Rect::new(0, 0, 106, 20));
        let parent = app.state.view.workspace_card_areas[0].rect;

        app.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            parent.x + 2,
            parent.y,
        ));
        app.handle_mouse(mouse(
            MouseEventKind::Up(MouseButton::Left),
            parent.x + 2,
            parent.y,
        ));

        assert_eq!(app.state.active, Some(0));
        assert!(!app.state.collapsed_space_keys.contains("repo-key"));
    }

    #[test]
    fn clicking_worktree_parent_chevron_toggles_group_only() {
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
        app.state.active = None;
        app.state.mode = Mode::Terminal;
        crate::ui::compute_view(&mut app.state, Rect::new(0, 0, 106, 20));
        let parent = app.state.view.workspace_card_areas[0];
        let chevron = crate::ui::workspace_group_chevron_rect(&parent);

        app.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            chevron.x,
            chevron.y,
        ));

        assert_eq!(app.state.active, None);
        assert!(app.state.workspace_presses.is_empty());
        assert!(app.state.collapsed_space_keys.contains("repo-key"));

        app.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            chevron.x,
            chevron.y,
        ));

        assert!(!app.state.collapsed_space_keys.contains("repo-key"));
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
    fn clicking_folder_header_row_does_not_toggle_collapse() {
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

        app.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            header.rect.x + 2,
            header.rect.y,
        ));

        assert!(!app.state.collapsed_folder_ids.contains(&folder_id));
        assert!(app.state.workspace_presses.is_empty());
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
    fn clicking_agents_panel_header_row_off_chevron_does_not_toggle() {
        let (mut app, folder_id) = folder_view_collapse_mouse_app();
        let two_id = app.state.workspaces[1].id.clone();
        let body = agent_panel_body(&app);

        // The gap cell right after each header's chevron: folder header
        // chevron at x+1, foldered space header chevron at x+3.
        for (row, col) in [(body.y + 2, body.x + 2), (body.y + 3, body.x + 4)] {
            app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), col, row));
        }

        assert!(!app.state.collapsed_folder_ids.contains(&folder_id));
        assert!(!app.state.collapsed_agent_space_ids.contains(&two_id));
    }

    #[test]
    fn thin_ancestor_header_has_no_collapse_chevron() {
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
        // gutter).
        let chevron_col = body.x + 1;

        // Rows: thin header(main), header(issue), agent.
        app.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            chevron_col,
            body.y,
        ));

        assert!(
            app.state.collapsed_agent_space_ids.is_empty(),
            "a thin ancestor header exposes no agent-list collapse"
        );
    }

    #[test]
    fn wheel_workspace_selection_follows_grouped_visual_order_without_scrollbar() {
        let mut app = app_for_mouse_test();
        app.state.workspaces = vec![
            Workspace::test_new("main"),
            Workspace::test_new("normal"),
            Workspace::test_new("issue"),
        ];
        for (idx, checkout_path) in [(0, "/repo/herdr"), (2, "/repo/herdr-issue")] {
            app.state.workspaces[idx].worktree_space =
                Some(crate::workspace::WorktreeSpaceMembership {
                    key: "repo-key".into(),
                    label: "herdr".into(),
                    repo_root: "/repo/herdr".into(),
                    checkout_path: checkout_path.into(),
                    is_linked_worktree: idx != 0,
                });
        }
        app.state.active = Some(0);
        app.state.selected = 0;
        app.state.mode = Mode::Navigate;
        crate::ui::compute_view(&mut app.state, Rect::new(0, 0, 106, 30));
        let list = app.state.workspace_list_rect();
        assert!(!crate::ui::should_show_scrollbar(
            crate::ui::workspace_list_scroll_metrics(&app.state, list)
        ));

        app.handle_mouse(mouse(MouseEventKind::ScrollDown, list.x + 1, list.y + 1));

        assert_eq!(app.state.selected, 2);
    }

    #[test]
    fn dragging_workspace_reorders_without_changing_identity() {
        let mut app = app_for_mouse_test();
        app.state.workspaces = vec![
            Workspace::test_new("a"),
            Workspace::test_new("b"),
            Workspace::test_new("c"),
        ];
        app.state.ensure_test_terminals();
        app.state.sidebar_spaces.rows = vec![vec![crate::config::SpaceSidebarToken::Workspace]];
        app.state.sidebar_spaces.row_gap = 0;
        let active_id = app.state.workspaces[1].id.clone();
        let selected_id = app.state.workspaces[2].id.clone();
        app.state.active = Some(1);
        app.state.selected = 2;
        crate::ui::compute_view(&mut app.state, Rect::new(0, 0, 106, 20));
        let packed_boundary_row = app.state.view.workspace_card_areas[1].rect.y;
        assert_eq!(
            app.state.workspace_drop_target_at_row(packed_boundary_row),
            Some(crate::app::state::WorkspaceDropTarget::Before(2))
        );

        let source_row = app.state.view.workspace_card_areas[1].rect.y;
        let target_row = crate::ui::workspace_drop_indicator_row(
            &app.state,
            &app.state.view.workspace_card_areas,
            &app.state.view.folder_header_areas,
            app.state.workspace_list_rect(),
            &crate::app::state::WorkspaceDropTarget::Before(0),
        )
        .unwrap();

        app.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            2,
            source_row,
        ));
        app.handle_mouse(mouse(
            MouseEventKind::Drag(MouseButton::Left),
            2,
            target_row,
        ));
        assert!(matches!(
            app.state.drag.as_ref().map(|drag| &drag.target),
            Some(DragTarget::WorkspaceReorder {
                source_ws_idx: 1,
                drop_target: Some(crate::app::state::WorkspaceDropTarget::Before(0)),
                ..
            })
        ));
        app.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), 2, target_row));

        let names: Vec<_> = app
            .state
            .workspaces
            .iter()
            .map(|ws| ws.display_name())
            .collect();
        assert_eq!(names, vec!["b", "a", "c"]);
        assert_eq!(app.state.active, Some(0));
        assert_eq!(app.state.selected, 2);
        assert_eq!(app.state.workspaces[0].id, active_id);
        assert_eq!(app.state.workspaces[2].id, selected_id);
        let events = app.event_hub.events_after(0);
        // Space drops resolve membership and position through the positional
        // assign path, so a plain top-level reorder emits `folder.assigned`.
        assert!(events.iter().any(|(_, event)| matches!(
            event.data,
            crate::api::schema::EventData::FolderAssigned {
                folder_id: None,
                ..
            }
        )));
        assert!(!events.iter().any(|(_, event)| matches!(
            event.data,
            crate::api::schema::EventData::WorkspaceReordered { .. }
        )));
        app.state.assert_invariants_for_test();
        let snapshot = capture_snapshot(&app.state);
        let captured_names: Vec<_> = snapshot
            .workspaces
            .iter()
            .map(|ws| ws.custom_name.clone().unwrap())
            .collect();
        assert_eq!(captured_names, vec!["b", "a", "c"]);
    }

    #[test]
    fn clicking_tab_scroll_button_reveals_hidden_tabs_without_renaming() {
        let mut app = app_for_mouse_test();
        let mut ws = Workspace::test_new("test");
        ws.test_add_tab(Some("logs"));
        ws.test_add_tab(Some("review"));
        ws.test_add_tab(Some("ops"));
        ws.test_add_tab(Some("notes"));
        app.state.workspaces = vec![ws];
        app.state.active = Some(0);
        app.state.selected = 0;
        crate::ui::compute_view(&mut app.state, Rect::new(0, 0, 65, 20));

        let right = app.state.view.tab_scroll_right_hit_area;
        assert!(right.width > 0);

        app.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            right.x + 1,
            right.y,
        ));

        assert_eq!(app.state.tab_scroll, 1);
        assert!(!app.state.tab_scroll_follow_active);
        assert_eq!(app.state.workspaces[0].active_tab, 0);
        assert_eq!(app.state.view.tab_hit_areas[0].width, 0);
        assert!(app.state.workspaces[0].tabs[0].custom_name.is_none());
        assert_eq!(
            app.state.workspaces[0].tabs[1].custom_name.as_deref(),
            Some("logs")
        );
    }

    #[test]
    fn clicking_last_visible_tab_at_right_edge_does_not_overscroll() {
        let mut app = app_for_mouse_test();
        let mut ws = Workspace::test_new("test");
        for name in [
            "one", "two", "three", "four", "five", "six", "seven", "eight",
        ] {
            ws.test_add_tab(Some(name));
        }
        app.state.workspaces = vec![ws];
        app.state.active = Some(0);
        app.state.selected = 0;
        app.state.tab_scroll = usize::MAX;
        app.state.tab_scroll_follow_active = false;
        crate::ui::compute_view(&mut app.state, Rect::new(0, 0, 65, 20));

        let last_idx = app.state.workspaces[0].tabs.len() - 1;
        let target = app.state.view.tab_hit_areas[last_idx];
        let clamped_scroll = app.state.tab_scroll;
        assert!(target.width > 0, "last tab should already be visible");

        app.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            target.x + 1,
            target.y,
        ));
        app.handle_mouse(mouse(
            MouseEventKind::Up(MouseButton::Left),
            target.x + 1,
            target.y,
        ));

        assert_eq!(app.state.workspaces[0].active_tab, last_idx);
        assert_eq!(app.state.tab_scroll, clamped_scroll);
        assert!(app.state.view.tab_hit_areas[last_idx].width > 0);
    }

    #[test]
    fn dragging_tab_reorders_auto_and_custom_names_without_materializing_numbers() {
        let mut app = app_for_mouse_test();
        let mut ws = Workspace::test_new("test");
        ws.test_add_tab(Some("foo"));
        ws.test_add_tab(None);
        let moved_root = ws.tabs[0].root_pane;
        app.state.workspaces = vec![ws];
        app.state.active = Some(0);
        app.state.selected = 0;
        crate::ui::compute_view(&mut app.state, Rect::new(0, 0, 106, 20));

        let source = app.state.view.tab_hit_areas[0];
        let last = app.state.view.tab_hit_areas[2];
        let drop_col = last.x + last.width;

        app.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            source.x + 1,
            source.y,
        ));
        app.handle_mouse(mouse(
            MouseEventKind::Drag(MouseButton::Left),
            drop_col,
            source.y,
        ));
        assert!(matches!(
            app.state.drag.as_ref().map(|drag| &drag.target),
            Some(DragTarget::TabReorder {
                ws_idx: 0,
                source_tab_idx: 0,
                insert_idx: Some(3),
                ..
            })
        ));
        app.handle_mouse(mouse(
            MouseEventKind::Up(MouseButton::Left),
            drop_col,
            source.y,
        ));

        let labels: Vec<_> = app.state.workspaces[0]
            .tabs
            .iter()
            .enumerate()
            .map(|(tab_idx, _)| app.state.workspaces[0].tab_display_name(tab_idx).unwrap())
            .collect();
        assert_eq!(labels, vec!["foo", "2", "3"]);
        assert_eq!(
            app.state.workspaces[0].tabs[0].custom_name.as_deref(),
            Some("foo")
        );
        assert!(app.state.workspaces[0].tabs[1].custom_name.is_none());
        assert!(app.state.workspaces[0].tabs[2].custom_name.is_none());
        assert_eq!(app.state.workspaces[0].tabs[0].number, 2);
        assert_eq!(app.state.workspaces[0].tabs[1].number, 3);
        assert_eq!(app.state.workspaces[0].tabs[2].number, 1);
        assert_eq!(app.state.workspaces[0].tabs[2].root_pane, moved_root);
        assert_eq!(app.state.workspaces[0].active_tab, 2);
    }

    fn temp_git_repo(branch: &str) -> std::path::PathBuf {
        let repo = unique_temp_path("sidebar-drop-slot-repo");
        fs::create_dir_all(repo.join(".git")).unwrap();
        fs::write(
            repo.join(".git/HEAD"),
            format!("ref: refs/heads/{branch}\n"),
        )
        .unwrap();
        repo
    }

    fn workspace_with_space(name: &str, key: &str) -> Workspace {
        let mut ws = Workspace::test_new(name);
        ws.worktree_space = Some(crate::workspace::WorktreeSpaceMembership {
            key: key.into(),
            label: "herdr".into(),
            repo_root: "/repo/herdr".into(),
            checkout_path: format!("/repo/{name}").into(),
            is_linked_worktree: name != "main",
        });
        ws
    }

    #[test]
    fn top_drop_slot_is_distinct_from_gap_below_first_workspace() {
        let mut app = app_for_mouse_test();
        let first_repo = temp_git_repo("main");
        let second_repo = temp_git_repo("main");

        let mut first = Workspace::test_new("a");
        let first_root = first.tabs[0].root_pane;
        first.identity_cwd = first_repo.clone();
        first.refresh_git_ahead_behind();

        let mut second = Workspace::test_new("b");
        let second_root = second.tabs[0].root_pane;
        second.identity_cwd = second_repo.clone();
        second.refresh_git_ahead_behind();

        app.state.workspaces = vec![first, second];
        app.state.ensure_test_terminals();
        let first_terminal_id = app.state.workspaces[0].tabs[0].panes[&first_root]
            .attached_terminal_id
            .clone();
        app.state.terminals.get_mut(&first_terminal_id).unwrap().cwd = first_repo.clone();
        let second_terminal_id = app.state.workspaces[1].tabs[0].panes[&second_root]
            .attached_terminal_id
            .clone();
        app.state
            .terminals
            .get_mut(&second_terminal_id)
            .unwrap()
            .cwd = second_repo.clone();
        app.state.sidebar_spaces.row_gap = 1;
        crate::ui::compute_view(&mut app.state, Rect::new(0, 0, 106, 20));

        assert_eq!(
            app.state.workspace_drop_target_at_row(0),
            Some(crate::app::state::WorkspaceDropTarget::Before(0))
        );
        assert_eq!(
            app.state.workspace_drop_target_at_row(1),
            Some(crate::app::state::WorkspaceDropTarget::Before(0))
        );
        assert_eq!(
            app.state.workspace_drop_target_at_row(2),
            Some(crate::app::state::WorkspaceDropTarget::Before(0))
        );
        assert_eq!(
            app.state.workspace_drop_target_at_row(3),
            Some(crate::app::state::WorkspaceDropTarget::Before(1))
        );

        let _ = fs::remove_dir_all(first_repo);
        let _ = fs::remove_dir_all(second_repo);
    }

    #[test]
    fn bottom_drop_slot_stays_below_last_workspace_not_footer() {
        let mut app = app_for_mouse_test();
        app.state.workspaces = vec![
            Workspace::test_new("a"),
            Workspace::test_new("b"),
            Workspace::test_new("c"),
        ];
        crate::ui::compute_view(&mut app.state, Rect::new(0, 0, 106, 24));

        let cards = &app.state.view.workspace_card_areas;
        let bottom_slot = crate::ui::workspace_drop_indicator_row(
            &app.state,
            cards,
            &app.state.view.folder_header_areas,
            app.state.workspace_list_rect(),
            &crate::app::state::WorkspaceDropTarget::End,
        )
        .unwrap();

        let last = cards.last().unwrap().rect;
        assert_eq!(bottom_slot, last.y + last.height);
        assert!(bottom_slot < app.state.sidebar_footer_rect().y.saturating_sub(1));
    }

    #[test]
    fn grouped_sidebar_drop_slots_do_not_land_inside_compact_group() {
        let mut app = app_for_mouse_test();
        app.state.workspaces = vec![
            workspace_with_space("main", "repo-key"),
            Workspace::test_new("normal"),
            workspace_with_space("issue", "repo-key"),
        ];
        app.state.active = Some(1);
        app.state.selected = 1;
        crate::ui::compute_view(&mut app.state, Rect::new(0, 0, 106, 40));

        let cards = &app.state.view.workspace_card_areas;
        let order = cards.iter().map(|card| card.ws_idx).collect::<Vec<_>>();
        assert_eq!(order, vec![0, 2, 1]);
        let issue = cards.iter().find(|card| card.ws_idx == 2).unwrap();
        let normal = cards.iter().find(|card| card.ws_idx == 1).unwrap();

        assert_eq!(
            app.state.workspace_drop_target_at_row(issue.rect.y),
            Some(crate::app::state::WorkspaceDropTarget::Before(1))
        );
        assert_eq!(
            crate::ui::workspace_drop_indicator_row(
                &app.state,
                cards,
                &app.state.view.folder_header_areas,
                app.state.workspace_list_rect(),
                &crate::app::state::WorkspaceDropTarget::End,
            ),
            Some(normal.rect.y + normal.rect.height)
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
    fn dragging_worktree_parent_reorders_the_complete_group() {
        let mut app = app_for_mouse_test();
        app.state.workspaces = vec![
            workspace_with_space("main", "repo-key"),
            Workspace::test_new("normal"),
            workspace_with_space("issue", "repo-key"),
        ];
        app.state.active = Some(2);
        app.state.selected = 1;
        crate::ui::compute_view(&mut app.state, Rect::new(0, 0, 106, 40));

        let parent = app
            .state
            .view
            .workspace_card_areas
            .iter()
            .find(|card| card.ws_idx == 0)
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
        let active_id = app.state.workspaces[2].id.clone();
        let selected_id = app.state.workspaces[1].id.clone();

        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), 2, parent.y));
        app.handle_mouse(mouse(
            MouseEventKind::Drag(MouseButton::Left),
            2,
            target_row,
        ));
        assert!(matches!(
            app.state.drag.as_ref().map(|drag| &drag.target),
            Some(DragTarget::WorkspaceReorder {
                source_ws_idx: 0,
                drop_target: Some(crate::app::state::WorkspaceDropTarget::End),
                ..
            })
        ));
        app.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), 2, target_row));

        assert_eq!(
            app.state
                .workspaces
                .iter()
                .map(|workspace| workspace.display_name())
                .collect::<Vec<_>>(),
            ["normal", "main", "issue"]
        );
        assert_eq!(
            app.state.workspaces[app.state.active.unwrap()].id,
            active_id
        );
        assert_eq!(app.state.workspaces[app.state.selected].id, selected_id);
    }

    #[test]
    fn dragging_collapsed_worktree_parent_still_moves_hidden_children() {
        let mut app = app_for_mouse_test();
        app.state.workspaces = vec![
            workspace_with_space("issue", "repo-key"),
            Workspace::test_new("normal"),
            workspace_with_space("main", "repo-key"),
            workspace_with_space("review", "repo-key"),
        ];
        app.state.ensure_test_terminals();
        app.state.active = Some(0);
        app.state.selected = 1;
        app.state.collapsed_space_keys.insert("repo-key".into());
        let active_id = app.state.workspaces[0].id.clone();
        let selected_id = app.state.workspaces[1].id.clone();
        crate::ui::compute_view(&mut app.state, Rect::new(0, 0, 106, 40));
        assert_eq!(app.state.view.workspace_card_areas.len(), 3);

        let parent = app.state.view.workspace_card_areas[0].rect;
        let target_row = crate::ui::workspace_drop_indicator_row(
            &app.state,
            &app.state.view.workspace_card_areas,
            &app.state.view.folder_header_areas,
            app.state.workspace_list_rect(),
            &crate::app::state::WorkspaceDropTarget::End,
        )
        .unwrap();
        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), 2, parent.y));
        app.handle_mouse(mouse(
            MouseEventKind::Drag(MouseButton::Left),
            2,
            target_row,
        ));
        app.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), 2, target_row));

        // The family block moves in its current canonical order (assign
        // semantics); the sidebar still hoists the parent above its children.
        assert_eq!(
            app.state
                .workspaces
                .iter()
                .map(|workspace| workspace.display_name())
                .collect::<Vec<_>>(),
            ["normal", "issue", "main", "review"]
        );
        assert_eq!(
            app.state.workspaces[app.state.active.unwrap()].id,
            active_id
        );
        assert_eq!(app.state.workspaces[app.state.selected].id, selected_id);
        app.state.assert_invariants_for_test();
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
    fn dragging_sidebar_divider_sets_manual_width() {
        let mut app = app_for_mouse_test();

        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), 25, 5));
        app.handle_mouse(mouse(MouseEventKind::Drag(MouseButton::Left), 30, 5));

        assert_eq!(app.state.sidebar_width, 31);
        let snapshot = capture_snapshot(&app.state);
        assert_eq!(snapshot.sidebar_width, Some(31));
    }

    #[test]
    fn dragging_sidebar_bottom_divider_still_sets_manual_width() {
        let mut app = app_for_mouse_test();
        let divider_col = app.state.view.sidebar_rect.x + app.state.view.sidebar_rect.width - 1;
        let bottom_row = app.state.view.sidebar_rect.y + app.state.view.sidebar_rect.height - 1;

        app.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            divider_col,
            bottom_row,
        ));
        app.handle_mouse(mouse(
            MouseEventKind::Drag(MouseButton::Left),
            divider_col + 5,
            bottom_row,
        ));

        assert_eq!(app.state.sidebar_width, 31);
    }

    #[test]
    fn dragging_past_max_clamps_to_configured_max() {
        let mut app = app_for_mouse_test();
        app.state.sidebar_max_width = 30;

        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), 25, 5));
        app.handle_mouse(mouse(MouseEventKind::Drag(MouseButton::Left), 50, 5));

        assert_eq!(app.state.sidebar_width, 30);
    }

    #[test]
    fn dragging_below_min_clamps_to_configured_min() {
        let mut app = app_for_mouse_test();
        app.state.sidebar_min_width = 22;

        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), 25, 5));
        app.handle_mouse(mouse(MouseEventKind::Drag(MouseButton::Left), 5, 5));

        assert_eq!(app.state.sidebar_width, 22);
    }

    #[test]
    fn dragging_sidebar_section_divider_sets_split_ratio() {
        let mut app = app_for_mouse_test();
        let divider = crate::ui::sidebar_section_divider_rect(
            app.state.view.sidebar_rect,
            app.state.sidebar_section_split,
        );

        app.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            divider.x + 1,
            divider.y,
        ));
        app.handle_mouse(mouse(
            MouseEventKind::Drag(MouseButton::Left),
            divider.x + 1,
            divider.y + 4,
        ));

        assert!(app.state.sidebar_section_split > 0.5);
        let snapshot = capture_snapshot(&app.state);
        assert_eq!(
            snapshot.sidebar_section_split,
            Some(app.state.sidebar_section_split)
        );
    }

    #[test]
    fn double_clicking_sidebar_divider_resets_default_width() {
        let mut app = app_for_mouse_test();
        app.state.default_sidebar_width = 26;
        app.state.sidebar_width = 30;

        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), 25, 5));
        app.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), 25, 5));
        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), 25, 5));

        assert_eq!(app.state.sidebar_width, 26);
        assert!(app.state.drag.is_none());
        let snapshot = capture_snapshot(&app.state);
        assert_eq!(snapshot.sidebar_width, Some(26));
    }
}
