//! The agents panel's `folders` mode: machine → folder → workspace → agent,
//! mirroring the spaces panel. Machine and folder collapse is shared with
//! the spaces panel; workspace collapse here is the panel's own.

use super::*;
use crate::api::schema::AgentStatus;

/// Indent step of the agents tree, matching the sidebar's nesting step.
const TREE_INDENT: u16 = 2;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::client::shell) enum FolderAgentHeader {
    Machine(ClientEndpointId),
    Folder(ClientEndpointId, String),
    Workspace(ClientEndpointId, String),
}

pub(in crate::client::shell) enum FolderAgentRow {
    Header {
        header: FolderAgentHeader,
        label: String,
        indent: u16,
        collapsed: bool,
        rollup: Option<AgentStatus>,
        hidden_focus: bool,
        bold: bool,
        dim: bool,
    },
    Agent {
        endpoint_id: ClientEndpointId,
        row: agent_sidebar::AgentRow,
        indent: u16,
        dim: bool,
    },
}

impl FolderAgentRow {
    fn lines(&self) -> usize {
        match self {
            Self::Header { .. } => 1,
            Self::Agent { row, .. } => row.rows.len(),
        }
    }
}

/// Whether the expanded agents panel shows the folders tree: the mode is
/// selected and no agent view overrides it.
pub(in crate::client::shell) fn folders_mode_active(
    folders: &FolderUiState,
    active_snapshot: Option<&ClientShellSnapshot>,
) -> bool {
    folders.agent_mode == AgentPanelMode::Folders
        && active_snapshot.is_none_or(|snapshot| snapshot.agent_view_label.is_none())
}

/// A snapshot's agents grouped by workspace, in snapshot order.
fn agents_by_workspace(
    snapshot: &ClientShellSnapshot,
) -> HashMap<&str, Vec<&crate::protocol::ClientShellAgent>> {
    let mut grouped = HashMap::<&str, Vec<_>>::new();
    for agent in &snapshot.agents {
        grouped
            .entry(agent.workspace_id.as_str())
            .or_default()
            .push(agent);
    }
    grouped
}

/// Drops tokens the tree's headers already show.
fn strip_header_tokens(row: &mut agent_sidebar::AgentRow) {
    for tokens in &mut row.rows {
        tokens.retain(|token| {
            !matches!(
                token.kind,
                crate::ui::ResolvedTokenKind::Machine(_)
                    | crate::ui::ResolvedTokenKind::Workspace(_)
            )
        });
    }
    row.rows.retain(|tokens| !tokens.is_empty());
}

/// The tree rows in display order, leaving out anything under a collapsed
/// header and every header with no agents beneath it.
pub(in crate::client::shell) fn folder_agent_tree(
    folders: &FolderUiState,
    endpoints: &[ClientShellEndpoint],
    active_endpoint_id: &ClientEndpointId,
    collapsed_endpoints: &HashSet<ClientEndpointId>,
    config: &ClientShellConfig,
) -> Vec<FolderAgentRow> {
    let multi = endpoints.len() > 1;
    let mut rows = Vec::new();
    for endpoint in endpoints {
        if !multi && &endpoint.endpoint_id != active_endpoint_id {
            continue;
        }
        let Some(snapshot) = endpoint.snapshot.as_deref() else {
            continue;
        };
        if snapshot.agents.is_empty() {
            continue;
        }
        let active = &endpoint.endpoint_id == active_endpoint_id;
        let focused_workspace = snapshot.focused_workspace_id.as_deref().filter(|_| active);
        let dim = endpoint.status != ClientEndpointStatus::Online;
        if multi {
            let collapsed = collapsed_endpoints.contains(&endpoint.endpoint_id);
            rows.push(FolderAgentRow::Header {
                header: FolderAgentHeader::Machine(endpoint.endpoint_id.clone()),
                label: endpoint.label.clone(),
                indent: 0,
                collapsed,
                rollup: None,
                hidden_focus: collapsed && active,
                bold: true,
                dim,
            });
            if collapsed {
                continue;
            }
        }
        let view = folders.view(endpoint);
        let entries = match view.as_ref().filter(|view| view.has_folders()) {
            Some(view) => entries::all_entries(snapshot, view),
            None => render::workspace_entries(snapshot, &HashSet::new()),
        };
        let collapsed_workspaces = folders
            .collapsed_agent_workspaces
            .get(&endpoint.endpoint_id);
        let grouped = agents_by_workspace(snapshot);
        let agents_in = |workspace: &str| grouped.get(workspace).map_or(&[][..], Vec::as_slice);
        let mut skip_folder = false;
        for entry in entries {
            if let Some((folder, _)) = entry.folder_header() {
                let Some(view) = view.as_ref() else {
                    continue;
                };
                let Some(layout_folder) = view.layout.folders.get(folder) else {
                    continue;
                };
                let statuses = || {
                    layout_folder
                        .workspace_ids
                        .iter()
                        .flat_map(|workspace| agents_in(workspace))
                        .map(|agent| agent.agent_status)
                };
                if statuses().next().is_none() {
                    skip_folder = true;
                    continue;
                }
                let collapsed = view.is_collapsed(folder);
                skip_folder = collapsed;
                rows.push(FolderAgentRow::Header {
                    header: FolderAgentHeader::Folder(
                        endpoint.endpoint_id.clone(),
                        layout_folder.id.clone(),
                    ),
                    label: layout_folder.name.clone(),
                    indent: 0,
                    collapsed,
                    rollup: collapsed.then(|| rows::rollup_status(statuses())).flatten(),
                    hidden_focus: collapsed
                        && focused_workspace.is_some_and(|focused| {
                            layout_folder.workspace_ids.iter().any(|id| id == focused)
                        }),
                    bold: true,
                    dim,
                });
                continue;
            }
            let member = entry.folder == FolderRow::Member;
            if !member {
                skip_folder = false;
            }
            if member && skip_folder {
                continue;
            }
            let Some(workspace) = snapshot.workspaces.get(entry.index) else {
                continue;
            };
            let agents = agents_in(&workspace.workspace_id);
            if agents.is_empty() {
                continue;
            }
            let indent = if member { TREE_INDENT } else { 0 };
            let collapsed = collapsed_workspaces
                .is_some_and(|collapsed| collapsed.contains(&workspace.workspace_id));
            rows.push(FolderAgentRow::Header {
                header: FolderAgentHeader::Workspace(
                    endpoint.endpoint_id.clone(),
                    workspace.workspace_id.clone(),
                ),
                label: workspace.label.clone(),
                indent,
                collapsed,
                rollup: collapsed
                    .then(|| rows::rollup_status(agents.iter().map(|agent| agent.agent_status)))
                    .flatten(),
                hidden_focus: collapsed
                    && focused_workspace == Some(workspace.workspace_id.as_str()),
                bold: false,
                dim,
            });
            if collapsed {
                continue;
            }
            for agent in agents {
                let Some(mut row) =
                    agent_sidebar::agent_row(snapshot, &agent.pane_id, config, None)
                else {
                    continue;
                };
                strip_header_tokens(&mut row);
                row.focused &= active;
                rows.push(FolderAgentRow::Agent {
                    endpoint_id: endpoint.endpoint_id.clone(),
                    row,
                    indent: indent + TREE_INDENT,
                    dim,
                });
            }
        }
    }
    rows
}

/// Renders the folders tree in the expanded agents panel. Returns `false`
/// when another mode applies.
pub(in crate::client::shell) fn render_folder_agent_panel(
    buffer: &mut Buffer,
    area: Rect,
    active_snapshot: Option<&ClientShellSnapshot>,
    config: &ClientShellConfig,
    state: &mut render::ShellRenderState<'_>,
    hits: &mut ShellHitMap,
) -> bool {
    let folders = state.folders.ui;
    if !folders_mode_active(folders, active_snapshot) {
        return false;
    }
    if !agent_sidebar::render_agent_panel_header(buffer, area, None, config, hits) {
        return true;
    }
    relabel_sort_toggle(buffer, area, config, hits);
    let rows = folder_agent_tree(
        folders,
        state.endpoints,
        state.active_endpoint_id,
        state.collapsed_endpoints,
        config,
    );
    let multi = state.endpoints.len() > 1;
    let endpoints = state.endpoints;
    let diagnostics = state.machine_diagnostics;
    agent_sidebar::render_agent_list(
        buffer,
        area,
        &rows,
        None,
        config,
        state.agent_scroll,
        hits,
        FolderAgentRow::lines,
        |buffer, rect, row, hits| match row {
            FolderAgentRow::Header {
                header,
                label,
                indent,
                collapsed,
                rollup,
                hidden_focus,
                bold,
                dim,
            } => {
                if let FolderAgentHeader::Machine(endpoint_id) = header {
                    if let Some(endpoint) = endpoints
                        .iter()
                        .find(|endpoint| &endpoint.endpoint_id == endpoint_id)
                    {
                        endpoint_sidebar::render_endpoint_row(
                            buffer,
                            rect,
                            if *collapsed { "▸" } else { "▾" },
                            endpoint,
                            *hidden_focus,
                            diagnostics,
                            &config.palette,
                        );
                    }
                } else {
                    render_tree_header(
                        buffer,
                        rect,
                        label,
                        *indent,
                        *collapsed,
                        *rollup,
                        *hidden_focus,
                        *bold,
                        config,
                    );
                }
                if *dim {
                    dim_rect(buffer, rect, &config.palette);
                }
                hits.folder_agent_headers.push((rect, header.clone()));
            }
            FolderAgentRow::Agent {
                endpoint_id,
                row,
                indent,
                dim,
            } => {
                let indented = Rect::new(
                    rect.x.saturating_add(*indent),
                    rect.y,
                    rect.width.saturating_sub(*indent),
                    rect.height,
                );
                if row.focused {
                    buffer.set_style(rect, Style::default().bg(config.palette.active_row_bg));
                }
                agent_sidebar::render_agent_row(buffer, indented, row, config);
                if *dim {
                    dim_rect(buffer, rect, &config.palette);
                }
                if multi {
                    hits.endpoint_agents
                        .push((rect, endpoint_id.clone(), row.pane_id.clone()));
                } else {
                    hits.agents.push((rect, row.pane_id.clone()));
                }
            }
        },
    );
    true
}

fn dim_rect(buffer: &mut Buffer, rect: Rect, palette: &Palette) {
    buffer.set_style(
        rect,
        Style::default()
            .fg(palette.overlay0)
            .add_modifier(Modifier::DIM),
    );
}

/// Upstream's header shows the flat sort; in folders mode it says so.
fn relabel_sort_toggle(
    buffer: &mut Buffer,
    area: Rect,
    config: &ClientShellConfig,
    hits: &mut ShellHitMap,
) {
    let label = AgentPanelMode::Folders.label();
    let old = hits.agent_sort_toggle;
    let row = area.y.saturating_add(1);
    let width = display_width(label).min(area.width);
    let previous = display_width(match config.agent_panel_sort {
        crate::config::AgentPanelSortConfig::Spaces => "grouped",
        crate::config::AgentPanelSortConfig::Priority => "priority",
    });
    let clear_from = area.right().saturating_sub(previous.max(width));
    put_text(
        buffer,
        clear_from,
        row,
        area.right().saturating_sub(clear_from),
        &" ".repeat(usize::from(area.right().saturating_sub(clear_from))),
        Style::default(),
    );
    let rect = Rect::new(area.right().saturating_sub(width), row, width, 1);
    put_text(
        buffer,
        rect.x,
        rect.y,
        rect.width,
        label,
        Style::default()
            .fg(config.palette.overlay0)
            .add_modifier(Modifier::BOLD),
    );
    if !old.is_empty() {
        hits.agent_sort_toggle = rect;
    }
}

#[allow(clippy::too_many_arguments)] // One flat row description, like the other row renderers.
fn render_tree_header(
    buffer: &mut Buffer,
    rect: Rect,
    label: &str,
    indent: u16,
    collapsed: bool,
    rollup: Option<AgentStatus>,
    hidden_focus: bool,
    bold: bool,
    config: &ClientShellConfig,
) {
    let palette = &config.palette;
    if hidden_focus {
        buffer.set_style(rect, Style::default().bg(palette.active_row_bg));
    }
    let x = rect.x.saturating_add(indent);
    let right = rect.right();
    put_text(
        buffer,
        x.saturating_add(1),
        rect.y,
        right.saturating_sub(x.saturating_add(1)),
        if collapsed { "▸" } else { "▾" },
        Style::default().fg(if hidden_focus {
            palette.text
        } else {
            palette.overlay0
        }),
    );
    let name_x = x.saturating_add(3);
    let name_width = right.saturating_sub(name_x).saturating_sub(2);
    let mut style = Style::default().fg(if hidden_focus {
        palette.text
    } else {
        palette.subtext0
    });
    if bold || hidden_focus {
        style = style.add_modifier(Modifier::BOLD);
    }
    put_text(
        buffer,
        name_x,
        rect.y,
        name_width,
        &crate::ui::truncate_end(label, usize::from(name_width)),
        style,
    );
    if let Some(status) = rollup.filter(|_| collapsed) {
        put_text(
            buffer,
            right.saturating_sub(1),
            rect.y,
            1,
            status_icon(status, config.status_indicators),
            Style::default().fg(status_color(status, palette)),
        );
    }
}

impl ClientShellState {
    /// Click on a tree header toggles it; it never activates a machine or
    /// focuses a workspace.
    pub(in crate::client::shell) fn click_folder_agent_header(
        &mut self,
        point: (u16, u16),
        outcome: &mut ClientShellInput,
    ) -> bool {
        let Some(header) = self
            .hits
            .folder_agent_headers
            .iter()
            .find(|(rect, _)| super::super::contains(*rect, point))
            .map(|(_, header)| header.clone())
        else {
            return false;
        };
        match header {
            FolderAgentHeader::Machine(endpoint_id) => {
                if !self.collapsed_endpoints.remove(&endpoint_id) {
                    self.collapsed_endpoints.insert(endpoint_id);
                }
            }
            FolderAgentHeader::Folder(endpoint_id, folder_id) => {
                self.folders.toggle_folder(&endpoint_id, &folder_id);
            }
            FolderAgentHeader::Workspace(endpoint_id, workspace_id) => {
                self.folders
                    .toggle_agent_workspace(&endpoint_id, &workspace_id);
            }
        }
        self.persist_chrome_preferences(outcome);
        outcome.repaint = true;
        true
    }

    /// Agents in the order the expanded folders tree shows them, with a flag
    /// for agents under collapsed headers. `None` when another order applies.
    pub(in crate::client::shell) fn folder_agent_order(
        &self,
    ) -> Option<Vec<(ClientEndpointId, String, bool)>> {
        if self.sidebar_collapsed
            || self.mobile_layout_active()
            || !folders_mode_active(&self.folders, self.snapshot.as_deref())
        {
            return None;
        }
        let multi = self.endpoints.len() > 1;
        let mut order = Vec::new();
        for endpoint in &self.endpoints {
            if !multi && endpoint.endpoint_id != self.active_endpoint_id {
                continue;
            }
            if multi && endpoint.status != ClientEndpointStatus::Online {
                continue;
            }
            let Some(snapshot) = endpoint.snapshot.as_deref() else {
                continue;
            };
            let machine_collapsed =
                multi && self.collapsed_endpoints.contains(&endpoint.endpoint_id);
            let view = self.folders.view(endpoint);
            let entries = match view.as_ref().filter(|view| view.has_folders()) {
                Some(view) => entries::all_entries(snapshot, view),
                None => render::workspace_entries(snapshot, &HashSet::new()),
            };
            let collapsed_workspaces = self
                .folders
                .collapsed_agent_workspaces
                .get(&endpoint.endpoint_id);
            for entry in entries {
                let Some(workspace) = snapshot.workspaces.get(entry.index) else {
                    continue;
                };
                let hidden = machine_collapsed
                    || view
                        .as_ref()
                        .is_some_and(|view| view.is_hidden(&workspace.workspace_id))
                    || collapsed_workspaces
                        .is_some_and(|collapsed| collapsed.contains(&workspace.workspace_id));
                for agent in snapshot
                    .agents
                    .iter()
                    .filter(|agent| agent.workspace_id == workspace.workspace_id)
                {
                    order.push((endpoint.endpoint_id.clone(), agent.pane_id.clone(), hidden));
                }
            }
        }
        Some(order)
    }

    /// Next/previous/indexed agent in folders mode. Returns the target, or
    /// `None` when another order applies.
    pub(in crate::client::shell) fn folder_agent_target(
        &self,
        action: &crate::input::KeybindAction,
    ) -> Option<Option<(ClientEndpointId, String)>> {
        use crate::input::KeybindAction;
        let order = self.folder_agent_order()?;
        let visible = order
            .iter()
            .filter(|(_, _, hidden)| !hidden)
            .collect::<Vec<_>>();
        let target = match action {
            KeybindAction::FocusAgent(index) => visible.get(*index).copied(),
            KeybindAction::NextAgent | KeybindAction::PreviousAgent => {
                if visible.is_empty() {
                    return Some(None);
                }
                let focused = self
                    .snapshot
                    .as_deref()
                    .and_then(|snapshot| snapshot.focused_pane_id.as_deref());
                let current = order.iter().position(|(endpoint_id, pane_id, _)| {
                    endpoint_id == &self.active_endpoint_id && Some(pane_id.as_str()) == focused
                });
                let hidden = order
                    .iter()
                    .map(|(_, _, hidden)| *hidden)
                    .collect::<Vec<_>>();
                let delta = if *action == KeybindAction::NextAgent {
                    1
                } else {
                    -1
                };
                let index = match current {
                    Some(current) => navigation::step_visible(&hidden, current, delta),
                    None if delta > 0 => hidden.iter().position(|hidden| !hidden),
                    None => hidden.iter().rposition(|hidden| !hidden),
                };
                index.map(|index| &order[index])
            }
            _ => return None,
        };
        Some(target.map(|(endpoint_id, pane_id, _)| (endpoint_id.clone(), pane_id.clone())))
    }
}
