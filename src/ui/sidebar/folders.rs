//! Folder projection and rendering helpers for the sidebar.

use super::*;

/// Height of a folder header row in the spaces panel.
pub(crate) const FOLDER_HEADER_ROWS: u16 = 1;

/// A display row in the agents panel. The grouped and priority orderings
/// produce only [`AgentPanelListEntry::Agent`] rows; the folder view
/// interleaves folder and space headers mirroring the spaces panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AgentPanelListEntry {
    /// A folder header row. `order_idx` indexes into `AppState::space_order`
    /// (always a `SpaceOrderEntry::Folder` entry).
    FolderHeader { order_idx: usize },
    SpaceHeader {
        ws_idx: usize,
        /// Indented as a worktree family child, mirroring the spaces panel.
        indented: bool,
        /// Nested under a folder header.
        foldered: bool,
        /// Thin ancestor header: the space has no agents of its own and
        /// appears only so its agent-bearing worktree children are not
        /// orphaned.
        thin: bool,
    },
    /// An agent row. `entry_idx` indexes the flat agent entry sequence.
    Agent { entry_idx: usize },
}

/// Whether the agents panel presents the folder view. An API agent view with
/// an explicit sort overrides the panel ordering, matching `apply_agent_view`.
pub(crate) fn agent_folder_view_active(app: &AppState) -> bool {
    matches!(app.agent_panel_sort, AgentPanelSort::Folders)
        && app
            .agent_view_override
            .as_ref()
            .is_none_or(|spec| spec.sort.is_empty())
}

/// Position of each workspace (by `ws_idx`) in the spaces panel's fully
/// expanded visual order. The folder view's flat agent sequence follows this
/// rank so both panels tell the same organizational story; collapse never
/// filters the sequence.
pub(crate) fn folder_view_workspace_ranks(app: &AppState) -> Vec<usize> {
    let mut ranks = vec![usize::MAX; app.workspaces.len()];
    for (pos, entry) in workspace_list_entries_expanded(app).iter().enumerate() {
        if let WorkspaceListEntry::Workspace { ws_idx, .. } = entry {
            if let Some(rank) = ranks.get_mut(*ws_idx) {
                *rank = pos;
            }
        }
    }
    ranks
}

/// Project the flat agent entry sequence into display rows. Outside the
/// folder view every entry maps to one `Agent` row. In the folder view the
/// rows mirror the spaces panel: folder headers above their member spaces,
/// space headers above their agents, worktree families nested exactly as in
/// the spaces panel. Folders and spaces without agents are hidden, except
/// that an agent-less family parent appears as a thin ancestor header when
/// one of its children has agents.
///
/// Collapse only filters display rows, never the flat sequence. Folder
/// collapse is the same state as the spaces panel (`collapsed_folder_ids`):
/// a collapsed folder keeps its header and hides all member rows — the
/// spaces panel's rule. A space in `collapsed_agent_space_ids` keeps its
/// header and hides its whole agent list; the header carries the active
/// indication instead.
pub(crate) fn agent_panel_list_entries(
    app: &AppState,
    entries: &[AgentPanelEntry],
) -> Vec<AgentPanelListEntry> {
    if !agent_folder_view_active(app) {
        return (0..entries.len())
            .map(|entry_idx| AgentPanelListEntry::Agent { entry_idx })
            .collect();
    }

    // Agent entry indices per workspace. The folder view's flat sequence is
    // sorted to the spaces-panel order (see `apply_agent_view`), so each
    // space's agents are contiguous and in flat-sequence order here.
    let mut agents_by_ws: Vec<Vec<usize>> = vec![Vec::new(); app.workspaces.len()];
    for (entry_idx, entry) in entries.iter().enumerate() {
        if let Some(slot) = agents_by_ws.get_mut(entry.ws_idx) {
            slot.push(entry_idx);
        }
    }
    let ws_has_agents = |ws_idx: usize| agents_by_ws.get(ws_idx).is_some_and(|a| !a.is_empty());
    // Append a space's agent rows. A collapsed agent list hides all of its
    // rows — the active pane's included — like a collapsed folder; the
    // space header carries the active indication instead.
    let extend_agent_rows = |rows: &mut Vec<AgentPanelListEntry>, ws_idx: usize| {
        let Some(agent_entries) = agents_by_ws.get(ws_idx) else {
            return;
        };
        let collapsed = app
            .workspaces
            .get(ws_idx)
            .is_some_and(|ws| app.collapsed_agent_space_ids.contains(&ws.id));
        if collapsed {
            return;
        }
        rows.extend(
            agent_entries
                .iter()
                .map(|entry_idx| AgentPanelListEntry::Agent {
                    entry_idx: *entry_idx,
                }),
        );
    };

    let workspace_entries = workspace_list_entries_expanded(app);
    let mut rows = Vec::new();
    // Folder headers are emitted lazily before the folder's first visible
    // member so agent-less folders stay hidden.
    let mut pending_folder: Option<usize> = None;
    // Whether subsequent foldered entries sit inside a collapsed folder.
    let mut in_collapsed_folder = false;
    for (idx, entry) in workspace_entries.iter().enumerate() {
        match entry {
            WorkspaceListEntry::FolderHeader { order_idx } => {
                in_collapsed_folder = matches!(
                    app.space_order.get(*order_idx),
                    Some(crate::folder::SpaceOrderEntry::Folder(folder))
                        if app.collapsed_folder_ids.contains(&folder.id)
                );
                if in_collapsed_folder {
                    pending_folder = None;
                    // A collapsed folder hides its members, so the lazy
                    // emission can never fire; keep the header whenever any
                    // member has agents so the folder stays expandable here.
                    let members_have_agents = workspace_entries[idx + 1..]
                        .iter()
                        .take_while(|next| {
                            matches!(next, WorkspaceListEntry::Workspace { foldered: true, .. })
                        })
                        .any(|next| {
                            matches!(
                                next,
                                WorkspaceListEntry::Workspace { ws_idx, .. }
                                    if ws_has_agents(*ws_idx)
                            )
                        });
                    if members_have_agents {
                        rows.push(AgentPanelListEntry::FolderHeader {
                            order_idx: *order_idx,
                        });
                    }
                } else {
                    pending_folder = Some(*order_idx);
                }
            }
            WorkspaceListEntry::Workspace {
                ws_idx,
                indented,
                foldered,
            } => {
                if !*foldered {
                    in_collapsed_folder = false;
                }
                if *foldered && in_collapsed_folder {
                    // Mirror the spaces panel: a collapsed folder hides all
                    // member rows; the folder header carries the active
                    // indication instead.
                    continue;
                }
                let thin = !ws_has_agents(*ws_idx);
                if thin {
                    // An agent-less family parent appears as a thin ancestor
                    // header only when an agent-bearing child follows.
                    let child_has_agents = !*indented
                        && workspace_entries[idx + 1..]
                            .iter()
                            .take_while(|next| {
                                matches!(next, WorkspaceListEntry::Workspace { indented: true, .. })
                            })
                            .any(|next| {
                                matches!(
                                    next,
                                    WorkspaceListEntry::Workspace { ws_idx, .. }
                                        if ws_has_agents(*ws_idx)
                                )
                            });
                    if !child_has_agents {
                        continue;
                    }
                }
                if *foldered {
                    if let Some(order_idx) = pending_folder.take() {
                        rows.push(AgentPanelListEntry::FolderHeader { order_idx });
                    }
                }
                rows.push(AgentPanelListEntry::SpaceHeader {
                    ws_idx: *ws_idx,
                    indented: *indented,
                    foldered: *foldered,
                    thin,
                });
                extend_agent_rows(&mut rows, *ws_idx);
            }
        }
    }
    rows
}

/// Display row index of a flat agent entry, for scroll targeting. Outside the
/// folder view rows and entries coincide. An entry hidden by collapse maps to
/// its nearest visible ancestor header: its space header, then its folder
/// header.
pub(crate) fn agent_panel_row_for_entry(app: &AppState, entry_idx: usize) -> usize {
    if !agent_folder_view_active(app) {
        return entry_idx;
    }
    let entries = agent_panel_entries(app);
    let rows = agent_panel_list_entries(app, &entries);
    if let Some(pos) = rows.iter().position(
        |row| matches!(row, AgentPanelListEntry::Agent { entry_idx: e } if *e == entry_idx),
    ) {
        return pos;
    }
    let Some(ws_idx) = entries.get(entry_idx).map(|entry| entry.ws_idx) else {
        return 0;
    };
    if let Some(pos) = rows.iter().position(
        |row| matches!(row, AgentPanelListEntry::SpaceHeader { ws_idx: w, .. } if *w == ws_idx),
    ) {
        return pos;
    }
    let ws_id = app.workspaces.get(ws_idx).map(|ws| ws.id.as_str());
    if let Some(order_idx) = ws_id.and_then(|ws_id| {
        app.space_order.iter().position(|entry| {
            matches!(
                entry,
                crate::folder::SpaceOrderEntry::Folder(folder)
                    if folder.members.iter().any(|member| member == ws_id)
            )
        })
    }) {
        if let Some(pos) = rows.iter().position(
            |row| matches!(row, AgentPanelListEntry::FolderHeader { order_idx: o } if *o == order_idx),
        ) {
            return pos;
        }
    }
    0
}

/// Whether `entry` is the folder header row for `folder_id`.
pub(crate) fn entry_is_folder_header(
    app: &AppState,
    entry: &WorkspaceListEntry,
    folder_id: &str,
) -> bool {
    matches!(
        entry,
        WorkspaceListEntry::FolderHeader { order_idx }
            if matches!(
                app.space_order.get(*order_idx),
                Some(crate::folder::SpaceOrderEntry::Folder(folder)) if folder.id == folder_id
            )
    )
}

/// Height of a folder or space header row in the agents panel folder view.
pub(crate) const AGENT_LIST_HEADER_ROWS: u16 = 1;

pub(crate) fn agent_row_height_in_body(
    app: &AppState,
    entries: &[AgentPanelEntry],
    row: &AgentPanelListEntry,
    body_height: u16,
) -> u16 {
    match row {
        AgentPanelListEntry::FolderHeader { .. } | AgentPanelListEntry::SpaceHeader { .. } => {
            AGENT_LIST_HEADER_ROWS.min(body_height)
        }
        AgentPanelListEntry::Agent { entry_idx } => entries
            .get(*entry_idx)
            .map(|entry| agent_entry_height_in_body(app, entry, body_height))
            .unwrap_or(0),
    }
}

/// Vertical gap after an agents-panel display row. Headers hug the content
/// nested beneath them; agent rows keep the configured row gap, with none
/// after the last row.
pub(crate) fn agent_row_gap(app: &AppState, rows: &[AgentPanelListEntry], row_idx: usize) -> u16 {
    if row_idx + 1 >= rows.len() {
        return 0;
    }
    if matches!(
        rows.get(row_idx),
        Some(AgentPanelListEntry::FolderHeader { .. } | AgentPanelListEntry::SpaceHeader { .. })
    ) {
        return 0;
    }
    app.sidebar_agents.row_gap
}

/// Collapse/expand chevron cell on a folder header: the state-icon column of
/// the row (after the 1-cell gutter), filesystem-browser style, so the
/// chevron aligns with loose space cards' icons and the folder name aligns
/// with their names.
pub(crate) fn folder_header_chevron_rect(header: &crate::app::state::FolderHeaderArea) -> Rect {
    if header.rect.width < 2 || header.rect.height == 0 {
        return Rect::default();
    }

    Rect::new(header.rect.x + 1, header.rect.y, 1, 1)
}

/// Collapse/expand chevron cell of an agents-panel folder or space header row
/// at `row_y`: the leading cell after the header's indent prefix (gutter,
/// folder margin, and any family connector), filesystem-browser style,
/// mirroring `folder_header_chevron_rect`. `indent` is the prefix width
/// before the chevron: [`AGENT_PANEL_HEADER_GUTTER`] for folder headers,
/// `space_header_chevron_indent` for space headers.
pub(crate) fn agent_panel_header_chevron_rect(body: Rect, row_y: u16, indent: u16) -> Rect {
    if body.width <= indent || body.height == 0 {
        return Rect::default();
    }

    Rect::new(body.x + indent, row_y, 1, 1)
}

/// Draw a collapse/expand chevron into its 1x1 cell.
pub(crate) fn render_collapse_chevron(
    frame: &mut Frame,
    collapsed: bool,
    rect: Rect,
    accent: Color,
) {
    frame.render_widget(
        Paragraph::new(Span::styled(
            if collapsed { "▸" } else { "▾" },
            Style::default().fg(accent),
        )),
        rect,
    );
}

/// Highlight carried by a collapsed folder header standing in for its hidden
/// members: like VSCode lighting up a collapsed folder that contains the
/// open file. An expanded folder never highlights — its member rows carry
/// the highlight themselves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct FolderHeaderHighlight {
    /// The Navigate-mode selected space is hidden inside the folder.
    pub selected: bool,
    /// The active space is hidden inside the folder.
    pub active: bool,
}

pub(crate) fn collapsed_folder_header_highlight(
    app: &AppState,
    folder: &crate::folder::Folder,
) -> FolderHeaderHighlight {
    if !app.collapsed_folder_ids.contains(&folder.id) {
        return FolderHeaderHighlight::default();
    }
    let contains = |ws_idx: usize| {
        app.workspaces
            .get(ws_idx)
            .is_some_and(|ws| folder.members.contains(&ws.id))
    };
    FolderHeaderHighlight {
        selected: matches!(app.mode, Mode::Navigate) && contains(app.selected),
        active: app.active.is_some_and(contains),
    }
}

/// Whether a drop target lands at the top level of the space order — the
/// only legal placements for a folder drag (a folder can never be dropped
/// into another folder).
pub(crate) fn is_top_level_drop_target(target: &crate::app::state::WorkspaceDropTarget) -> bool {
    matches!(
        target,
        crate::app::state::WorkspaceDropTarget::Before(_)
            | crate::app::state::WorkspaceDropTarget::BeforeFolder(_)
            | crate::app::state::WorkspaceDropTarget::End
    )
}

/// 1-cell gutter before the agents panel folder view's top-level headers,
/// matching the spaces panel's leading gutter so top-level chevrons align
/// across panels.
pub(crate) const AGENT_PANEL_HEADER_GUTTER: u16 = 1;

/// Width of the indent (gutter, folder margin, and any worktree connector)
/// before a folder-view space header's collapse chevron.
pub(crate) fn space_header_chevron_indent(indented: bool, foldered: bool) -> u16 {
    AGENT_PANEL_HEADER_GUTTER + (if foldered { 2 } else { 0 }) + (if indented { 6 } else { 0 })
}

/// Width of the prefix (indent, chevron cell, and gap cell) before a
/// folder-view space header's name.
pub(crate) fn space_header_prefix_width(indented: bool, foldered: bool) -> u16 {
    space_header_chevron_indent(indented, foldered) + 2
}

/// Indent applied to agent rows nested under a folder-view space header:
/// one cell past the header's chevron column, so the row's leading state
/// icon lands under the first letter of the header's name — mirroring how
/// the spaces panel nests member icons under their folder's name.
pub(crate) fn space_header_agent_indent(indented: bool, foldered: bool) -> u16 {
    space_header_chevron_indent(indented, foldered) + 1
}

/// Whether the next header row after `idx` (skipping agent rows) is an
/// indented space header, i.e. the current worktree child is not the last of
/// its family in the folder view.
pub(crate) fn next_agent_header_is_indented_space(
    rows: &[AgentPanelListEntry],
    idx: usize,
) -> bool {
    rows[idx.saturating_add(1)..]
        .iter()
        .find_map(|row| match row {
            AgentPanelListEntry::Agent { .. } => None,
            AgentPanelListEntry::SpaceHeader { indented, .. } => Some(*indented),
            AgentPanelListEntry::FolderHeader { .. } => Some(false),
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::super::tests::{
        find_symbol_x, row_text, workspace_with_git_space, workspace_with_worktree_space,
    };
    use super::*;
    use crate::{detect::Agent, layout::PaneId, workspace::Workspace};
    use ratatui::{backend::TestBackend, Terminal};

    #[test]
    fn folder_view_renders_folder_and_space_headers_above_agents() {
        let mut app = crate::app::state::AppState::test_new();
        app.workspaces = vec![Workspace::test_new("one"), Workspace::test_new("two")];
        let member = app.workspaces[1].id.clone();
        let folder_id = app.create_folder("work").expect("create folder");
        app.assign_workspace_to_folder(&member, Some(&folder_id), None)
            .expect("assign");
        // Canonical order: one, [work: two]
        app.ensure_test_terminals();
        app.active = Some(0);
        for ws_idx in 0..app.workspaces.len() {
            set_root_agent(&mut app, ws_idx, Agent::Pi);
        }
        app.agent_panel_sort = crate::app::state::AgentPanelSort::Folders;
        app.sidebar_agents.rows = vec![vec![crate::config::AgentSidebarToken::Agent]];
        app.sidebar_agents.row_gap = 0;

        let area = Rect::new(0, 0, 26, 20);
        let mut terminal = Terminal::new(TestBackend::new(26, 20)).unwrap();
        terminal
            .draw(|frame| render_sidebar(&app, &TerminalRuntimeRegistry::new(), frame, area))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let (_, agent_area) = expanded_sidebar_sections(area, app.sidebar_section_split);
        let body = agent_panel_body_rect(agent_area, false);

        assert!(
            row_text(buffer, agent_area.y + 1, 25).ends_with("folders"),
            "the header label names the active ordering"
        );
        // Filesystem-browser layout: a 1-cell gutter (matching the spaces
        // panel) then the chevron leading each collapsible header, foldered
        // headers keep the folder-nesting margin, and agent rows start under
        // their header name's first letter.
        let one_row = row_text(buffer, body.y, 25);
        assert!(
            one_row.starts_with(" ▾ one"),
            "space headers lead with a collapse chevron after the gutter: {one_row:?}"
        );
        assert_eq!(row_text(buffer, body.y + 1, 25), "   pi");
        let folder_row = row_text(buffer, body.y + 2, 25);
        assert!(
            folder_row.starts_with(" ▾ work"),
            "folder headers lead with a collapse chevron after the gutter: {folder_row:?}"
        );
        // The gutter aligns top-level chevrons across panels: the agents
        // panel folder chevron sits in the spaces panel's chevron column.
        let spaces_chevron_x =
            folder_header_chevron_rect(&compute_workspace_list_areas(&app, area).1[0]).x;
        assert_eq!(buffer[(spaces_chevron_x, body.y + 2)].symbol(), "▾");
        let two_row = row_text(buffer, body.y + 3, 25);
        assert!(
            two_row.starts_with("   ▾ two"),
            "foldered space headers lead with a collapse chevron: {two_row:?}"
        );
        assert_eq!(row_text(buffer, body.y + 4, 25), "     pi");

        // Space header names carry the same weight as folder headers,
        // foldered or not.
        let one_style = buffer[(find_symbol_x(buffer, body.y, body.width, "o"), body.y)].style();
        assert!(
            one_style.add_modifier.contains(Modifier::BOLD),
            "a space header name is bold like a folder header"
        );
        let work_style = buffer[(
            find_symbol_x(buffer, body.y + 2, body.width, "w"),
            body.y + 2,
        )]
            .style();
        assert!(work_style.add_modifier.contains(Modifier::BOLD));
        let two_style = buffer[(
            find_symbol_x(buffer, body.y + 3, body.width, "t"),
            body.y + 3,
        )]
            .style();
        assert!(
            two_style.add_modifier.contains(Modifier::BOLD),
            "a foldered space header name is bold too"
        );
    }

    #[test]
    fn agents_panel_headers_render_collapse_chevrons_matching_state() {
        let mut app = crate::app::state::AppState::test_new();
        app.workspaces = vec![Workspace::test_new("one"), Workspace::test_new("two")];
        let member = app.workspaces[1].id.clone();
        let folder_id = app.create_folder("work").expect("create folder");
        app.assign_workspace_to_folder(&member, Some(&folder_id), None)
            .expect("assign");
        // Canonical order: one, [work: two]
        app.ensure_test_terminals();
        app.mode = Mode::Terminal;
        app.active = Some(0);
        for ws_idx in 0..app.workspaces.len() {
            set_root_agent(&mut app, ws_idx, Agent::Pi);
        }
        app.agent_panel_sort = crate::app::state::AgentPanelSort::Folders;
        app.sidebar_agents.rows = vec![vec![crate::config::AgentSidebarToken::Agent]];
        app.sidebar_agents.row_gap = 0;

        let area = Rect::new(0, 0, 26, 20);
        let render = |app: &crate::app::state::AppState| {
            let mut terminal = Terminal::new(TestBackend::new(26, 20)).unwrap();
            terminal
                .draw(|frame| render_sidebar(app, &TerminalRuntimeRegistry::new(), frame, area))
                .unwrap();
            terminal.backend().buffer().clone()
        };
        let (_, agent_area) = expanded_sidebar_sections(area, app.sidebar_section_split);
        let body = agent_panel_body_rect(agent_area, false);
        // Chevrons lead their headers after the 1-cell gutter: top-level
        // headers one past the body's left edge, foldered space headers past
        // the 2-cell folder margin on top of the gutter.
        let top_level_chevron_x = body.x + 1;
        let foldered_chevron_x = body.x + 3;

        // Rows: header(one), pi, folder(work), header(two), pi.
        let buffer = render(&app);
        assert_eq!(buffer[(top_level_chevron_x, body.y)].symbol(), "▾");
        assert_eq!(buffer[(top_level_chevron_x, body.y + 2)].symbol(), "▾");
        assert_eq!(buffer[(foldered_chevron_x, body.y + 3)].symbol(), "▾");

        app.collapsed_folder_ids.insert(folder_id);
        let one_id = app.workspaces[0].id.clone();
        app.collapsed_agent_space_ids.insert(one_id);
        let buffer = render(&app);
        // Rows: header(one) with its agent list hidden, folder(work) with
        // its members hidden.
        assert_eq!(
            buffer[(top_level_chevron_x, body.y)].symbol(),
            "▸",
            "the collapsed space header shows a collapsed chevron"
        );
        assert_eq!(
            buffer[(top_level_chevron_x, body.y + 1)].symbol(),
            "▸",
            "the collapsed folder header shows a collapsed chevron"
        );

        app.collapsed_folder_ids.clear();
        app.collapsed_agent_space_ids.clear();
        let two_id = app.workspaces[1].id.clone();
        app.collapsed_agent_space_ids.insert(two_id);
        let buffer = render(&app);
        assert_eq!(
            buffer[(foldered_chevron_x, body.y + 3)].symbol(),
            "▸",
            "the collapsed space header shows a collapsed chevron"
        );
        assert_eq!(
            row_text(&buffer, body.y + 4, 25),
            "",
            "the collapsed space hides its agent rows"
        );
    }

    #[test]
    fn folder_view_collapsed_folder_header_indicates_hidden_active_space() {
        let mut app = crate::app::state::AppState::test_new();
        app.workspaces = vec![Workspace::test_new("one"), Workspace::test_new("two")];
        let member = app.workspaces[1].id.clone();
        let folder_id = app.create_folder("work").expect("create folder");
        app.assign_workspace_to_folder(&member, Some(&folder_id), None)
            .expect("assign");
        // Canonical order: one, [work: two]
        app.ensure_test_terminals();
        app.mode = Mode::Terminal;
        app.active = Some(1);
        for ws_idx in 0..app.workspaces.len() {
            set_root_agent(&mut app, ws_idx, Agent::Pi);
        }
        app.agent_panel_sort = crate::app::state::AgentPanelSort::Folders;
        app.sidebar_agents.rows = vec![vec![crate::config::AgentSidebarToken::Agent]];
        app.sidebar_agents.row_gap = 0;
        app.collapsed_folder_ids.insert(folder_id);

        let area = Rect::new(0, 0, 26, 20);
        let mut terminal = Terminal::new(TestBackend::new(26, 20)).unwrap();
        terminal
            .draw(|frame| render_sidebar(&app, &TerminalRuntimeRegistry::new(), frame, area))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let (_, agent_area) = expanded_sidebar_sections(area, app.sidebar_section_split);
        let body = agent_panel_body_rect(agent_area, false);

        // Rows: header(one), pi, folder(work) with every member row hidden.
        assert_eq!(
            row_text(buffer, body.y + 3, 25),
            "",
            "a collapsed folder hides all member rows, the active space included"
        );
        let name_x = find_symbol_x(buffer, body.y + 2, body.width, "w");
        let header = buffer[(name_x, body.y + 2)].style();
        assert_eq!(
            header.bg,
            Some(app.palette.active_row_bg),
            "the folder header indicates the hidden active space"
        );
        assert_eq!(header.fg, Some(app.palette.text));
    }

    #[test]
    fn folder_view_collapsed_space_header_indicates_hidden_active_agent() {
        let mut app = crate::app::state::AppState::test_new();
        app.workspaces = vec![Workspace::test_new("one"), Workspace::test_new("two")];
        app.ensure_test_terminals();
        app.mode = Mode::Terminal;
        app.active = Some(0);
        for ws_idx in 0..app.workspaces.len() {
            set_root_agent(&mut app, ws_idx, Agent::Pi);
        }
        app.agent_panel_sort = crate::app::state::AgentPanelSort::Folders;
        app.sidebar_agents.rows = vec![vec![crate::config::AgentSidebarToken::Agent]];
        app.sidebar_agents.row_gap = 0;
        let one_id = app.workspaces[0].id.clone();
        app.collapsed_agent_space_ids.insert(one_id);

        let area = Rect::new(0, 0, 26, 20);
        let mut terminal = Terminal::new(TestBackend::new(26, 20)).unwrap();
        terminal
            .draw(|frame| render_sidebar(&app, &TerminalRuntimeRegistry::new(), frame, area))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let (_, agent_area) = expanded_sidebar_sections(area, app.sidebar_section_split);
        let body = agent_panel_body_rect(agent_area, false);

        // Rows: header(one) with its agent list hidden, header(two), pi.
        let two_row = row_text(buffer, body.y + 1, 25);
        assert!(
            two_row.starts_with(" ▾ two"),
            "the collapsed space hides its agent rows: {two_row:?}"
        );
        let name_x = find_symbol_x(buffer, body.y, body.width, "o");
        let header = buffer[(name_x, body.y)].style();
        assert_eq!(
            header.bg,
            Some(app.palette.active_row_bg),
            "the space header indicates the hidden active agent"
        );
        assert_eq!(header.fg, Some(app.palette.text));

        // The inactive space's collapsed header stays unhighlighted.
        app.active = Some(1);
        let mut terminal = Terminal::new(TestBackend::new(26, 20)).unwrap();
        terminal
            .draw(|frame| render_sidebar(&app, &TerminalRuntimeRegistry::new(), frame, area))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let header = buffer[(name_x, body.y)].style();
        assert_eq!(header.bg, Some(app.palette.sidebar_bg));
        assert_eq!(header.fg, Some(app.palette.subtext0));
    }

    #[test]
    fn thin_ancestor_header_renders_without_chevron() {
        let mut app = crate::app::state::AppState::test_new();
        app.workspaces = vec![
            workspace_with_worktree_space("main", Some("repo-key"), "/repo/herdr"),
            workspace_with_worktree_space("issue", Some("repo-key"), "/repo/herdr-issue"),
        ];
        app.ensure_test_terminals();
        app.agent_panel_sort = crate::app::state::AgentPanelSort::Folders;
        app.sidebar_agents.rows = vec![vec![crate::config::AgentSidebarToken::Agent]];
        app.sidebar_agents.row_gap = 0;
        // Only the child has an agent: the parent renders as a thin header.
        set_root_agent(&mut app, 1, Agent::Pi);

        let area = Rect::new(0, 0, 26, 20);
        let mut terminal = Terminal::new(TestBackend::new(26, 20)).unwrap();
        terminal
            .draw(|frame| render_sidebar(&app, &TerminalRuntimeRegistry::new(), frame, area))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let (_, agent_area) = expanded_sidebar_sections(area, app.sidebar_section_split);
        let body = agent_panel_body_rect(agent_area, false);

        // Rows: thin header(main), header(issue), pi.
        assert_eq!(
            buffer[(body.x + 1, body.y)].symbol(),
            " ",
            "a thin ancestor header has no agent list to collapse"
        );
        // The indented child's chevron follows the gutter and its family
        // connector.
        assert_eq!(buffer[(body.x + 7, body.y + 1)].symbol(), "▾");
    }

    /// Loose "one" plus folder "work" containing "two". Canonical order:
    /// one, [work: two] — ws_idx 0 = one, 1 = two.
    fn folder_highlight_state() -> (crate::app::state::AppState, String) {
        let mut app = crate::app::state::AppState::test_new();
        app.workspaces = vec![Workspace::test_new("one"), Workspace::test_new("two")];
        let member = app.workspaces[1].id.clone();
        let folder_id = app.create_folder("work").expect("create folder");
        app.assign_workspace_to_folder(&member, Some(&folder_id), None)
            .expect("assign");
        app.mode = Mode::Terminal;
        (app, folder_id)
    }

    #[test]
    fn collapsed_folder_header_highlight_tracks_hidden_active_and_selected() {
        let (mut app, folder_id) = folder_highlight_state();
        app.active = Some(1);
        let folder = app.folder(&folder_id).expect("folder").clone();

        // Expanded: the member row carries the highlight, not the header.
        assert_eq!(
            collapsed_folder_header_highlight(&app, &folder),
            FolderHeaderHighlight::default()
        );

        app.collapsed_folder_ids.insert(folder_id.clone());
        assert_eq!(
            collapsed_folder_header_highlight(&app, &folder),
            FolderHeaderHighlight {
                selected: false,
                active: true,
            }
        );

        // The Navigate-mode selection inside the folder lights the header.
        app.mode = Mode::Navigate;
        app.selected = 1;
        app.active = None;
        assert_eq!(
            collapsed_folder_header_highlight(&app, &folder),
            FolderHeaderHighlight {
                selected: true,
                active: false,
            }
        );

        // Selection means nothing outside Navigate mode.
        app.mode = Mode::Terminal;
        assert_eq!(
            collapsed_folder_header_highlight(&app, &folder),
            FolderHeaderHighlight::default()
        );

        // Active/selected spaces outside the folder never light the header.
        app.mode = Mode::Navigate;
        app.selected = 0;
        app.active = Some(0);
        assert_eq!(
            collapsed_folder_header_highlight(&app, &folder),
            FolderHeaderHighlight::default()
        );
    }

    #[test]
    fn collapsed_folder_header_carries_active_space_background() {
        let (mut app, folder_id) = folder_highlight_state();
        app.active = Some(1);
        app.collapsed_folder_ids.insert(folder_id);

        let area = Rect::new(0, 0, 26, 20);
        let (cards, headers) = compute_workspace_list_areas(&app, area);
        app.view.workspace_card_areas = cards;
        app.view.folder_header_areas = headers;
        let header_row = app.view.folder_header_areas[0].rect.y;
        let mut terminal = Terminal::new(TestBackend::new(26, 20)).unwrap();
        terminal
            .draw(|frame| render_sidebar(&app, &TerminalRuntimeRegistry::new(), frame, area))
            .unwrap();
        let buffer = terminal.backend().buffer();

        assert_eq!(
            buffer[(0, header_row)].bg,
            app.palette.active_row_bg,
            "the collapsed folder header carries the active-space background"
        );
        let name = buffer[(find_symbol_x(buffer, header_row, 25, "w"), header_row)].style();
        assert_eq!(name.fg, Some(app.palette.text));
        assert!(name.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn collapsed_folder_header_carries_navigate_selection_background() {
        let (mut app, folder_id) = folder_highlight_state();
        app.mode = Mode::Navigate;
        app.selected = 1;
        app.active = None;
        app.collapsed_folder_ids.insert(folder_id);

        let area = Rect::new(0, 0, 26, 20);
        let (cards, headers) = compute_workspace_list_areas(&app, area);
        app.view.workspace_card_areas = cards;
        app.view.folder_header_areas = headers;
        let header_row = app.view.folder_header_areas[0].rect.y;
        let mut terminal = Terminal::new(TestBackend::new(26, 20)).unwrap();
        terminal
            .draw(|frame| render_sidebar(&app, &TerminalRuntimeRegistry::new(), frame, area))
            .unwrap();

        assert_eq!(
            terminal.backend().buffer()[(0, header_row)].bg,
            app.palette.selection_bg,
            "the collapsed folder header carries the Navigate selection background"
        );
    }

    fn set_root_agent(app: &mut crate::app::state::AppState, ws_idx: usize, agent: Agent) {
        let pane = app.workspaces[ws_idx].tabs[0].root_pane;
        let terminal_id = app.workspaces[ws_idx].tabs[0].panes[&pane]
            .attached_terminal_id
            .clone();
        app.terminals.get_mut(&terminal_id).unwrap().detected_agent = Some(agent);
    }

    /// Non-contiguous worktree family: the spaces panel hoists the child next
    /// to its parent, and the folder view's flat sequence must follow that
    /// visual order rather than the raw workspace vec.
    #[test]
    fn folder_view_flat_entries_mirror_spaces_panel_expanded_order() {
        let mut app = crate::app::state::AppState::test_new();
        app.workspaces = vec![
            workspace_with_worktree_space("main", Some("repo-key"), "/repo/herdr"),
            workspace_with_git_space("normal", "other-key"),
            workspace_with_worktree_space("issue", Some("repo-key"), "/repo/herdr-issue"),
        ];
        app.ensure_test_terminals();
        for ws_idx in 0..app.workspaces.len() {
            set_root_agent(&mut app, ws_idx, Agent::Claude);
        }

        let grouped: Vec<usize> = agent_panel_entries(&app)
            .iter()
            .map(|entry| entry.ws_idx)
            .collect();
        assert_eq!(grouped, [0, 1, 2], "grouped order follows the raw vec");

        app.agent_panel_sort = crate::app::state::AgentPanelSort::Folders;
        let folder_view: Vec<usize> = agent_panel_entries(&app)
            .iter()
            .map(|entry| entry.ws_idx)
            .collect();
        assert_eq!(
            folder_view,
            [0, 2, 1],
            "folder view hoists the family child next to its parent, like the spaces panel"
        );
    }

    #[test]
    fn folder_view_rows_nest_folder_space_agents_mirroring_spaces_panel() {
        let mut app = crate::app::state::AppState::test_new();
        app.workspaces = vec![
            workspace_with_worktree_space("main", Some("repo-key"), "/repo/herdr"),
            workspace_with_worktree_space("issue", Some("repo-key"), "/repo/herdr-issue"),
            Workspace::test_new("notes"),
        ];
        let parent = app.workspaces[0].id.clone();
        let folder_id = app.create_folder("work").expect("create folder");
        app.assign_workspace_to_folder(&parent, Some(&folder_id), None)
            .expect("assign family");
        // Canonical order after assign: notes, [work: main, issue]
        app.ensure_test_terminals();
        for ws_idx in 0..app.workspaces.len() {
            set_root_agent(&mut app, ws_idx, Agent::Claude);
        }
        app.agent_panel_sort = crate::app::state::AgentPanelSort::Folders;

        let entries = agent_panel_entries(&app);
        let rows = agent_panel_list_entries(&app, &entries);

        assert_eq!(
            rows,
            vec![
                AgentPanelListEntry::SpaceHeader {
                    ws_idx: 0,
                    indented: false,
                    foldered: false,
                    thin: false,
                },
                AgentPanelListEntry::Agent { entry_idx: 0 },
                AgentPanelListEntry::FolderHeader { order_idx: 1 },
                AgentPanelListEntry::SpaceHeader {
                    ws_idx: 1,
                    indented: false,
                    foldered: true,
                    thin: false,
                },
                AgentPanelListEntry::Agent { entry_idx: 1 },
                AgentPanelListEntry::SpaceHeader {
                    ws_idx: 2,
                    indented: true,
                    foldered: true,
                    thin: false,
                },
                AgentPanelListEntry::Agent { entry_idx: 2 },
            ]
        );
        assert_eq!(entries[0].ws_idx, 0);
        assert_eq!(entries[1].ws_idx, 1);
        assert_eq!(entries[2].ws_idx, 2);
    }

    #[test]
    fn folder_view_hides_agentless_folders_and_spaces() {
        let mut app = crate::app::state::AppState::test_new();
        app.workspaces = vec![
            Workspace::test_new("one"),
            Workspace::test_new("two"),
            Workspace::test_new("three"),
        ];
        let foldered = app.workspaces[1].id.clone();
        app.create_folder("quiet").expect("create folder");
        let folder_id = app.create_folder("busy").expect("create folder");
        app.assign_workspace_to_folder(&foldered, Some(&folder_id), None)
            .expect("assign");
        // Canonical order: one, three, [quiet], [busy: two]
        app.ensure_test_terminals();
        app.agent_panel_sort = crate::app::state::AgentPanelSort::Folders;
        // Only "three" has an agent.
        let three_idx = app
            .workspaces
            .iter()
            .position(|ws| ws.display_name() == "three")
            .unwrap();
        set_root_agent(&mut app, three_idx, Agent::Claude);

        let entries = agent_panel_entries(&app);
        let rows = agent_panel_list_entries(&app, &entries);

        assert_eq!(
            rows,
            vec![
                AgentPanelListEntry::SpaceHeader {
                    ws_idx: three_idx,
                    indented: false,
                    foldered: false,
                    thin: false,
                },
                AgentPanelListEntry::Agent { entry_idx: 0 },
            ],
            "agent-less folders and spaces are hidden"
        );
    }

    #[test]
    fn folder_view_emits_thin_ancestor_header_for_agentless_parent() {
        let mut app = crate::app::state::AppState::test_new();
        app.workspaces = vec![
            workspace_with_worktree_space("main", Some("repo-key"), "/repo/herdr"),
            workspace_with_worktree_space("issue", Some("repo-key"), "/repo/herdr-issue"),
        ];
        app.ensure_test_terminals();
        app.agent_panel_sort = crate::app::state::AgentPanelSort::Folders;
        set_root_agent(&mut app, 1, Agent::Claude);

        let entries = agent_panel_entries(&app);
        let rows = agent_panel_list_entries(&app, &entries);

        assert_eq!(
            rows,
            vec![
                AgentPanelListEntry::SpaceHeader {
                    ws_idx: 0,
                    indented: false,
                    foldered: false,
                    thin: true,
                },
                AgentPanelListEntry::SpaceHeader {
                    ws_idx: 1,
                    indented: true,
                    foldered: false,
                    thin: false,
                },
                AgentPanelListEntry::Agent { entry_idx: 0 },
            ],
            "the agent-less parent appears as a thin ancestor header"
        );
    }

    #[test]
    fn folder_view_hides_agentless_child_under_agent_bearing_parent() {
        let mut app = crate::app::state::AppState::test_new();
        app.workspaces = vec![
            workspace_with_worktree_space("main", Some("repo-key"), "/repo/herdr"),
            workspace_with_worktree_space("issue", Some("repo-key"), "/repo/herdr-issue"),
        ];
        app.ensure_test_terminals();
        app.agent_panel_sort = crate::app::state::AgentPanelSort::Folders;
        set_root_agent(&mut app, 0, Agent::Claude);

        let entries = agent_panel_entries(&app);
        let rows = agent_panel_list_entries(&app, &entries);

        assert_eq!(
            rows,
            vec![
                AgentPanelListEntry::SpaceHeader {
                    ws_idx: 0,
                    indented: false,
                    foldered: false,
                    thin: false,
                },
                AgentPanelListEntry::Agent { entry_idx: 0 },
            ],
            "an agent-less child never earns a header"
        );
    }

    /// Shared setup for the collapse projection tests: loose "notes" followed
    /// by folder "work" containing "main", one agent per space. Canonical
    /// order after assign: notes, [work: main] — ws_idx 0 = notes, 1 = main.
    fn collapse_projection_state() -> (crate::app::state::AppState, String) {
        let mut app = crate::app::state::AppState::test_new();
        app.workspaces = vec![Workspace::test_new("main"), Workspace::test_new("notes")];
        let member = app.workspaces[0].id.clone();
        let folder_id = app.create_folder("work").expect("create folder");
        app.assign_workspace_to_folder(&member, Some(&folder_id), None)
            .expect("assign");
        app.ensure_test_terminals();
        for ws_idx in 0..app.workspaces.len() {
            set_root_agent(&mut app, ws_idx, Agent::Claude);
        }
        app.mode = Mode::Terminal;
        app.active = Some(0);
        app.selected = 0;
        app.agent_panel_sort = crate::app::state::AgentPanelSort::Folders;
        (app, folder_id)
    }

    #[test]
    fn folder_view_collapsed_folder_hides_members_but_keeps_header() {
        let (mut app, folder_id) = collapse_projection_state();
        app.collapsed_folder_ids.insert(folder_id);

        let entries = agent_panel_entries(&app);
        let rows = agent_panel_list_entries(&app, &entries);

        assert_eq!(
            rows,
            vec![
                AgentPanelListEntry::SpaceHeader {
                    ws_idx: 0,
                    indented: false,
                    foldered: false,
                    thin: false,
                },
                AgentPanelListEntry::Agent { entry_idx: 0 },
                AgentPanelListEntry::FolderHeader { order_idx: 1 },
            ],
            "a collapsed folder keeps its header and hides its members"
        );
        assert_eq!(
            entries.len(),
            2,
            "hidden agents stay in the flat agent sequence"
        );
    }

    #[test]
    fn folder_view_collapsed_folder_hides_active_space_members() {
        let (mut app, folder_id) = collapse_projection_state();
        app.active = Some(1);
        app.collapsed_folder_ids.insert(folder_id);

        let entries = agent_panel_entries(&app);
        let rows = agent_panel_list_entries(&app, &entries);

        assert_eq!(
            rows,
            vec![
                AgentPanelListEntry::SpaceHeader {
                    ws_idx: 0,
                    indented: false,
                    foldered: false,
                    thin: false,
                },
                AgentPanelListEntry::Agent { entry_idx: 0 },
                AgentPanelListEntry::FolderHeader { order_idx: 1 },
            ],
            "a collapsed folder hides all members, the active space included"
        );
        assert_eq!(
            entries.len(),
            2,
            "hidden agents stay in the flat agent sequence"
        );
    }

    #[test]
    fn folder_view_hides_agentless_collapsed_folder() {
        let mut app = crate::app::state::AppState::test_new();
        app.workspaces = vec![Workspace::test_new("main"), Workspace::test_new("notes")];
        let member = app.workspaces[0].id.clone();
        let folder_id = app.create_folder("work").expect("create folder");
        app.assign_workspace_to_folder(&member, Some(&folder_id), None)
            .expect("assign");
        // Canonical order: notes, [work: main]
        app.ensure_test_terminals();
        app.agent_panel_sort = crate::app::state::AgentPanelSort::Folders;
        // Only the loose space has an agent.
        set_root_agent(&mut app, 0, Agent::Claude);
        app.collapsed_folder_ids.insert(folder_id);

        let entries = agent_panel_entries(&app);
        let rows = agent_panel_list_entries(&app, &entries);

        assert_eq!(
            rows,
            vec![
                AgentPanelListEntry::SpaceHeader {
                    ws_idx: 0,
                    indented: false,
                    foldered: false,
                    thin: false,
                },
                AgentPanelListEntry::Agent { entry_idx: 0 },
            ],
            "an agent-less folder stays hidden even while collapsed"
        );
    }

    #[test]
    fn folder_view_collapsed_space_hides_agents_but_keeps_header() {
        let (mut app, _folder_id) = collapse_projection_state();
        let main_id = app.workspaces[1].id.clone();
        app.collapsed_agent_space_ids.insert(main_id);

        let entries = agent_panel_entries(&app);
        let rows = agent_panel_list_entries(&app, &entries);

        assert_eq!(
            rows,
            vec![
                AgentPanelListEntry::SpaceHeader {
                    ws_idx: 0,
                    indented: false,
                    foldered: false,
                    thin: false,
                },
                AgentPanelListEntry::Agent { entry_idx: 0 },
                AgentPanelListEntry::FolderHeader { order_idx: 1 },
                AgentPanelListEntry::SpaceHeader {
                    ws_idx: 1,
                    indented: false,
                    foldered: true,
                    thin: false,
                },
            ],
            "a collapsed space keeps its header and hides its agent list"
        );
        assert_eq!(
            entries.len(),
            2,
            "hidden agents stay in the flat agent sequence"
        );
    }

    #[test]
    fn folder_view_collapsed_space_hides_active_pane_row() {
        let mut ws = Workspace::test_new("one");
        let root = ws.tabs[0].root_pane;
        let second = ws.test_split(ratatui::layout::Direction::Horizontal);
        ws.tabs[0].layout.focus_pane(root);

        let mut app = crate::app::state::AppState::test_new();
        app.workspaces = vec![ws];
        app.ensure_test_terminals();
        app.mode = Mode::Terminal;
        app.active = Some(0);
        app.agent_panel_sort = crate::app::state::AgentPanelSort::Folders;
        for pane in [root, second] {
            let terminal_id = app.workspaces[0].tabs[0].panes[&pane]
                .attached_terminal_id
                .clone();
            app.terminals.get_mut(&terminal_id).unwrap().detected_agent = Some(Agent::Claude);
        }
        let ws_id = app.workspaces[0].id.clone();
        app.collapsed_agent_space_ids.insert(ws_id);

        let entries = agent_panel_entries(&app);
        let rows = agent_panel_list_entries(&app, &entries);

        assert_eq!(
            rows,
            vec![AgentPanelListEntry::SpaceHeader {
                ws_idx: 0,
                indented: false,
                foldered: false,
                thin: false,
            }],
            "a collapsed space hides all agent rows, the active pane's included"
        );
        assert_eq!(
            entries.len(),
            2,
            "hidden agents stay in the flat agent sequence"
        );
    }

    #[test]
    fn flat_agent_sequence_is_identical_under_every_collapse_combination() {
        // A worktree family (main + issue) inside folder "work", plus loose
        // "notes": every collapse dimension has something real to hide.
        let mut app = crate::app::state::AppState::test_new();
        app.workspaces = vec![
            workspace_with_worktree_space("main", Some("repo-key"), "/repo/herdr"),
            workspace_with_worktree_space("issue", Some("repo-key"), "/repo/herdr-issue"),
            Workspace::test_new("notes"),
        ];
        let parent = app.workspaces[0].id.clone();
        let folder_id = app.create_folder("work").expect("create folder");
        app.assign_workspace_to_folder(&parent, Some(&folder_id), None)
            .expect("assign family");
        // Canonical order after assign: notes, [work: main, issue]
        app.ensure_test_terminals();
        for ws_idx in 0..app.workspaces.len() {
            set_root_agent(&mut app, ws_idx, Agent::Claude);
        }
        app.mode = Mode::Terminal;
        app.active = Some(0);
        app.selected = 0;
        app.agent_panel_sort = crate::app::state::AgentPanelSort::Folders;
        let main_id = app.workspaces[1].id.clone();

        let baseline: Vec<_> = agent_panel_entries(&app)
            .iter()
            .map(|entry| (entry.ws_idx, entry.pane_id))
            .collect();

        for combo in 0u8..8 {
            app.collapsed_folder_ids.clear();
            app.collapsed_agent_space_ids.clear();
            app.collapsed_space_keys.clear();
            if combo & 1 != 0 {
                app.collapsed_folder_ids.insert(folder_id.clone());
            }
            if combo & 2 != 0 {
                app.collapsed_agent_space_ids.insert(main_id.clone());
            }
            if combo & 4 != 0 {
                app.collapsed_space_keys.insert("repo-key".into());
            }
            let sequence: Vec<_> = agent_panel_entries(&app)
                .iter()
                .map(|entry| (entry.ws_idx, entry.pane_id))
                .collect();
            assert_eq!(
                sequence, baseline,
                "collapse must never filter the flat agent sequence (combo {combo:#05b})"
            );
        }
    }

    /// Deterministic xorshift* PRNG for the mirroring property sweep: no
    /// rand dependency, and failures stay reproducible by seed.
    struct PropertyRng(u64);

    impl PropertyRng {
        fn new(seed: u64) -> Self {
            Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
        }

        fn next(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x >> 12;
            x ^= x << 25;
            x ^= x >> 27;
            self.0 = x;
            x.wrapping_mul(0x2545_F491_4F6C_DD1D)
        }

        fn below(&mut self, n: usize) -> usize {
            if n == 0 {
                return 0;
            }
            (self.next() % n as u64) as usize
        }

        fn chance(&mut self, percent: u64) -> bool {
            self.next() % 100 < percent
        }
    }

    /// Build an arbitrary valid organization state from `seed`: a mix of
    /// loose plain/git spaces and worktree families, folders (some left
    /// empty), membership and positions driven through the real mutation
    /// seams, agents on a random subset of spaces, and a random collapse
    /// combination across all three collapse dimensions. The `chance`
    /// percentages are mix knobs tuned so every shape shows up often across
    /// the sweep; the vacuity guards in the property test keep them honest.
    fn arbitrary_organization_state(seed: u64) -> crate::app::state::AppState {
        let mut rng = PropertyRng::new(seed);
        let mut app = crate::app::state::AppState::test_new();

        let mut workspaces = Vec::new();
        let mut family_keys = Vec::new();
        for unit in 0..1 + rng.below(6) {
            match rng.below(3) {
                0 => {
                    let key = format!("family-{unit}");
                    workspaces.push(workspace_with_worktree_space(
                        "main",
                        Some(&key),
                        &format!("/repo/{key}"),
                    ));
                    for child in 0..1 + rng.below(3) {
                        workspaces.push(workspace_with_worktree_space(
                            &format!("{key}-child-{child}"),
                            Some(&key),
                            &format!("/repo/{key}-{child}"),
                        ));
                    }
                    family_keys.push(key);
                }
                1 => workspaces.push(workspace_with_git_space(
                    &format!("git-{unit}"),
                    &format!("git-key-{unit}"),
                )),
                _ => workspaces.push(Workspace::test_new(&format!("plain-{unit}"))),
            }
        }
        app.workspaces = workspaces;

        let mut folder_ids = Vec::new();
        for folder in 0..rng.below(4) {
            folder_ids.push(
                app.create_folder(&format!("folder-{folder}"))
                    .expect("create folder"),
            );
        }
        for _ in 0..rng.below(12) {
            let ws_id = app.workspaces[rng.below(app.workspaces.len())].id.clone();
            let folder_id = if folder_ids.is_empty() || rng.chance(30) {
                None
            } else {
                Some(folder_ids[rng.below(folder_ids.len())].clone())
            };
            let position = rng
                .chance(60)
                .then(|| rng.below(app.workspaces.len() + folder_ids.len() + 1));
            app.assign_workspace_to_folder(&ws_id, folder_id.as_deref(), position)
                .expect("assign workspace");
        }
        for _ in 0..rng.below(3) {
            if folder_ids.is_empty() {
                break;
            }
            let folder_id = folder_ids[rng.below(folder_ids.len())].clone();
            let position = rng.below(app.space_order.len() + 1);
            app.move_folder(&folder_id, position).expect("move folder");
        }

        for folder_id in &folder_ids {
            if rng.chance(35) {
                app.collapsed_folder_ids.insert(folder_id.clone());
            }
        }
        for key in &family_keys {
            if rng.chance(35) {
                app.collapsed_space_keys.insert(key.clone());
            }
        }

        app.ensure_test_terminals();
        for ws_idx in 0..app.workspaces.len() {
            if rng.chance(60) {
                set_root_agent(&mut app, ws_idx, Agent::Claude);
            }
            if rng.chance(35) {
                let ws_id = app.workspaces[ws_idx].id.clone();
                app.collapsed_agent_space_ids.insert(ws_id);
            }
        }

        app.mode = if rng.chance(50) {
            Mode::Navigate
        } else {
            Mode::Terminal
        };
        app.active = Some(rng.below(app.workspaces.len()));
        app.selected = rng.below(app.workspaces.len());
        app.agent_panel_sort = crate::app::state::AgentPanelSort::Folders;
        app
    }

    /// Ticket-10 mirroring property: for arbitrary organization states —
    /// folders, loose spaces, worktree families, and collapse combinations —
    /// the spaces panel's order projected to agent-bearing spaces equals the
    /// folder view's flat order, and no collapse combination changes the
    /// flat agent sequence.
    #[test]
    fn folder_view_mirrors_spaces_panel_for_arbitrary_organization_states() {
        // Vacuity guards: the sweep must actually exercise foldered
        // agent-bearing spaces, worktree families, and every collapse
        // dimension.
        let mut saw_foldered_agents = false;
        let mut saw_family = false;
        let mut saw_folder_collapse = false;
        let mut saw_agent_space_collapse = false;
        let mut saw_group_collapse = false;
        let flat_sequence = |entries: &[AgentPanelEntry]| -> Vec<(usize, PaneId)> {
            entries
                .iter()
                .map(|entry| (entry.ws_idx, entry.pane_id))
                .collect()
        };
        for seed in 0..300 {
            let mut app = arbitrary_organization_state(seed);
            app.assert_invariants_for_test();

            let entries = agent_panel_entries(&app);
            let agent_bearing: std::collections::HashSet<usize> =
                entries.iter().map(|entry| entry.ws_idx).collect();
            saw_foldered_agents |= app.space_order.iter().any(|entry| {
                matches!(
                    entry,
                    crate::folder::SpaceOrderEntry::Folder(folder)
                        if folder.members.iter().any(|member| {
                            app.workspaces
                                .iter()
                                .position(|ws| &ws.id == member)
                                .is_some_and(|ws_idx| agent_bearing.contains(&ws_idx))
                        })
                )
            });
            saw_family |= app
                .workspaces
                .iter()
                .any(|ws| ws.worktree_space().is_some());
            saw_folder_collapse |= !app.collapsed_folder_ids.is_empty();
            saw_agent_space_collapse |= !app.collapsed_agent_space_ids.is_empty();
            saw_group_collapse |= !app.collapsed_space_keys.is_empty();

            // The spaces panel's fully expanded visual order, projected to
            // agent-bearing spaces.
            let spaces_panel_order: Vec<usize> = workspace_list_entries_expanded(&app)
                .iter()
                .filter_map(|entry| match entry {
                    WorkspaceListEntry::Workspace { ws_idx, .. }
                        if agent_bearing.contains(ws_idx) =>
                    {
                        Some(*ws_idx)
                    }
                    _ => None,
                })
                .collect();

            // The folder view's flat agent order, one run per space. Equality
            // with the projection (each space exactly once) also proves every
            // space's agents stay contiguous.
            let mut folder_view_order: Vec<usize> = Vec::new();
            for entry in &entries {
                if folder_view_order.last() != Some(&entry.ws_idx) {
                    folder_view_order.push(entry.ws_idx);
                }
            }
            assert_eq!(
                folder_view_order, spaces_panel_order,
                "both panels must tell the same story (seed {seed})"
            );

            // Display rows only ever hide agents: under every collapse
            // combination the visible agent rows stay a subsequence of the
            // flat sequence.
            let rows = agent_panel_list_entries(&app, &entries);
            let visible_agents: Vec<usize> = rows
                .iter()
                .filter_map(|row| match row {
                    AgentPanelListEntry::Agent { entry_idx } => Some(*entry_idx),
                    _ => None,
                })
                .collect();
            assert!(
                visible_agents.windows(2).all(|pair| pair[0] < pair[1]),
                "visible agent rows must follow the flat sequence (seed {seed})"
            );

            // Collapse never filters the flat sequence.
            let with_collapse = flat_sequence(&entries);
            app.collapsed_folder_ids.clear();
            app.collapsed_agent_space_ids.clear();
            app.collapsed_space_keys.clear();
            let without_collapse = flat_sequence(&agent_panel_entries(&app));
            assert_eq!(
                with_collapse, without_collapse,
                "collapse must never change the flat agent sequence (seed {seed})"
            );
        }
        assert!(
            saw_foldered_agents,
            "sweep never foldered an agent-bearing space"
        );
        assert!(saw_family, "sweep never generated a worktree family");
        assert!(
            saw_folder_collapse,
            "sweep never generated folder collapse state"
        );
        assert!(
            saw_agent_space_collapse,
            "sweep never generated agent-list collapse state"
        );
        assert!(
            saw_group_collapse,
            "sweep never generated worktree-group collapse state"
        );
    }

    #[test]
    fn hidden_agent_entry_scrolls_to_its_nearest_visible_header() {
        let (mut app, folder_id) = collapse_projection_state();
        let main_id = app.workspaces[1].id.clone();

        // Collapsed space: the hidden agent maps to its space header row.
        app.collapsed_agent_space_ids.insert(main_id);
        assert_eq!(agent_panel_row_for_entry(&app, 1), 3);

        // Collapsed folder: the hidden agent maps to the folder header row.
        app.collapsed_agent_space_ids.clear();
        app.collapsed_folder_ids.insert(folder_id);
        assert_eq!(agent_panel_row_for_entry(&app, 1), 2);

        // Even the active space's agent maps to the folder header once the
        // folder hides all members.
        app.active = Some(1);
        assert_eq!(agent_panel_row_for_entry(&app, 1), 2);
    }

    #[test]
    fn grouped_and_priority_orderings_project_flat_agent_rows() {
        let mut app = crate::app::state::AppState::test_new();
        app.workspaces = vec![Workspace::test_new("one"), Workspace::test_new("two")];
        app.create_folder("work").expect("create folder");
        app.ensure_test_terminals();
        for ws_idx in 0..app.workspaces.len() {
            set_root_agent(&mut app, ws_idx, Agent::Claude);
        }

        for sort in [
            crate::app::state::AgentPanelSort::Spaces,
            crate::app::state::AgentPanelSort::Priority,
        ] {
            app.agent_panel_sort = sort;
            let entries = agent_panel_entries(&app);
            let rows = agent_panel_list_entries(&app, &entries);
            assert_eq!(
                rows,
                vec![
                    AgentPanelListEntry::Agent { entry_idx: 0 },
                    AgentPanelListEntry::Agent { entry_idx: 1 },
                ],
                "non-folder orderings stay flat"
            );
        }
    }

    #[test]
    fn folder_view_scroll_metrics_count_display_rows() {
        let mut app = crate::app::state::AppState::test_new();
        app.workspaces = vec![
            Workspace::test_new("one"),
            Workspace::test_new("two"),
            Workspace::test_new("three"),
        ];
        app.ensure_test_terminals();
        app.sidebar_agents.rows = vec![vec![
            crate::config::AgentSidebarToken::StateIcon,
            crate::config::AgentSidebarToken::Workspace,
        ]];
        app.sidebar_agents.row_gap = 0;
        for ws_idx in 0..app.workspaces.len() {
            set_root_agent(&mut app, ws_idx, Agent::Claude);
        }

        // Header (3 rows) + body: total 7 leaves a 4-row body.
        let area = Rect::new(0, 0, 24, 7);

        app.agent_panel_sort = crate::app::state::AgentPanelSort::Spaces;
        let grouped = agent_panel_scroll_metrics(&app, area);
        assert_eq!(grouped.max_offset_from_bottom, 0, "3 agents fit in 4 rows");

        app.agent_panel_sort = crate::app::state::AgentPanelSort::Folders;
        let folder_view = agent_panel_scroll_metrics(&app, area);
        // 3 space headers + 3 agents = 6 display rows in a 4-row body.
        assert_eq!(folder_view.max_offset_from_bottom, 2);
        assert_eq!(folder_view.viewport_rows, 4);
    }

    #[test]
    fn agent_panel_row_for_entry_maps_flat_entries_to_folder_view_rows() {
        let mut app = crate::app::state::AppState::test_new();
        app.workspaces = vec![Workspace::test_new("one"), Workspace::test_new("two")];
        app.ensure_test_terminals();
        for ws_idx in 0..app.workspaces.len() {
            set_root_agent(&mut app, ws_idx, Agent::Claude);
        }

        assert_eq!(agent_panel_row_for_entry(&app, 1), 1);

        app.agent_panel_sort = crate::app::state::AgentPanelSort::Folders;
        // Rows: header(one), agent 0, header(two), agent 1.
        assert_eq!(agent_panel_row_for_entry(&app, 0), 1);
        assert_eq!(agent_panel_row_for_entry(&app, 1), 3);
    }

    /// The compact sidebar numbers agents by flat list position, so with the
    /// folder view active its order follows the spaces-panel hoisting rather
    /// than the raw workspace order.
    #[test]
    fn collapsed_sidebar_numbers_folder_view_agents_by_list_position() {
        let mut app = crate::app::state::AppState::test_new();
        app.workspaces = vec![
            workspace_with_worktree_space("main", Some("repo-key"), "/repo/herdr"),
            workspace_with_git_space("normal", "other-key"),
            workspace_with_worktree_space("issue", Some("repo-key"), "/repo/herdr-issue"),
        ];
        app.ensure_test_terminals();
        app.agent_panel_sort = crate::app::state::AgentPanelSort::Folders;
        app.status_indicators = crate::config::StatusIndicatorStyle::Symbols;

        let set_state = |app: &mut crate::app::state::AppState, ws_idx: usize, state| {
            let pane_id = app.workspaces[ws_idx].tabs[0].root_pane;
            let terminal_id = app.workspaces[ws_idx].tabs[0].panes[&pane_id]
                .attached_terminal_id
                .clone();
            let terminal = app.terminals.get_mut(&terminal_id).unwrap();
            terminal.detected_agent = Some(Agent::Claude);
            terminal.state = state;
        };
        set_state(&mut app, 0, AgentState::Working);
        set_state(&mut app, 1, AgentState::Idle);
        set_state(&mut app, 2, AgentState::Blocked);

        // Folder view hoists the family child (blocked) above the loose
        // space (idle); grouped order would interleave them the other way.
        let order: Vec<usize> = agent_panel_entries(&app)
            .iter()
            .map(|entry| entry.ws_idx)
            .collect();
        assert_eq!(order, [0, 2, 1]);

        let area = Rect::new(0, 0, 4, 16);
        let (_, _, detail_area) = collapsed_sidebar_sections(area);
        let mut terminal = Terminal::new(TestBackend::new(area.width, area.height))
            .expect("test terminal should initialize");

        terminal
            .draw(|frame| render_sidebar_collapsed(&app, frame, area))
            .expect("collapsed sidebar should render");

        let buffer = terminal.backend().buffer();
        assert_eq!(buffer[(detail_area.x, detail_area.y)].symbol(), "1");
        assert_eq!(buffer[(detail_area.x, detail_area.y + 1)].symbol(), "2");
        assert_eq!(buffer[(detail_area.x, detail_area.y + 2)].symbol(), "3");
        assert_eq!(
            buffer[(detail_area.x + 2, detail_area.y + 1)].symbol(),
            "×",
            "position 2 is the hoisted blocked family child"
        );
    }

    #[test]
    fn in_folder_drag_indicators_start_at_the_folder_name_column() {
        let mut app = AppState::test_new();
        app.workspaces = vec![
            Workspace::test_new("a"),
            Workspace::test_new("b"),
            Workspace::test_new("c"),
        ];
        let folder_id = app.create_folder("work").expect("create folder");
        for name in ["b", "c"] {
            let id = app
                .workspaces
                .iter()
                .find(|ws| ws.display_name() == name)
                .expect("workspace exists")
                .id
                .clone();
            app.assign_workspace_to_folder(&id, Some(&folder_id), None)
                .expect("assign member");
        }
        app.sidebar_spaces.rows = vec![vec![crate::config::SpaceSidebarToken::Workspace]];
        app.sidebar_spaces.row_gap = 0;
        let area = Rect::new(0, 0, 30, 20);
        let (cards, headers) = compute_workspace_list_areas(&app, area);
        app.view.workspace_card_areas = cards;
        app.view.folder_header_areas = headers;
        let list_area = workspace_list_rect(area, app.sidebar_section_split);
        let c_idx = app
            .workspaces
            .iter()
            .position(|ws| ws.display_name() == "c")
            .expect("workspace exists");
        let targets = [
            crate::app::state::WorkspaceDropTarget::InFolderEnd {
                folder_id: folder_id.clone(),
            },
            crate::app::state::WorkspaceDropTarget::InFolderBefore {
                folder_id: folder_id.clone(),
                ws_idx: c_idx,
            },
        ];

        for target in targets {
            let indicator_row = workspace_drop_indicator_row(
                &app,
                &app.view.workspace_card_areas,
                &app.view.folder_header_areas,
                list_area,
                &target,
            )
            .unwrap();
            app.drag = Some(crate::app::state::DragState {
                target: crate::app::state::DragTarget::WorkspaceReorder {
                    source_id: 0,
                    source_ws_idx: 0,
                    drop_target: Some(target.clone()),
                },
            });

            let mut terminal = Terminal::new(TestBackend::new(area.width, area.height)).unwrap();
            terminal
                .draw(|frame| {
                    render_workspace_list(
                        &app,
                        &TerminalRuntimeRegistry::new(),
                        frame,
                        list_area,
                        false,
                    )
                })
                .unwrap();

            // The insertion line starts at the folder name's column, not the
            // panel edge, so it reads as inside the folder.
            let buffer = terminal.backend().buffer();
            for x in 0..3 {
                assert_ne!(
                    buffer[(list_area.x + x, indicator_row)].symbol(),
                    "─",
                    "column {x} should stay clear for {target:?}"
                );
            }
            assert_eq!(
                buffer[(list_area.x + 3, indicator_row)].symbol(),
                "─",
                "line should start at the folder name column for {target:?}"
            );
        }
    }

    #[test]
    fn cramped_list_keeps_trailing_top_level_slot_over_end_of_folder() {
        let mut app = AppState::test_new();
        app.workspaces = vec![Workspace::test_new("a"), Workspace::test_new("b")];
        let folder_id = app.create_folder("work").expect("create folder");
        for ws_idx in [0, 1] {
            let id = app.workspaces[ws_idx].id.clone();
            app.assign_workspace_to_folder(&id, Some(&folder_id), None)
                .expect("assign member");
        }
        app.sidebar_spaces.rows = vec![vec![crate::config::SpaceSidebarToken::Workspace]];
        app.sidebar_spaces.row_gap = 0;
        let area = Rect::new(0, 0, 30, 20);
        let (cards, headers) = compute_workspace_list_areas(&app, area);
        let last = cards.last().expect("folder members visible").rect;
        let last_bottom = last.y + last.height;
        let list_area = workspace_list_rect(area, app.sidebar_section_split);

        // With room for two rows below the folder, both slots exist.
        let roomy = workspace_drop_slots(&app, &cards, &headers, list_area);
        assert!(roomy.contains(&(
            crate::app::state::WorkspaceDropTarget::InFolderEnd {
                folder_id: folder_id.clone(),
            },
            last_bottom,
        )));
        assert!(roomy.contains(&(crate::app::state::WorkspaceDropTarget::End, last_bottom + 1,)));

        // With only one free row left, the trailing top-level slot wins so
        // ejecting from the folder always stays reachable (the header drop
        // still appends).
        let cramped = Rect::new(
            list_area.x,
            list_area.y,
            list_area.width,
            last_bottom + 2 - list_area.y,
        );
        let slots = workspace_drop_slots(&app, &cards, &headers, cramped);
        assert!(slots.contains(&(crate::app::state::WorkspaceDropTarget::End, last_bottom)));
        assert!(!slots.iter().any(|(target, _)| matches!(
            target,
            crate::app::state::WorkspaceDropTarget::InFolderEnd { .. }
        )));
    }

    #[test]
    fn workspace_list_entries_nest_folder_members_under_header() {
        let mut app = AppState::test_new();
        app.workspaces = vec![
            Workspace::test_new("one"),
            Workspace::test_new("two"),
            Workspace::test_new("three"),
        ];
        let target = app.workspaces[0].id.clone();
        let folder_id = app.create_folder("work").expect("create folder");
        app.assign_workspace_to_folder(&target, Some(&folder_id), None)
            .expect("assign");
        // Canonical order after assign: two, three, [work: one]

        let entries = workspace_list_entries(&app);

        assert_eq!(
            entries,
            vec![
                WorkspaceListEntry::Workspace {
                    ws_idx: 0,
                    indented: false,
                    foldered: false,
                },
                WorkspaceListEntry::Workspace {
                    ws_idx: 1,
                    indented: false,
                    foldered: false,
                },
                WorkspaceListEntry::FolderHeader { order_idx: 2 },
                WorkspaceListEntry::Workspace {
                    ws_idx: 2,
                    indented: false,
                    foldered: true,
                },
            ]
        );
        assert!(matches!(
            app.space_order.get(2),
            Some(crate::folder::SpaceOrderEntry::Folder(folder)) if folder.id == folder_id
        ));
    }

    #[test]
    fn workspace_list_entries_render_empty_folder_header() {
        let mut app = AppState::test_new();
        app.workspaces = vec![Workspace::test_new("one")];
        app.create_folder("empty").expect("create folder");

        let entries = workspace_list_entries(&app);

        assert_eq!(
            entries,
            vec![
                WorkspaceListEntry::Workspace {
                    ws_idx: 0,
                    indented: false,
                    foldered: false,
                },
                WorkspaceListEntry::FolderHeader { order_idx: 1 },
            ]
        );
    }

    #[test]
    fn workspace_list_entries_nest_worktree_family_inside_folder() {
        let mut app = AppState::test_new();
        app.workspaces = vec![
            workspace_with_worktree_space("main", Some("repo-key"), "/repo/herdr"),
            workspace_with_worktree_space("issue", Some("repo-key"), "/repo/herdr-issue"),
            Workspace::test_new("notes"),
        ];
        let parent = app.workspaces[0].id.clone();
        let folder_id = app.create_folder("work").expect("create folder");
        app.assign_workspace_to_folder(&parent, Some(&folder_id), None)
            .expect("assign family");
        // Canonical order after assign: notes, [work: main, issue]

        let entries = workspace_list_entries(&app);

        assert_eq!(
            entries,
            vec![
                WorkspaceListEntry::Workspace {
                    ws_idx: 0,
                    indented: false,
                    foldered: false,
                },
                WorkspaceListEntry::FolderHeader { order_idx: 1 },
                WorkspaceListEntry::Workspace {
                    ws_idx: 1,
                    indented: false,
                    foldered: true,
                },
                WorkspaceListEntry::Workspace {
                    ws_idx: 2,
                    indented: true,
                    foldered: true,
                },
            ]
        );
    }

    #[test]
    fn foldered_space_card_renders_same_content_as_loose_card() {
        let mut app = AppState::test_new();
        app.workspaces = vec![Workspace::test_new("alpha"), Workspace::test_new("alpha")];
        for ws in &mut app.workspaces {
            ws.cached_git_branch = Some("feature/x".into());
        }
        let foldered = app.workspaces[1].id.clone();
        let folder_id = app.create_folder("work").expect("create folder");
        app.assign_workspace_to_folder(&foldered, Some(&folder_id), None)
            .expect("assign");
        app.sidebar_spaces.rows = vec![
            vec![
                crate::config::SpaceSidebarToken::StateIcon,
                crate::config::SpaceSidebarToken::Workspace,
            ],
            vec![crate::config::SpaceSidebarToken::Branch],
        ];
        app.sidebar_spaces.row_gap = 0;
        let area = Rect::new(0, 0, 30, 20);
        let (cards, headers) = compute_workspace_list_areas(&app, area);
        app.view.workspace_card_areas = cards;
        app.view.folder_header_areas = headers;
        let list_area = workspace_list_rect(area, app.sidebar_section_split);

        let mut terminal = Terminal::new(TestBackend::new(area.width, area.height)).unwrap();
        terminal
            .draw(|frame| {
                render_workspace_list(
                    &app,
                    &TerminalRuntimeRegistry::new(),
                    frame,
                    list_area,
                    false,
                )
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        let cards = &app.view.workspace_card_areas;
        assert_eq!(cards.len(), 2, "both space cards must be visible");
        let row_text = |y: u16| -> String {
            (0..area.width)
                .map(|x| buffer[(x, y)].symbol().to_string())
                .collect::<String>()
        };
        // Every card row — name and git information — must carry identical
        // content, differing only in the folder-nesting margin.
        assert!(cards[0].rect.height >= 2, "cards must include the git row");
        assert_eq!(cards[0].rect.height, cards[1].rect.height);
        for row in 0..cards[0].rect.height {
            let loose_row = row_text(cards[0].rect.y + row);
            let foldered_row = row_text(cards[1].rect.y + row);
            assert_eq!(
                foldered_row.trim(),
                loose_row.trim(),
                "foldered card row {row} content must match the loose card"
            );
        }
        assert!(
            find_symbol_x(buffer, cards[1].rect.y, area.width, "a")
                > find_symbol_x(buffer, cards[0].rect.y, area.width, "a"),
            "foldered card must be indented under its folder header"
        );
        let headers = &app.view.folder_header_areas;
        assert_eq!(headers.len(), 1, "folder header must be visible");
        let header_row = row_text(headers[0].rect.y);
        assert!(
            header_row.contains("work"),
            "folder header row must show the folder name: {header_row:?}"
        );
    }

    #[test]
    fn collapsed_folder_hides_members_and_expanding_restores_them() {
        let mut app = AppState::test_new();
        app.workspaces = vec![Workspace::test_new("one"), Workspace::test_new("two")];
        let member = app.workspaces[1].id.clone();
        let folder_id = app.create_folder("work").expect("create folder");
        app.assign_workspace_to_folder(&member, Some(&folder_id), None)
            .expect("assign");
        // Canonical order: one, [work: two]
        app.active = None;
        app.mode = Mode::Terminal;

        app.collapsed_folder_ids.insert(folder_id.clone());
        assert_eq!(
            workspace_list_entries(&app),
            vec![
                WorkspaceListEntry::Workspace {
                    ws_idx: 0,
                    indented: false,
                    foldered: false,
                },
                WorkspaceListEntry::FolderHeader { order_idx: 1 },
            ]
        );

        app.collapsed_folder_ids.remove(&folder_id);
        assert_eq!(
            workspace_list_entries(&app),
            vec![
                WorkspaceListEntry::Workspace {
                    ws_idx: 0,
                    indented: false,
                    foldered: false,
                },
                WorkspaceListEntry::FolderHeader { order_idx: 1 },
                WorkspaceListEntry::Workspace {
                    ws_idx: 1,
                    indented: false,
                    foldered: true,
                },
            ]
        );
    }

    #[test]
    fn collapsed_folder_hides_active_member() {
        let mut app = AppState::test_new();
        app.workspaces = vec![Workspace::test_new("one"), Workspace::test_new("two")];
        let member = app.workspaces[1].id.clone();
        let folder_id = app.create_folder("work").expect("create folder");
        app.assign_workspace_to_folder(&member, Some(&folder_id), None)
            .expect("assign");
        // Canonical order: one, [work: two]
        app.active = Some(1);
        app.mode = Mode::Terminal;
        app.collapsed_folder_ids.insert(folder_id);

        assert_eq!(
            workspace_list_entries(&app),
            vec![
                WorkspaceListEntry::Workspace {
                    ws_idx: 0,
                    indented: false,
                    foldered: false,
                },
                WorkspaceListEntry::FolderHeader { order_idx: 1 },
            ],
            "a collapsed folder hides all members, the active one included"
        );
    }

    #[test]
    fn collapsed_folder_hides_selected_member_in_navigate_mode() {
        let mut app = AppState::test_new();
        app.workspaces = vec![Workspace::test_new("one"), Workspace::test_new("two")];
        let member = app.workspaces[1].id.clone();
        let folder_id = app.create_folder("work").expect("create folder");
        app.assign_workspace_to_folder(&member, Some(&folder_id), None)
            .expect("assign");
        app.mode = Mode::Navigate;
        app.selected = 1;
        app.active = None;
        app.collapsed_folder_ids.insert(folder_id);

        assert_eq!(
            workspace_list_entries(&app),
            vec![
                WorkspaceListEntry::Workspace {
                    ws_idx: 0,
                    indented: false,
                    foldered: false,
                },
                WorkspaceListEntry::FolderHeader { order_idx: 1 },
            ],
            "a collapsed folder hides all members, the selected one included"
        );
    }

    #[test]
    fn folder_collapse_is_independent_from_worktree_group_collapse() {
        let mut app = AppState::test_new();
        app.workspaces = vec![
            workspace_with_worktree_space("main", Some("repo-key"), "/repo/herdr"),
            workspace_with_worktree_space("issue", Some("repo-key"), "/repo/herdr-issue"),
            Workspace::test_new("notes"),
        ];
        let parent = app.workspaces[0].id.clone();
        let folder_id = app.create_folder("work").expect("create folder");
        app.assign_workspace_to_folder(&parent, Some(&folder_id), None)
            .expect("assign family");
        // Canonical order: notes, [work: main, issue]
        app.active = None;
        app.mode = Mode::Terminal;
        app.collapsed_space_keys.insert("repo-key".into());

        // Collapsing the folder hides the whole family; the worktree-group
        // collapse entry stays untouched.
        app.collapsed_folder_ids.insert(folder_id.clone());
        assert_eq!(
            workspace_list_entries(&app),
            vec![
                WorkspaceListEntry::Workspace {
                    ws_idx: 0,
                    indented: false,
                    foldered: false,
                },
                WorkspaceListEntry::FolderHeader { order_idx: 1 },
            ]
        );
        assert!(app.collapsed_space_keys.contains("repo-key"));

        // Expanding the folder restores the family with its own collapse
        // state still applied: parent visible, child hidden.
        app.collapsed_folder_ids.remove(&folder_id);
        assert_eq!(
            workspace_list_entries(&app),
            vec![
                WorkspaceListEntry::Workspace {
                    ws_idx: 0,
                    indented: false,
                    foldered: false,
                },
                WorkspaceListEntry::FolderHeader { order_idx: 1 },
                WorkspaceListEntry::Workspace {
                    ws_idx: 1,
                    indented: false,
                    foldered: true,
                },
            ]
        );
    }

    #[test]
    fn folder_header_renders_collapse_chevron_matching_state() {
        let mut app = AppState::test_new();
        app.workspaces = vec![Workspace::test_new("one")];
        let member = app.workspaces[0].id.clone();
        let folder_id = app.create_folder("work").expect("create folder");
        app.assign_workspace_to_folder(&member, Some(&folder_id), None)
            .expect("assign");
        app.active = None;
        app.mode = Mode::Terminal;
        let area = Rect::new(0, 0, 30, 20);

        let render = |app: &mut AppState| -> String {
            let (cards, headers) = compute_workspace_list_areas(app, area);
            app.view.workspace_card_areas = cards;
            app.view.folder_header_areas = headers;
            let list_area = workspace_list_rect(area, app.sidebar_section_split);
            let mut terminal = Terminal::new(TestBackend::new(area.width, area.height)).unwrap();
            terminal
                .draw(|frame| {
                    render_workspace_list(
                        app,
                        &TerminalRuntimeRegistry::new(),
                        frame,
                        list_area,
                        false,
                    )
                })
                .unwrap();
            let buffer = terminal.backend().buffer();
            let header = &app.view.folder_header_areas[0];
            (0..area.width)
                .map(|x| buffer[(x, header.rect.y)].symbol().to_string())
                .collect::<String>()
        };

        // Filesystem-browser layout: the chevron sits in the state-icon
        // column and the folder name aligns with loose space names.
        let expanded_row = render(&mut app);
        assert!(
            expanded_row.starts_with(" ▾ work"),
            "expanded folder header must lead with an expanded chevron: {expanded_row:?}"
        );

        app.collapsed_folder_ids.insert(folder_id);
        let collapsed_row = render(&mut app);
        assert!(
            collapsed_row.starts_with(" ▸ work"),
            "collapsed folder header must lead with a collapsed chevron: {collapsed_row:?}"
        );
    }

    #[test]
    fn expanded_entries_ignore_folder_collapse() {
        let mut app = AppState::test_new();
        app.workspaces = vec![Workspace::test_new("one"), Workspace::test_new("two")];
        let member = app.workspaces[1].id.clone();
        let folder_id = app.create_folder("work").expect("create folder");
        app.assign_workspace_to_folder(&member, Some(&folder_id), None)
            .expect("assign");
        app.active = None;
        app.mode = Mode::Terminal;
        app.collapsed_folder_ids.insert(folder_id);

        assert_eq!(
            workspace_list_entries_expanded(&app),
            vec![
                WorkspaceListEntry::Workspace {
                    ws_idx: 0,
                    indented: false,
                    foldered: false,
                },
                WorkspaceListEntry::FolderHeader { order_idx: 1 },
                WorkspaceListEntry::Workspace {
                    ws_idx: 1,
                    indented: false,
                    foldered: true,
                },
            ]
        );
    }
}
