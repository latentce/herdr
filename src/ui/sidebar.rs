mod tokens;

use ratatui::{
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use self::tokens::{ResolvedToken, ResolvedTokenKind, SpaceTokenContext};
use super::scrollbar::{render_scrollbar, should_show_scrollbar};
use super::status::{state_icon, state_label, state_label_color};
use super::text::{display_width, display_width_u16, truncate_end};
use crate::app::state::{AgentPanelSort, Palette};
use crate::app::{AppState, Mode};
use crate::detect::AgentState;
use crate::terminal::TerminalRuntimeRegistry;

const WORKSPACE_SECTION_HEADER_ROWS: u16 = 2;
const AGENT_PANEL_HEADER_ROWS: u16 = 3;
/// Height of a folder header row in the spaces panel.
const FOLDER_HEADER_ROWS: u16 = 1;

pub(crate) struct AgentPanelEntry {
    pub ws_idx: usize,
    pub tab_idx: usize,
    pub pane_id: crate::layout::PaneId,
    pub primary_label: String,
    pub primary_tab_label: Option<String>,
    pub pane_label: Option<String>,
    pub terminal_title: Option<String>,
    pub terminal_title_stripped: Option<String>,
    pub agent_label: Option<String>,
    pub agent_kind_label: Option<String>,
    pub agent: Option<crate::detect::Agent>,
    pub state: AgentState,
    pub seen: bool,
    pub last_agent_state_change_seq: Option<u64>,
    pub state_labels: std::collections::HashMap<String, String>,
    pub tokens: std::collections::HashMap<String, String>,
}

fn sidebar_section_heights(total_h: u16, split_ratio: f32) -> (u16, u16) {
    if total_h == 0 {
        return (0, 0);
    }

    if total_h < 6 {
        let ws_h = total_h.div_ceil(2);
        return (ws_h, total_h.saturating_sub(ws_h));
    }

    let ratio = split_ratio.clamp(0.1, 0.9);
    let ws_h = ((total_h as f32) * ratio).round() as u16;
    let ws_h = ws_h.clamp(3, total_h.saturating_sub(3));
    let detail_h = total_h.saturating_sub(ws_h);
    (ws_h, detail_h)
}

pub(crate) fn expanded_sidebar_sections(area: Rect, split_ratio: f32) -> (Rect, Rect) {
    let content = Rect::new(area.x, area.y, area.width.saturating_sub(1), area.height);
    if content.width == 0 || content.height == 0 {
        return (Rect::default(), Rect::default());
    }

    let (ws_h, detail_h) = sidebar_section_heights(content.height, split_ratio);
    let ws_area = Rect::new(content.x, content.y, content.width, ws_h);
    let detail_area = Rect::new(content.x, content.y + ws_h, content.width, detail_h);
    (ws_area, detail_area)
}

pub(crate) fn sidebar_section_divider_rect(area: Rect, split_ratio: f32) -> Rect {
    let content = Rect::new(area.x, area.y, area.width.saturating_sub(1), area.height);
    if content.width == 0 || content.height < 6 {
        return Rect::default();
    }

    let (ws_h, _) = sidebar_section_heights(content.height, split_ratio);
    Rect::new(content.x, content.y + ws_h, content.width, 1)
}

fn agent_panel_sort_label(sort: AgentPanelSort) -> &'static str {
    match sort {
        AgentPanelSort::Spaces => "grouped",
        AgentPanelSort::Priority => "priority",
        AgentPanelSort::Folders => "folders",
    }
}

pub(crate) fn agent_panel_toggle_rect(area: Rect, sort: AgentPanelSort) -> Rect {
    agent_panel_header_label_rect(area, agent_panel_sort_label(sort))
}

fn agent_panel_header_label_rect(area: Rect, label: &str) -> Rect {
    if area.width == 0 || area.height < 2 {
        return Rect::default();
    }

    let width = display_width_u16(label).min(area.width);
    Rect::new(
        area.x + area.width.saturating_sub(width),
        area.y + 1,
        width,
        1,
    )
}

fn active_agent_view_label(app: &AppState) -> Option<&str> {
    app.agent_view_override
        .as_ref()
        .map(|view| view.label.as_deref().unwrap_or("filtered"))
}

pub(crate) fn agent_panel_entries(app: &AppState) -> Vec<AgentPanelEntry> {
    agent_panel_entries_with_runtimes(app, None)
}

pub(crate) fn all_agent_panel_entries(app: &AppState) -> Vec<AgentPanelEntry> {
    collect_agent_panel_entries_with_runtimes(app, None)
}

pub(crate) fn agent_panel_entries_from(
    app: &AppState,
    terminal_runtimes: &TerminalRuntimeRegistry,
) -> Vec<AgentPanelEntry> {
    agent_panel_entries_with_runtimes(app, Some(terminal_runtimes))
}

fn agent_panel_entries_with_runtimes(
    app: &AppState,
    terminal_runtimes: Option<&TerminalRuntimeRegistry>,
) -> Vec<AgentPanelEntry> {
    let mut entries = collect_agent_panel_entries_with_runtimes(app, terminal_runtimes);
    crate::app::agent_view::apply_agent_view(app, &mut entries);
    entries
}

fn collect_agent_panel_entries_with_runtimes(
    app: &AppState,
    terminal_runtimes: Option<&TerminalRuntimeRegistry>,
) -> Vec<AgentPanelEntry> {
    let empty_runtimes;
    let terminal_runtimes = match terminal_runtimes {
        Some(terminal_runtimes) => terminal_runtimes,
        None => {
            empty_runtimes = TerminalRuntimeRegistry::new();
            &empty_runtimes
        }
    };

    app.workspaces
        .iter()
        .enumerate()
        .flat_map(|(ws_idx, ws)| {
            let multi_tab = ws.tabs.len() > 1;
            let workspace_label = ws.display_name_from(&app.terminals, terminal_runtimes);
            ws.pane_details(&app.terminals)
                .into_iter()
                .map(move |detail| {
                    let show_tab = multi_tab
                        || ws
                            .tabs
                            .get(detail.tab_idx)
                            .is_some_and(|tab| !tab.is_auto_named());
                    AgentPanelEntry {
                        ws_idx,
                        tab_idx: detail.tab_idx,
                        pane_id: detail.pane_id,
                        primary_label: workspace_label.clone(),
                        primary_tab_label: show_tab.then_some(detail.tab_label),
                        pane_label: detail.pane_label,
                        terminal_title: detail.terminal_title,
                        terminal_title_stripped: detail.terminal_title_stripped,
                        agent_label: Some(detail.agent_label),
                        agent_kind_label: detail.agent_kind_label,
                        agent: detail.agent,
                        state: detail.state,
                        seen: detail.seen,
                        last_agent_state_change_seq: detail.last_agent_state_change_seq,
                        state_labels: detail.state_labels,
                        tokens: detail.tokens,
                    }
                })
        })
        .collect()
}

/// A display row in the agents panel. The grouped and priority orderings
/// produce only [`AgentPanelListEntry::Agent`] rows; the folder view
/// interleaves folder and space headers mirroring the spaces panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AgentPanelListEntry {
    /// A folder header row. `order_idx` indexes into `AppState::space_order`
    /// (always a `SpaceOrderEntry::Folder` entry).
    FolderHeader { order_idx: usize },
    /// A space header row above the space's agents.
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

pub(super) fn agent_panel_status_key(state: AgentState, seen: bool) -> &'static str {
    match (state, seen) {
        (AgentState::Idle, false) => "done",
        (AgentState::Idle, true) => "idle",
        (AgentState::Working, _) => "working",
        (AgentState::Blocked, _) => "blocked",
        (AgentState::Unknown, _) => "unknown",
    }
}

fn workspace_row_height(app: &AppState, ws: &crate::workspace::Workspace, indented: bool) -> u16 {
    let (state, seen) = ws.aggregate_state(&app.terminals);
    let label = if indented {
        grouped_child_display_label(
            &ws.display_name_from_terminals(&app.terminals),
            ws.branch().as_deref(),
            ws.custom_name.is_some(),
        )
    } else {
        ws.display_name_from_terminals(&app.terminals)
    };
    let token_values = ws.metadata_tokens.values();
    tokens::space_rows(
        &app.sidebar_spaces,
        SpaceTokenContext {
            workspace: &label,
            branch: ws.branch().as_deref(),
            state_text: state_label(state, seen),
            ahead_behind: ws.git_ahead_behind(),
            tokens: &token_values,
            suppress_git_details: indented,
        },
    )
    .len()
    .max(1)
    .min(u16::MAX as usize) as u16
}

fn workspace_row_height_in_body(
    app: &AppState,
    workspace: &crate::workspace::Workspace,
    indented: bool,
    body_height: u16,
) -> u16 {
    workspace_row_height(app, workspace, indented).min(body_height)
}

fn workspace_entry_gap(app: &AppState, entries: &[WorkspaceListEntry], entry_idx: usize) -> u16 {
    if entry_idx + 1 >= entries.len() || next_entry_is_indented_workspace(entries, entry_idx) {
        return 0;
    }
    // A folder header hugs its first member.
    if matches!(
        entries.get(entry_idx),
        Some(WorkspaceListEntry::FolderHeader { .. })
    ) && matches!(
        entries.get(entry_idx + 1),
        Some(WorkspaceListEntry::Workspace { foldered: true, .. })
    ) {
        return 0;
    }
    app.sidebar_spaces.row_gap
}

fn workspace_attention_priority(state: AgentState, seen: bool) -> u8 {
    match (state, seen) {
        (AgentState::Blocked, _) => 4,
        (AgentState::Idle, false) => 3,
        (AgentState::Working, _) => 2,
        (AgentState::Idle, true) => 1,
        (AgentState::Unknown, _) => 0,
    }
}

fn space_aggregate_state(app: &AppState, key: &str) -> (AgentState, bool) {
    app.workspaces
        .iter()
        .filter(|ws| ws.worktree_space().is_some_and(|space| space.key == key))
        .map(|ws| ws.aggregate_state(&app.terminals))
        .max_by_key(|(state, seen)| workspace_attention_priority(*state, *seen))
        .unwrap_or((AgentState::Unknown, true))
}

pub(crate) fn workspace_parent_group_state(
    app: &AppState,
    ws_idx: usize,
) -> Option<(String, bool)> {
    let space = app.workspaces.get(ws_idx)?.worktree_space()?;
    if space.is_linked_worktree {
        return None;
    }
    let member_count = app
        .workspaces
        .iter()
        .filter(|ws| {
            ws.worktree_space()
                .is_some_and(|member| member.key == space.key)
        })
        .count();
    (member_count >= 2).then(|| {
        (
            space.key.clone(),
            app.collapsed_space_keys.contains(&space.key),
        )
    })
}

pub(crate) fn grouped_child_display_label(
    label: &str,
    branch: Option<&str>,
    has_custom_name: bool,
) -> String {
    if has_custom_name {
        return label.to_string();
    }
    let Some(branch) = branch else {
        return label.to_string();
    };
    branch
        .strip_prefix("worktree/")
        .unwrap_or(branch)
        .to_string()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WorkspaceListEntry {
    /// A folder header row. `order_idx` indexes into `AppState::space_order`
    /// (always a `SpaceOrderEntry::Folder` entry).
    FolderHeader { order_idx: usize },
    Workspace {
        ws_idx: usize,
        /// Indented as a worktree family child under its parent checkout.
        indented: bool,
        /// Nested under a folder header.
        foldered: bool,
    },
}

pub(crate) fn next_entry_is_indented_workspace(entries: &[WorkspaceListEntry], idx: usize) -> bool {
    matches!(
        entries.get(idx.saturating_add(1)),
        Some(WorkspaceListEntry::Workspace { indented: true, .. })
    )
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

pub(crate) fn normalized_workspace_scroll(app: &AppState, area: Rect, requested: usize) -> usize {
    let ws_area = workspace_list_rect(area, app.sidebar_section_split);
    let body = workspace_list_body_rect(ws_area, false);
    if body.height == 0 {
        return requested;
    }

    if workspace_list_entries(app).is_empty() {
        0
    } else {
        requested.min(workspace_list_bottom_start(app, ws_area))
    }
}

pub(crate) fn workspace_list_entries(app: &AppState) -> Vec<WorkspaceListEntry> {
    workspace_list_entries_inner(app, false)
}

/// Like [`workspace_list_entries`] but always expands worktree groups, ignoring
/// `collapsed_space_keys`. The mobile switcher has no collapse affordance and
/// always shows the full worktree tree.
pub(crate) fn workspace_list_entries_expanded(app: &AppState) -> Vec<WorkspaceListEntry> {
    workspace_list_entries_inner(app, true)
}

fn workspace_list_entries_inner(app: &AppState, force_expanded: bool) -> Vec<WorkspaceListEntry> {
    let mut members_by_key = std::collections::HashMap::<String, Vec<usize>>::new();
    for (ws_idx, ws) in app.workspaces.iter().enumerate() {
        if let Some(space) = ws.worktree_space() {
            members_by_key
                .entry(space.key.clone())
                .or_default()
                .push(ws_idx);
        }
    }
    let grouped_keys = members_by_key
        .iter()
        .filter(|(_, members)| {
            members.len() >= 2
                && members.iter().any(|idx| {
                    app.workspaces
                        .get(*idx)
                        .and_then(|ws| ws.worktree_space())
                        .is_some_and(|space| !space.is_linked_worktree)
                })
        })
        .map(|(key, _)| key.clone())
        .collect::<std::collections::HashSet<_>>();

    let visible_group_idx = if matches!(app.mode, Mode::Navigate) {
        Some(app.selected)
    } else {
        app.active
    };
    let active_group = visible_group_idx.and_then(|idx| {
        app.workspaces
            .get(idx)
            .and_then(|ws| ws.worktree_space())
            .map(|space| space.key.clone())
    });

    let idx_by_id: std::collections::HashMap<&str, usize> = if app.space_order.is_empty() {
        // Fast path: sessions without folders never look up ids by name.
        std::collections::HashMap::new()
    } else {
        app.workspaces
            .iter()
            .enumerate()
            .map(|(ws_idx, ws)| (ws.id.as_str(), ws_idx))
            .collect()
    };
    let mut listed = vec![false; app.workspaces.len()];
    let mut emitted_groups = std::collections::HashSet::<String>::new();
    let mut entries = Vec::new();

    // Emit one workspace at this position — hoisting its whole worktree
    // family (parent first, children indented) when it belongs to one.
    let emit_workspace = |ws_idx: usize,
                          foldered: bool,
                          emitted_groups: &mut std::collections::HashSet<String>,
                          entries: &mut Vec<WorkspaceListEntry>| {
        let Some(ws) = app.workspaces.get(ws_idx) else {
            return;
        };
        let Some(space) = ws
            .worktree_space()
            .filter(|space| grouped_keys.contains(&space.key))
        else {
            entries.push(WorkspaceListEntry::Workspace {
                ws_idx,
                indented: false,
                foldered,
            });
            return;
        };

        if !emitted_groups.insert(space.key.clone()) {
            return;
        }

        let Some(members) = members_by_key.get(&space.key) else {
            return;
        };
        let Some(parent_idx) = members.iter().copied().find(|idx| {
            app.workspaces
                .get(*idx)
                .and_then(|member| member.worktree_space())
                .is_some_and(|member_space| !member_space.is_linked_worktree)
        }) else {
            entries.push(WorkspaceListEntry::Workspace {
                ws_idx,
                indented: false,
                foldered,
            });
            return;
        };
        let collapsed = !force_expanded && app.collapsed_space_keys.contains(&space.key);
        entries.push(WorkspaceListEntry::Workspace {
            ws_idx: parent_idx,
            indented: false,
            foldered,
        });

        if collapsed {
            if let Some(active_idx) = visible_group_idx
                .filter(|idx| *idx != parent_idx)
                .filter(|_| active_group.as_deref() == Some(space.key.as_str()))
            {
                entries.push(WorkspaceListEntry::Workspace {
                    ws_idx: active_idx,
                    indented: true,
                    foldered,
                });
            }
        } else {
            for member_idx in members {
                if *member_idx == parent_idx {
                    continue;
                }
                entries.push(WorkspaceListEntry::Workspace {
                    ws_idx: *member_idx,
                    indented: true,
                    foldered,
                });
            }
        }
    };

    // Walk the explicit space order: loose spaces flat, folder members under
    // their header. Stale/duplicate references are skipped; workspaces missing
    // from the order are implicitly loose at the end.
    for (order_idx, order_entry) in app.space_order.iter().enumerate() {
        match order_entry {
            crate::folder::SpaceOrderEntry::Workspace(id) => {
                let Some(&ws_idx) = idx_by_id.get(id.as_str()) else {
                    continue;
                };
                if std::mem::replace(&mut listed[ws_idx], true) {
                    continue;
                }
                emit_workspace(ws_idx, false, &mut emitted_groups, &mut entries);
            }
            crate::folder::SpaceOrderEntry::Folder(folder) => {
                entries.push(WorkspaceListEntry::FolderHeader { order_idx });
                let folder_collapsed =
                    !force_expanded && app.collapsed_folder_ids.contains(&folder.id);
                for member in &folder.members {
                    let Some(&ws_idx) = idx_by_id.get(member.as_str()) else {
                        continue;
                    };
                    if std::mem::replace(&mut listed[ws_idx], true) {
                        continue;
                    }
                    if folder_collapsed {
                        // Unlike worktree-group collapse, a collapsed folder
                        // hides all members unconditionally; the folder
                        // header carries the active/selected highlight
                        // instead (see `collapsed_folder_header_highlight`).
                        continue;
                    }
                    emit_workspace(ws_idx, true, &mut emitted_groups, &mut entries);
                }
            }
        }
    }
    for (ws_idx, ws_listed) in listed.iter().enumerate() {
        if !ws_listed {
            emit_workspace(ws_idx, false, &mut emitted_groups, &mut entries);
        }
    }
    entries
}

pub(crate) fn workspace_list_rect(area: Rect, split_ratio: f32) -> Rect {
    let (ws_area, _) = expanded_sidebar_sections(area, split_ratio);
    ws_area
}

pub(crate) fn workspace_list_body_rect(area: Rect, has_scrollbar: bool) -> Rect {
    if area.width == 0 || area.height <= WORKSPACE_SECTION_HEADER_ROWS {
        return Rect::default();
    }

    let body_y = area.y.saturating_add(WORKSPACE_SECTION_HEADER_ROWS);
    let footer_y = area.y + area.height.saturating_sub(1);
    let body_height = footer_y.saturating_sub(body_y);
    let body_width = area.width.saturating_sub(u16::from(has_scrollbar));
    Rect::new(area.x, body_y, body_width, body_height)
}

fn workspace_list_visible_count(app: &AppState, area: Rect, scroll: usize) -> usize {
    let body = workspace_list_body_rect(area, false);
    if body.width == 0 || body.height == 0 {
        return 0;
    }

    let mut used_rows = 0u16;
    let mut visible = 0usize;
    let entries = workspace_list_entries(app);
    for (entry_idx, entry) in entries.iter().enumerate().skip(scroll) {
        let (row_height, gap) = match entry {
            WorkspaceListEntry::FolderHeader { .. } => (
                FOLDER_HEADER_ROWS.min(body.height),
                workspace_entry_gap(app, &entries, entry_idx),
            ),
            WorkspaceListEntry::Workspace {
                ws_idx, indented, ..
            } => {
                let Some(ws) = app.workspaces.get(*ws_idx) else {
                    continue;
                };
                (
                    workspace_row_height_in_body(app, ws, *indented, body.height),
                    workspace_entry_gap(app, &entries, entry_idx),
                )
            }
        };
        if used_rows.saturating_add(row_height) > body.height {
            break;
        }
        used_rows = used_rows.saturating_add(row_height);
        visible += 1;
        used_rows = used_rows.saturating_add(gap).min(body.height);
    }
    visible
}

fn workspace_list_bottom_start(app: &AppState, area: Rect) -> usize {
    let body = workspace_list_body_rect(area, false);
    let entries = workspace_list_entries(app);
    let mut used_rows = 0u16;
    let mut start = entries.len();
    for (entry_idx, entry) in entries.iter().enumerate().rev() {
        let row_height = match entry {
            WorkspaceListEntry::FolderHeader { .. } => FOLDER_HEADER_ROWS.min(body.height),
            WorkspaceListEntry::Workspace {
                ws_idx, indented, ..
            } => {
                let Some(workspace) = app.workspaces.get(*ws_idx) else {
                    continue;
                };
                workspace_row_height_in_body(app, workspace, *indented, body.height)
            }
        };
        let gap = workspace_entry_gap(app, &entries, entry_idx);
        let needed = row_height.saturating_add(gap);
        if used_rows.saturating_add(needed) > body.height {
            break;
        }
        used_rows = used_rows.saturating_add(needed);
        start = entry_idx;
    }
    start.min(entries.len().saturating_sub(1))
}

pub(crate) fn workspace_list_scroll_metrics(
    app: &AppState,
    area: Rect,
) -> crate::pane::ScrollMetrics {
    let max_scroll = workspace_list_bottom_start(app, area);
    let scroll = app.workspace_scroll.min(max_scroll);
    let viewport_rows = workspace_list_visible_count(app, area, scroll);

    crate::pane::ScrollMetrics {
        offset_from_bottom: max_scroll.saturating_sub(scroll),
        max_offset_from_bottom: max_scroll,
        viewport_rows,
    }
}

pub(crate) fn workspace_list_scrollbar_rect(app: &AppState, area: Rect) -> Option<Rect> {
    let metrics = workspace_list_scroll_metrics(app, area);
    let body = workspace_list_body_rect(area, true);
    (should_show_scrollbar(metrics) && body.width > 0 && body.height > 0).then_some(Rect::new(
        area.x + area.width.saturating_sub(1),
        body.y,
        1,
        body.height,
    ))
}

pub(crate) fn agent_panel_body_rect(area: Rect, has_scrollbar: bool) -> Rect {
    if area.width == 0 || area.height <= AGENT_PANEL_HEADER_ROWS {
        return Rect::default();
    }

    let body_y = area.y.saturating_add(AGENT_PANEL_HEADER_ROWS);
    let body_height = (area.y + area.height).saturating_sub(body_y);
    let body_width = area.width.saturating_sub(u16::from(has_scrollbar));
    Rect::new(area.x, body_y, body_width, body_height)
}

fn resolved_agent_rows(app: &AppState, entry: &AgentPanelEntry) -> Vec<Vec<ResolvedToken>> {
    let label = entry
        .state_labels
        .get(agent_panel_status_key(entry.state, entry.seen))
        .map(String::as_str)
        .unwrap_or_else(|| state_label(entry.state, entry.seen));
    tokens::agent_rows(&app.sidebar_agents, entry, label)
}

pub(crate) fn agent_entry_height_in_body(
    app: &AppState,
    entry: &AgentPanelEntry,
    body_height: u16,
) -> u16 {
    (resolved_agent_rows(app, entry)
        .len()
        .max(1)
        .min(u16::MAX as usize) as u16)
        .min(body_height)
}

/// Height of a folder or space header row in the agents panel folder view.
const AGENT_LIST_HEADER_ROWS: u16 = 1;

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

fn agent_panel_visible_count_from(app: &AppState, area: Rect, scroll: usize) -> usize {
    let body = agent_panel_body_rect(area, false);
    if body.width == 0 || body.height == 0 {
        return 0;
    }

    let mut used_rows = 0u16;
    let mut visible = 0usize;
    let entries = agent_panel_entries(app);
    let rows = agent_panel_list_entries(app, &entries);
    for (index, row) in rows.iter().enumerate().skip(scroll) {
        let height = agent_row_height_in_body(app, &entries, row, body.height);
        if used_rows.saturating_add(height) > body.height {
            break;
        }
        used_rows = used_rows.saturating_add(height);
        visible += 1;
        used_rows = used_rows
            .saturating_add(agent_row_gap(app, &rows, index))
            .min(body.height);
    }
    visible
}

fn agent_panel_bottom_start(app: &AppState, area: Rect) -> usize {
    let body = agent_panel_body_rect(area, false);
    let entries = agent_panel_entries(app);
    let rows = agent_panel_list_entries(app, &entries);
    let mut used_rows = 0u16;
    let mut start = rows.len();
    for (index, row) in rows.iter().enumerate().rev() {
        let gap = agent_row_gap(app, &rows, index);
        let needed = agent_row_height_in_body(app, &entries, row, body.height).saturating_add(gap);
        if used_rows.saturating_add(needed) > body.height {
            break;
        }
        used_rows = used_rows.saturating_add(needed);
        start = index;
    }
    start.min(rows.len().saturating_sub(1))
}

pub(crate) fn agent_panel_scroll_for_target(
    app: &AppState,
    area: Rect,
    current_scroll: usize,
    target: usize,
) -> usize {
    let max_scroll = agent_panel_bottom_start(app, area);
    if target < current_scroll {
        return target.min(max_scroll);
    }
    let mut scroll = current_scroll.min(max_scroll);
    while scroll < target {
        let visible = agent_panel_visible_count_from(app, area, scroll);
        if visible > 0 && target < scroll.saturating_add(visible) {
            break;
        }
        scroll += 1;
    }
    scroll.min(max_scroll)
}

pub(crate) fn agent_panel_scroll_metrics(app: &AppState, area: Rect) -> crate::pane::ScrollMetrics {
    let max_scroll = agent_panel_bottom_start(app, area);
    let scroll = app.agent_panel_scroll.min(max_scroll);
    let viewport_rows = agent_panel_visible_count_from(app, area, scroll);

    crate::pane::ScrollMetrics {
        offset_from_bottom: max_scroll.saturating_sub(scroll),
        max_offset_from_bottom: max_scroll,
        viewport_rows,
    }
}

pub(crate) fn agent_panel_scrollbar_rect(app: &AppState, area: Rect) -> Option<Rect> {
    let metrics = agent_panel_scroll_metrics(app, area);
    let body = agent_panel_body_rect(area, true);
    (should_show_scrollbar(metrics) && body.width > 0 && body.height > 0).then_some(Rect::new(
        area.x + area.width.saturating_sub(1),
        body.y,
        1,
        body.height,
    ))
}

pub(crate) fn compute_workspace_list_areas(
    app: &AppState,
    area: Rect,
) -> (
    Vec<crate::app::state::WorkspaceCardArea>,
    Vec<crate::app::state::FolderHeaderArea>,
) {
    let ws_area = workspace_list_rect(area, app.sidebar_section_split);
    if ws_area == Rect::default() {
        return (Vec::new(), Vec::new());
    }

    let metrics = workspace_list_scroll_metrics(app, ws_area);
    let body = workspace_list_body_rect(ws_area, should_show_scrollbar(metrics));
    if body.width == 0 || body.height == 0 {
        return (Vec::new(), Vec::new());
    }

    let scroll = app.workspace_scroll;
    let mut row_y = body.y;
    let body_bottom = body.y + body.height;
    let mut cards = Vec::new();
    let mut headers = Vec::new();

    let entries = workspace_list_entries(app);
    for (entry_idx, entry) in entries.iter().enumerate().skip(scroll) {
        match entry {
            WorkspaceListEntry::FolderHeader { order_idx } => {
                let Some(crate::folder::SpaceOrderEntry::Folder(folder)) =
                    app.space_order.get(*order_idx)
                else {
                    continue;
                };
                let row_height = FOLDER_HEADER_ROWS.min(body.height);
                let gap = workspace_entry_gap(app, &entries, entry_idx);
                if row_y.saturating_add(row_height) > body_bottom {
                    break;
                }
                headers.push(crate::app::state::FolderHeaderArea {
                    folder_id: folder.id.clone(),
                    rect: Rect::new(body.x, row_y, body.width, row_height),
                });
                row_y = row_y
                    .saturating_add(row_height)
                    .saturating_add(gap)
                    .min(body_bottom);
            }
            WorkspaceListEntry::Workspace {
                ws_idx,
                indented,
                foldered,
            } => {
                let Some(ws) = app.workspaces.get(*ws_idx) else {
                    continue;
                };
                let row_height = workspace_row_height_in_body(app, ws, *indented, body.height);
                let gap = workspace_entry_gap(app, &entries, entry_idx);
                if row_y.saturating_add(row_height) > body_bottom {
                    break;
                }
                cards.push(crate::app::state::WorkspaceCardArea {
                    ws_idx: *ws_idx,
                    rect: Rect::new(body.x, row_y, body.width, row_height),
                    indented: *indented,
                    foldered: *foldered,
                });
                row_y = row_y
                    .saturating_add(row_height)
                    .saturating_add(gap)
                    .min(body_bottom);
            }
        }
    }

    (cards, headers)
}

pub(crate) fn compute_workspace_card_areas(
    app: &AppState,
    area: Rect,
) -> Vec<crate::app::state::WorkspaceCardArea> {
    compute_workspace_list_areas(app, area).0
}

pub(crate) fn workspace_group_chevron_rect(card: &crate::app::state::WorkspaceCardArea) -> Rect {
    if card.rect.width == 0 || card.rect.height == 0 {
        return Rect::default();
    }

    Rect::new(
        card.rect.x + card.rect.width.saturating_sub(1),
        card.rect.y,
        1,
        1,
    )
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
fn render_collapse_chevron(frame: &mut Frame, collapsed: bool, rect: Rect, accent: Color) {
    frame.render_widget(
        Paragraph::new(Span::styled(
            if collapsed { "▸" } else { "▾" },
            Style::default().fg(accent),
        )),
        rect,
    );
}

/// Auto-scale sidebar width based on workspace identity + agent summary.
pub(crate) fn collapsed_sidebar_sections(area: Rect) -> (Rect, Option<u16>, Rect) {
    let content = Rect::new(area.x, area.y, area.width.saturating_sub(1), area.height);
    if content.width == 0 || content.height == 0 {
        return (Rect::default(), None, Rect::default());
    }

    if content.height < 7 {
        return (content, None, Rect::default());
    }

    let total_h = content.height as usize;
    let ws_h = total_h.div_ceil(2);
    let detail_h = total_h.saturating_sub(ws_h + 1);
    if ws_h == 0 || detail_h == 0 {
        return (content, None, Rect::default());
    }

    let divider_y = content.y + ws_h as u16;
    let ws_area = Rect::new(content.x, content.y, content.width, ws_h as u16);
    let detail_area = Rect::new(content.x, divider_y + 1, content.width, detail_h as u16);
    (ws_area, Some(divider_y), detail_area)
}

fn workspace_selection_background(p: &Palette, is_active: bool) -> Color {
    if is_active && p.selection_bg == Color::Reset {
        p.active_row_bg
    } else {
        p.selection_bg
    }
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

/// Collapsed sidebar: workspace glance on top, compact agent list below.
pub(super) fn render_sidebar_collapsed(app: &AppState, frame: &mut Frame, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let is_navigating = matches!(app.mode, Mode::Navigate);

    let p = &app.palette;
    frame
        .buffer_mut()
        .set_style(area, Style::default().bg(p.sidebar_bg));
    let sep_style = if is_navigating {
        Style::default().fg(p.accent)
    } else {
        Style::default().fg(p.surface_dim)
    };
    let sep_x = area.x + area.width.saturating_sub(1);
    let buf = frame.buffer_mut();
    for y in area.y..area.y + area.height {
        buf[(sep_x, y)].set_symbol("│");
        buf[(sep_x, y)].set_style(sep_style);
    }

    let (ws_area, divider_y, detail_area) = collapsed_sidebar_sections(area);
    if ws_area == Rect::default() {
        render_sidebar_toggle(app, frame, area, true, p);
        return;
    }

    for (visible_idx, ws) in app.workspaces.iter().enumerate() {
        let y = ws_area.y + visible_idx as u16;
        if y >= ws_area.y + ws_area.height {
            break;
        }
        let (agg_state, agg_seen) = ws.aggregate_state(&app.terminals);
        let (icon, icon_style) = state_icon(agg_state, agg_seen, app.status_indicators, p);
        let is_selected = visible_idx == app.selected && is_navigating;
        let is_active = Some(visible_idx) == app.active;
        let selection_bg = workspace_selection_background(p, is_active);
        let row_style = if is_selected {
            Style::default().bg(selection_bg)
        } else if is_active {
            Style::default().bg(p.active_row_bg)
        } else {
            Style::default()
        };
        let num_style = if is_selected {
            Style::default().fg(p.overlay1).bg(selection_bg)
        } else if is_active {
            Style::default().fg(p.text).bg(p.active_row_bg)
        } else {
            Style::default().fg(p.overlay0)
        };

        if is_selected || is_active {
            let buf = frame.buffer_mut();
            for x in ws_area.x..ws_area.x + ws_area.width {
                buf[(x, y)].set_style(row_style);
            }
        }

        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(format!("{:<2}", visible_idx + 1), num_style),
                Span::styled(icon, icon_style),
            ])),
            Rect::new(ws_area.x, y, ws_area.width, 1),
        );
    }

    if let Some(divider_y) = divider_y {
        let buf = frame.buffer_mut();
        let divider_color = if app.agent_view_override.is_some() {
            p.accent
        } else {
            p.surface_dim
        };
        for x in ws_area.x..ws_area.x + ws_area.width {
            buf[(x, divider_y)].set_symbol("─");
            buf[(x, divider_y)].set_style(Style::default().fg(divider_color));
        }
    }

    let detail_content_area = Rect::new(
        detail_area.x,
        detail_area.y,
        detail_area.width,
        detail_area.height.saturating_sub(1),
    );
    if detail_content_area != Rect::default() {
        for (detail_idx, detail) in agent_panel_entries(app).iter().enumerate() {
            let y = detail_content_area.y + detail_idx as u16;
            if y >= detail_content_area.y + detail_content_area.height {
                break;
            }
            let position = detail_idx + 1;
            let is_active = app.is_active_pane(detail.ws_idx, detail.tab_idx, detail.pane_id);
            let position_style = if is_active {
                Style::default().fg(p.text).bg(p.active_row_bg)
            } else {
                Style::default().fg(p.overlay0)
            };
            let (icon, icon_style) =
                state_icon(detail.state, detail.seen, app.status_indicators, p);

            if is_active {
                let buf = frame.buffer_mut();
                for x in detail_content_area.x..detail_content_area.x + detail_content_area.width {
                    buf[(x, y)].set_style(Style::default().bg(p.active_row_bg));
                }
            }

            frame.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled(format!("{position:<2}"), position_style),
                    Span::styled(icon, icon_style),
                ])),
                Rect::new(detail_content_area.x, y, detail_content_area.width, 1),
            );
        }
    }

    render_sidebar_toggle(app, frame, area, true, p);
}

/// Legal insertion points for a spaces-panel drag, in visual order. Each slot
/// pairs a drop target with its indicator row. Slots exist before every
/// top-level block (loose space, worktree family, or folder) and before every
/// member block inside an expanded folder; appending into a folder happens by
/// dropping onto its header row, which is not a slot.
pub(crate) fn workspace_drop_slots(
    app: &AppState,
    cards: &[crate::app::state::WorkspaceCardArea],
    headers: &[crate::app::state::FolderHeaderArea],
    area: Rect,
) -> Vec<(crate::app::state::WorkspaceDropTarget, u16)> {
    use crate::app::state::WorkspaceDropTarget;

    if area.height == 0 || (cards.is_empty() && headers.is_empty()) {
        return Vec::new();
    }
    let list_bottom = area.y + area.height.saturating_sub(1);
    let entries = workspace_list_entries(app);
    let folder_id_at = |order_idx: usize| match app.space_order.get(order_idx) {
        Some(crate::folder::SpaceOrderEntry::Folder(folder)) => Some(folder.id.clone()),
        _ => None,
    };
    // The folder containing the entry at `idx`: the nearest preceding header,
    // unless a non-foldered workspace closes the folder run first.
    let containing_folder = |idx: usize| -> Option<String> {
        entries[..=idx].iter().rev().find_map(|entry| match entry {
            WorkspaceListEntry::FolderHeader { order_idx } => Some(folder_id_at(*order_idx)),
            WorkspaceListEntry::Workspace {
                foldered: false, ..
            } => Some(None),
            WorkspaceListEntry::Workspace { .. } => None,
        })?
    };
    let card_rect = |ws_idx: usize| {
        cards
            .iter()
            .find(|card| card.ws_idx == ws_idx)
            .map(|card| card.rect)
    };
    let header_rect = |folder_id: &str| {
        headers
            .iter()
            .find(|header| header.folder_id == folder_id)
            .map(|header| header.rect)
    };

    let mut slots: Vec<(WorkspaceDropTarget, u16)> = Vec::new();
    // Entry index and bottom row of the lowest visible element, for the
    // trailing slot after the last card or header.
    let mut last_visible: Option<(usize, u16)> = None;
    let mut previous_was_header = false;
    for (entry_idx, entry) in entries.iter().enumerate() {
        let was_header = std::mem::replace(&mut previous_was_header, false);
        match entry {
            WorkspaceListEntry::FolderHeader { order_idx } => {
                previous_was_header = true;
                let Some(folder_id) = folder_id_at(*order_idx) else {
                    continue;
                };
                let Some(rect) = header_rect(&folder_id) else {
                    continue;
                };
                last_visible = Some((entry_idx, rect.y.saturating_add(rect.height)));
                if let Some(row) = rect.y.checked_sub(1).filter(|row| *row < list_bottom) {
                    slots.push((WorkspaceDropTarget::BeforeFolder(folder_id), row));
                }
            }
            WorkspaceListEntry::Workspace {
                ws_idx,
                indented,
                foldered,
            } => {
                let Some(rect) = card_rect(*ws_idx) else {
                    continue;
                };
                last_visible = Some((entry_idx, rect.y.saturating_add(rect.height)));
                if *indented {
                    // Never split a worktree family: no slots inside a block.
                    continue;
                }
                if *foldered {
                    let Some(folder_id) = containing_folder(entry_idx) else {
                        continue;
                    };
                    // A folder header hugs its first member, so the slot for
                    // position 0 sits on the member's own top row; later
                    // member slots use the gap row above, like top level.
                    let row = if was_header {
                        Some(rect.y)
                    } else {
                        rect.y.checked_sub(1)
                    };
                    if let Some(row) = row.filter(|row| *row < list_bottom) {
                        slots.push((
                            WorkspaceDropTarget::InFolderBefore {
                                folder_id,
                                ws_idx: *ws_idx,
                            },
                            row,
                        ));
                    }
                } else if let Some(row) = rect.y.checked_sub(1).filter(|row| *row < list_bottom) {
                    slots.push((WorkspaceDropTarget::Before(*ws_idx), row));
                }
            }
        }
    }

    let Some((last_entry_idx, last_bottom)) = last_visible else {
        return slots;
    };
    let next_entry = entries.get(last_entry_idx.saturating_add(1));
    let target = match next_entry {
        // Mid-family clip: never split a worktree family.
        Some(WorkspaceListEntry::Workspace { indented: true, .. }) => return slots,
        Some(WorkspaceListEntry::FolderHeader { order_idx }) => match folder_id_at(*order_idx) {
            Some(folder_id) => WorkspaceDropTarget::BeforeFolder(folder_id),
            None => return slots,
        },
        Some(WorkspaceListEntry::Workspace {
            ws_idx,
            foldered: true,
            ..
        }) => match containing_folder(last_entry_idx.saturating_add(1)) {
            Some(folder_id) => WorkspaceDropTarget::InFolderBefore {
                folder_id,
                ws_idx: *ws_idx,
            },
            None => return slots,
        },
        Some(WorkspaceListEntry::Workspace { ws_idx, .. }) => WorkspaceDropTarget::Before(*ws_idx),
        None => WorkspaceDropTarget::End,
    };
    if last_bottom < list_bottom
        && slots
            .last()
            .is_none_or(|(last_target, _)| *last_target != target)
    {
        slots.push((target, last_bottom));
    }
    slots
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

pub(crate) fn workspace_drop_indicator_row(
    app: &AppState,
    cards: &[crate::app::state::WorkspaceCardArea],
    headers: &[crate::app::state::FolderHeaderArea],
    area: Rect,
    target: &crate::app::state::WorkspaceDropTarget,
) -> Option<u16> {
    workspace_drop_slots(app, cards, headers, area)
        .into_iter()
        .find_map(|(candidate, row)| (candidate == *target).then_some(row))
}

pub(super) fn render_sidebar(
    app: &AppState,
    terminal_runtimes: &TerminalRuntimeRegistry,
    frame: &mut Frame,
    area: Rect,
) {
    let p = &app.palette;
    frame
        .buffer_mut()
        .set_style(area, Style::default().bg(p.sidebar_bg));
    let is_navigating = matches!(app.mode, Mode::Navigate);
    let sep_style = if is_navigating {
        Style::default().fg(p.accent)
    } else {
        Style::default().fg(p.surface_dim)
    };

    let sep_x = area.x + area.width.saturating_sub(1);
    let buf = frame.buffer_mut();
    for y in area.y..area.y + area.height {
        buf[(sep_x, y)].set_symbol("│");
        buf[(sep_x, y)].set_style(sep_style);
    }

    let (ws_area, detail_area) = expanded_sidebar_sections(area, app.sidebar_section_split);

    render_workspace_list(app, terminal_runtimes, frame, ws_area, is_navigating);
    render_agent_detail(app, terminal_runtimes, frame, detail_area);
    render_sidebar_toggle(app, frame, area, false, p);
}

fn resolved_token_spans(
    resolved: &[ResolvedToken],
    state_icon: (&str, Style),
    state_text_style: Style,
    workspace_style: Style,
    secondary_style: Style,
    custom_style: Style,
    p: &Palette,
    max_width: usize,
) -> Vec<Span<'static>> {
    let fixed_widths = resolved
        .iter()
        .map(|token| match &token.kind {
            ResolvedTokenKind::StateIcon => display_width(state_icon.0),
            ResolvedTokenKind::GitStatus { ahead, behind } => {
                usize::from(*ahead > 0) * display_width(&format!("↑{ahead}"))
                    + usize::from(*behind > 0) * display_width(&format!("↓{behind}"))
                    + usize::from(*ahead > 0 && *behind > 0)
            }
            _ => 0,
        })
        .collect::<Vec<_>>();
    let flexible_widths = resolved
        .iter()
        .map(|token| match &token.kind {
            ResolvedTokenKind::StateText(text)
            | ResolvedTokenKind::Workspace(text)
            | ResolvedTokenKind::Tab(text)
            | ResolvedTokenKind::Pane(text)
            | ResolvedTokenKind::Agent(text)
            | ResolvedTokenKind::TerminalTitle(text)
            | ResolvedTokenKind::Branch(text)
            | ResolvedTokenKind::Custom(text) => display_width(text),
            _ => 0,
        })
        .collect::<Vec<_>>();
    let minimum_width = |active: &[bool]| {
        let indices = active
            .iter()
            .enumerate()
            .filter_map(|(index, active)| active.then_some(index))
            .collect::<Vec<_>>();
        let content = indices
            .iter()
            .map(|index| fixed_widths[*index] + usize::from(flexible_widths[*index] > 0))
            .sum::<usize>();
        let separators = indices
            .windows(2)
            .map(|pair| display_width(tokens::separator(&resolved[pair[0]], &resolved[pair[1]])))
            .sum::<usize>();
        content + separators
    };
    let mut active = resolved.iter().map(|_| true).collect::<Vec<_>>();
    if minimum_width(&active) > max_width {
        for (index, width) in flexible_widths.iter().enumerate() {
            if *width > 0 {
                active[index] = false;
            }
        }
        for index in (0..resolved.len()).rev() {
            if flexible_widths[index] == 0 {
                continue;
            }
            active[index] = true;
            if minimum_width(&active) > max_width {
                active[index] = false;
            }
        }
    }
    let visible_indices = active
        .iter()
        .enumerate()
        .filter_map(|(index, active)| active.then_some(index))
        .collect::<Vec<_>>();
    let separator_width = visible_indices
        .windows(2)
        .map(|pair| display_width(tokens::separator(&resolved[pair[0]], &resolved[pair[1]])))
        .sum::<usize>();
    let fixed_width = visible_indices
        .iter()
        .map(|index| fixed_widths[*index])
        .sum::<usize>();
    let mut budgets = flexible_widths
        .iter()
        .enumerate()
        .map(|(index, width)| usize::from(active[index] && *width > 0))
        .collect::<Vec<_>>();
    let minimum = budgets.iter().sum::<usize>();
    let mut remaining = max_width
        .saturating_sub(separator_width + fixed_width)
        .saturating_sub(minimum);
    while remaining > 0 {
        let mut grew = false;
        for (budget, width) in budgets.iter_mut().zip(&flexible_widths) {
            if *budget > 0 && *budget < *width {
                *budget += 1;
                remaining -= 1;
                grew = true;
                if remaining == 0 {
                    break;
                }
            }
        }
        if !grew {
            break;
        }
    }
    let mut spans = Vec::new();
    for (position, index) in visible_indices.iter().copied().enumerate() {
        let token = &resolved[index];
        if position > 0 {
            let previous = &resolved[visible_indices[position - 1]];
            spans.push(Span::styled(
                tokens::separator(previous, token),
                Style::default().fg(p.overlay0).add_modifier(Modifier::DIM),
            ));
        }
        match &token.kind {
            ResolvedTokenKind::StateIcon => {
                spans.push(Span::styled(
                    state_icon.0.to_string(),
                    apply_token_style(state_icon.1, token.style),
                ));
            }
            ResolvedTokenKind::StateText(text) => {
                spans.push(Span::styled(
                    truncate_end(text, budgets[index]),
                    apply_token_style(state_text_style, token.style),
                ));
            }
            ResolvedTokenKind::Workspace(text) => {
                spans.push(Span::styled(
                    truncate_end(text, budgets[index]),
                    apply_token_style(workspace_style, token.style),
                ));
            }
            ResolvedTokenKind::Tab(text)
            | ResolvedTokenKind::Pane(text)
            | ResolvedTokenKind::Agent(text)
            | ResolvedTokenKind::Branch(text) => {
                spans.push(Span::styled(
                    truncate_end(text, budgets[index]),
                    apply_token_style(secondary_style, token.style),
                ));
            }
            ResolvedTokenKind::GitStatus { ahead, behind } => {
                if *ahead > 0 {
                    spans.push(Span::styled(
                        format!("↑{ahead}"),
                        apply_token_style(Style::default().fg(p.green), token.style),
                    ));
                }
                if *ahead > 0 && *behind > 0 {
                    spans.push(Span::styled(
                        " ",
                        apply_token_style(Style::default(), token.style),
                    ));
                }
                if *behind > 0 {
                    spans.push(Span::styled(
                        format!("↓{behind}"),
                        apply_token_style(Style::default().fg(p.red), token.style),
                    ));
                }
            }
            ResolvedTokenKind::TerminalTitle(text) | ResolvedTokenKind::Custom(text) => {
                spans.push(Span::styled(
                    truncate_end(text, budgets[index]),
                    apply_token_style(custom_style, token.style),
                ));
            }
        }
    }
    spans
}

fn apply_token_style(mut style: Style, patch: crate::config::SidebarTokenStyle) -> Style {
    if let Some(fg) = patch.fg {
        style = style.fg(fg.ratatui());
    }
    if let Some(bold) = patch.bold {
        style = if bold {
            style.add_modifier(Modifier::BOLD)
        } else {
            style.remove_modifier(Modifier::BOLD)
        };
    }
    if let Some(dim) = patch.dim {
        style = if dim {
            style.add_modifier(Modifier::DIM)
        } else {
            style.remove_modifier(Modifier::DIM)
        };
    }
    style
}

fn render_workspace_list(
    app: &AppState,
    terminal_runtimes: &TerminalRuntimeRegistry,
    frame: &mut Frame,
    area: Rect,
    is_navigating: bool,
) {
    let p = &app.palette;
    let dragged_ws_idx = match app.drag.as_ref().map(|drag| &drag.target) {
        Some(crate::app::state::DragTarget::WorkspaceReorder { source_ws_idx, .. }) => {
            Some(*source_ws_idx)
        }
        _ => None,
    };
    let dragged_folder_id = match app.drag.as_ref().map(|drag| &drag.target) {
        Some(crate::app::state::DragTarget::FolderReorder { folder_id, .. }) => {
            Some(folder_id.as_str())
        }
        _ => None,
    };
    let drag_drop_target = match app.drag.as_ref().map(|drag| &drag.target) {
        Some(
            crate::app::state::DragTarget::WorkspaceReorder {
                drop_target: Some(drop_target),
                ..
            }
            | crate::app::state::DragTarget::FolderReorder {
                drop_target: Some(drop_target),
                ..
            },
        ) => Some(drop_target),
        _ => None,
    };
    // Dropping onto a folder header highlights the header instead of drawing
    // an insertion line.
    let drop_into_folder_id = match drag_drop_target {
        Some(crate::app::state::WorkspaceDropTarget::IntoFolder(folder_id)) => {
            Some(folder_id.as_str())
        }
        _ => None,
    };
    let insertion_row = match drag_drop_target {
        Some(crate::app::state::WorkspaceDropTarget::IntoFolder(_)) | None => None,
        Some(drop_target) => workspace_drop_indicator_row(
            app,
            &app.view.workspace_card_areas,
            &app.view.folder_header_areas,
            area,
            drop_target,
        ),
    };

    let list_bottom = area.y + area.height.saturating_sub(1);
    if area.height > 0 {
        frame.render_widget(
            Paragraph::new(Line::from(vec![Span::styled(
                " spaces",
                Style::default().fg(p.overlay0).add_modifier(Modifier::BOLD),
            )])),
            Rect::new(area.x, area.y, area.width, 1),
        );
    }

    let metrics = workspace_list_scroll_metrics(app, area);
    let scrollbar_rect = workspace_list_scrollbar_rect(app, area);
    let cards = &app.view.workspace_card_areas;
    let entries = workspace_list_entries(app);

    for header in &app.view.folder_header_areas {
        if header.rect.y >= list_bottom || header.rect.width == 0 {
            continue;
        }
        let Some(folder) = app.folder(&header.folder_id) else {
            continue;
        };
        let is_dragged = dragged_folder_id == Some(header.folder_id.as_str());
        let is_drop_target = drop_into_folder_id == Some(header.folder_id.as_str());
        let highlight = collapsed_folder_header_highlight(app, folder);
        if highlight.selected || highlight.active || is_dragged {
            let bg = if highlight.selected {
                workspace_selection_background(p, highlight.active)
            } else if is_dragged {
                p.surface1
            } else {
                p.active_row_bg
            };
            let buf = frame.buffer_mut();
            for x in header.rect.x..header.rect.x + header.rect.width {
                buf[(x, header.rect.y)].set_style(Style::default().bg(bg));
            }
        }
        let name_style = if is_drop_target {
            Style::default().fg(p.accent).add_modifier(Modifier::BOLD)
        } else if highlight.selected || highlight.active {
            Style::default().fg(p.text).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(p.subtext0).add_modifier(Modifier::BOLD)
        };
        // Reserve the gutter, chevron, and gap cells so the name never runs
        // into the collapse affordance and aligns with loose space names.
        let name = truncate_end(&folder.name, header.rect.width.saturating_sub(3) as usize);
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::raw("   "),
                Span::styled(name, name_style),
            ])),
            header.rect,
        );
        let collapsed = app.collapsed_folder_ids.contains(&header.folder_id);
        render_collapse_chevron(
            frame,
            collapsed,
            folder_header_chevron_rect(header),
            p.accent,
        );
    }

    for card in cards {
        let i = card.ws_idx;
        let ws = &app.workspaces[i];
        let row_y = card.rect.y;
        let row_height = card.rect.height;
        let selected = i == app.selected && is_navigating;
        let is_active = Some(i) == app.active;
        let is_dragged = dragged_ws_idx == Some(i);
        let highlighted = selected || is_active || is_dragged;
        let (agg_state, agg_seen) = ws.aggregate_state(&app.terminals);

        if highlighted {
            let bg = if selected {
                workspace_selection_background(p, is_active)
            } else if is_dragged {
                p.surface1
            } else {
                p.active_row_bg
            };
            let buf = frame.buffer_mut();
            for y in row_y..row_y + row_height {
                if y >= list_bottom {
                    break;
                }
                for x in card.rect.x..card.rect.x + card.rect.width {
                    buf[(x, y)].set_style(Style::default().bg(bg));
                }
            }
        }

        let name_style = if selected || is_active || is_dragged {
            Style::default().fg(p.text).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(p.subtext0)
        };

        let label = ws.display_name_from(&app.terminals, terminal_runtimes);
        let display_label = if card.indented {
            grouped_child_display_label(&label, ws.branch().as_deref(), ws.custom_name.is_some())
        } else {
            label
        };
        let parent_group = (!card.indented)
            .then(|| workspace_parent_group_state(app, i))
            .flatten();
        let is_last_child = card.indented
            && entries
                .iter()
                .position(|entry| {
                    matches!(
                        entry,
                        WorkspaceListEntry::Workspace { ws_idx, .. } if *ws_idx == i
                    )
                })
                .is_none_or(|entry_idx| !next_entry_is_indented_workspace(&entries, entry_idx));
        let (display_state, display_seen) = parent_group
            .as_ref()
            .filter(|(_, collapsed)| *collapsed)
            .map(|(key, _)| space_aggregate_state(app, key))
            .unwrap_or((agg_state, agg_seen));
        let state_icon = state_icon(display_state, display_seen, app.status_indicators, p);
        let state_text_style = Style::default()
            .fg(state_label_color(display_state, display_seen, p))
            .add_modifier(Modifier::DIM);
        let branch_style = Style::default().fg(if selected || is_active {
            p.mauve
        } else {
            p.overlay0
        });
        let token_values = ws.metadata_tokens.values();
        let rows = tokens::space_rows(
            &app.sidebar_spaces,
            SpaceTokenContext {
                workspace: &display_label,
                branch: ws.branch().as_deref(),
                state_text: state_label(display_state, display_seen),
                ahead_behind: ws.git_ahead_behind(),
                tokens: &token_values,
                suppress_git_details: card.indented,
            },
        );

        for (row_index, resolved) in rows.iter().enumerate() {
            if row_index as u16 >= row_height || row_y + row_index as u16 >= list_bottom {
                break;
            }
            let mut spans = Vec::new();
            // Foldered cards are nested under their folder header with a
            // fixed margin; card content is otherwise identical.
            let folder_margin: u16 = if card.foldered {
                spans.push(Span::raw("  "));
                2
            } else {
                0
            };
            let prefix_width = folder_margin
                + if card.indented {
                    spans.push(Span::raw("   "));
                    if row_index == 0 {
                        spans.push(Span::styled(
                            if is_last_child { "└─ " } else { "├─ " },
                            Style::default().fg(p.overlay0),
                        ));
                        6
                    } else if is_last_child {
                        spans.push(Span::raw("     "));
                        8
                    } else {
                        spans.push(Span::styled("│", Style::default().fg(p.overlay0)));
                        spans.push(Span::raw("    "));
                        8
                    }
                } else if row_index == 0 {
                    spans.push(Span::raw(" "));
                    1
                } else {
                    spans.push(Span::raw("   "));
                    3
                };
            let trailing_width = if row_index == 0 && parent_group.is_some() {
                2
            } else {
                0
            };
            spans.extend(resolved_token_spans(
                resolved,
                state_icon,
                state_text_style,
                name_style,
                branch_style,
                branch_style,
                p,
                card.rect
                    .width
                    .saturating_sub(prefix_width + trailing_width) as usize,
            ));
            frame.render_widget(
                Paragraph::new(Line::from(spans)),
                Rect::new(card.rect.x, row_y + row_index as u16, card.rect.width, 1),
            );
        }

        if let Some((_, collapsed)) = parent_group {
            frame.render_widget(
                Paragraph::new(Span::styled(
                    if collapsed { "▸" } else { "▾" },
                    Style::default().fg(p.accent),
                )),
                workspace_group_chevron_rect(card),
            );
        }
    }

    if let Some(y) = insertion_row.filter(|y| *y < list_bottom) {
        let indicator_right = scrollbar_rect
            .map(|rect| rect.x)
            .unwrap_or(area.x + area.width);
        let buf = frame.buffer_mut();
        for x in area.x..indicator_right {
            buf[(x, y)].set_symbol("─");
            buf[(x, y)].set_style(Style::default().fg(p.accent));
        }
    }

    if let Some(track) = scrollbar_rect {
        render_scrollbar(frame, metrics, track, p.surface_dim, p.overlay0, "▕");
    }

    if app.mouse_capture && list_bottom > area.y {
        let new_rect = app.sidebar_new_button_rect();
        frame.render_widget(
            Paragraph::new(Span::styled(" new", Style::default().fg(p.overlay0))),
            new_rect,
        );

        let menu_rect = app.global_launcher_rect();
        let menu_line = if app.global_menu_attention_badge_visible() {
            Line::from(vec![
                Span::styled(
                    "● ",
                    Style::default().fg(p.accent).add_modifier(Modifier::BOLD),
                ),
                Span::styled("menu", Style::default().fg(p.overlay0)),
            ])
        } else {
            Line::from(vec![Span::styled("menu", Style::default().fg(p.overlay0))])
        };
        frame.render_widget(
            Paragraph::new(menu_line).alignment(Alignment::Right),
            menu_rect,
        );
    }
}

fn render_agent_detail(
    app: &AppState,
    terminal_runtimes: &TerminalRuntimeRegistry,
    frame: &mut Frame,
    area: Rect,
) {
    let p = &app.palette;

    if area.height < 3 {
        return;
    }

    let sep_line = "─".repeat(area.width as usize);
    frame.render_widget(
        Paragraph::new(Span::styled(&sep_line, Style::default().fg(p.surface_dim))),
        Rect::new(area.x, area.y, area.width, 1),
    );

    frame.render_widget(
        Paragraph::new(Line::from(vec![Span::styled(
            " agents",
            Style::default().fg(p.overlay0).add_modifier(Modifier::BOLD),
        )])),
        Rect::new(area.x, area.y + 1, area.width, 1),
    );
    let control_label = active_agent_view_label(app)
        .unwrap_or_else(|| agent_panel_sort_label(app.agent_panel_sort));
    let toggle_rect = agent_panel_header_label_rect(area, control_label);
    if toggle_rect != Rect::default() {
        let color = if app.agent_view_override.is_some() {
            p.accent
        } else {
            p.overlay0
        };
        frame.render_widget(
            Paragraph::new(Span::styled(
                control_label,
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ))
            .alignment(Alignment::Right),
            toggle_rect,
        );
    }

    let details = agent_panel_entries_from(app, terminal_runtimes);
    let metrics = agent_panel_scroll_metrics(app, area);
    let scrollbar_rect = agent_panel_scrollbar_rect(app, area);
    let body = agent_panel_body_rect(area, should_show_scrollbar(metrics));
    if body == Rect::default() {
        return;
    }
    if details.is_empty() && app.agent_view_override.is_some() {
        frame.render_widget(
            Paragraph::new(" no matching agents")
                .style(Style::default().fg(p.overlay0).add_modifier(Modifier::DIM)),
            Rect::new(body.x, body.y, body.width, 1),
        );
        return;
    }

    let scroll = app.agent_panel_scroll.min(metrics.max_offset_from_bottom);
    let list_rows = agent_panel_list_entries(app, &details);
    let mut row_y = body.y;
    let body_bottom = body.y + body.height;
    // Indent applied to agent rows nested under the folder view's space
    // headers; zero outside the folder view. When scrolled past a space
    // header, its agents keep the indent it established.
    let mut agent_indent: u16 = list_rows[..scroll.min(list_rows.len())]
        .iter()
        .rev()
        .find_map(|row| match row {
            AgentPanelListEntry::SpaceHeader {
                indented, foldered, ..
            } => Some(space_header_agent_indent(*indented, *foldered)),
            _ => None,
        })
        .unwrap_or(0);
    for (index, list_row) in list_rows.iter().enumerate().skip(scroll) {
        let height = agent_row_height_in_body(app, &details, list_row, body.height);
        if row_y.saturating_add(height) > body_bottom {
            break;
        }

        match list_row {
            AgentPanelListEntry::FolderHeader { order_idx } => {
                if let Some(crate::folder::SpaceOrderEntry::Folder(folder)) =
                    app.space_order.get(*order_idx)
                {
                    // A collapsed folder hides all member rows; its header
                    // indicates a hidden active space, like the spaces panel.
                    let highlight = collapsed_folder_header_highlight(app, folder);
                    let row_style = if highlight.active {
                        Style::default().bg(p.active_row_bg)
                    } else {
                        Style::default()
                    };
                    let name_style = if highlight.active {
                        Style::default().fg(p.text).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(p.subtext0).add_modifier(Modifier::BOLD)
                    };
                    // Reserve the gutter, chevron, and gap cells so the name
                    // never runs into the collapse affordance.
                    let name = truncate_end(&folder.name, body.width.saturating_sub(3) as usize);
                    frame.render_widget(
                        Paragraph::new(Line::from(vec![
                            Span::raw("   "),
                            Span::styled(name, name_style),
                        ]))
                        .style(row_style),
                        Rect::new(body.x, row_y, body.width, 1),
                    );
                    let collapsed = app.collapsed_folder_ids.contains(&folder.id);
                    render_collapse_chevron(
                        frame,
                        collapsed,
                        agent_panel_header_chevron_rect(body, row_y, AGENT_PANEL_HEADER_GUTTER),
                        p.accent,
                    );
                }
            }
            AgentPanelListEntry::SpaceHeader {
                ws_idx,
                indented,
                foldered,
                thin,
            } => {
                agent_indent = space_header_agent_indent(*indented, *foldered);
                if let Some(ws) = app.workspaces.get(*ws_idx) {
                    let label = ws.display_name_from(&app.terminals, terminal_runtimes);
                    let label = if *indented {
                        grouped_child_display_label(
                            &label,
                            ws.branch().as_deref(),
                            ws.custom_name.is_some(),
                        )
                    } else {
                        label
                    };
                    let mut spans = Vec::new();
                    spans.push(Span::raw(" ".repeat(AGENT_PANEL_HEADER_GUTTER as usize)));
                    if *foldered {
                        spans.push(Span::raw("  "));
                    }
                    if *indented {
                        spans.push(Span::raw("   "));
                        let is_last_child = !next_agent_header_is_indented_space(&list_rows, index);
                        spans.push(Span::styled(
                            if is_last_child { "└─ " } else { "├─ " },
                            Style::default().fg(p.overlay0),
                        ));
                    }
                    // Leading chevron cell plus one gap cell; the chevron is
                    // drawn over the first cell below (thin headers keep the
                    // blank cells so sibling names stay aligned).
                    spans.push(Span::raw("  "));
                    let prefix_width = space_header_prefix_width(*indented, *foldered);
                    let collapsed = !*thin && app.collapsed_agent_space_ids.contains(&ws.id);
                    // A collapsed agent list hides all rows; the header
                    // indicates the hidden active agent, like a collapsed
                    // folder header.
                    let indicates_active = collapsed && app.active == Some(*ws_idx);
                    let row_style = if indicates_active {
                        Style::default().bg(p.active_row_bg)
                    } else {
                        Style::default()
                    };
                    let name_style = if *thin {
                        Style::default().fg(p.overlay0).add_modifier(Modifier::DIM)
                    } else if indicates_active {
                        Style::default().fg(p.text).add_modifier(Modifier::BOLD)
                    } else {
                        // Space header names carry the same weight as folder
                        // headers, foldered or not; only their agent rows
                        // below stay regular.
                        Style::default().fg(p.subtext0).add_modifier(Modifier::BOLD)
                    };
                    spans.push(Span::styled(
                        truncate_end(&label, body.width.saturating_sub(prefix_width) as usize),
                        name_style,
                    ));
                    frame.render_widget(
                        Paragraph::new(Line::from(spans)).style(row_style),
                        Rect::new(body.x, row_y, body.width, 1),
                    );
                    // Thin ancestor headers have no agent list to collapse.
                    if !*thin {
                        render_collapse_chevron(
                            frame,
                            collapsed,
                            agent_panel_header_chevron_rect(
                                body,
                                row_y,
                                space_header_chevron_indent(*indented, *foldered),
                            ),
                            p.accent,
                        );
                    }
                }
            }
            AgentPanelListEntry::Agent { entry_idx } => {
                let Some(detail) = details.get(*entry_idx) else {
                    continue;
                };
                let label_color = state_label_color(detail.state, detail.seen, p);
                let rows = resolved_agent_rows(app, detail);

                let is_active = app.is_active_pane(detail.ws_idx, detail.tab_idx, detail.pane_id);
                let row_style = if is_active {
                    Style::default().bg(p.active_row_bg)
                } else {
                    Style::default()
                };
                let name_style = if is_active {
                    Style::default().fg(p.text).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(p.subtext0).add_modifier(Modifier::BOLD)
                };
                let status_style = if is_active {
                    Style::default().fg(label_color)
                } else {
                    Style::default().fg(label_color).add_modifier(Modifier::DIM)
                };
                let agent_style = Style::default().fg(p.overlay0).add_modifier(Modifier::DIM);
                let state_icon = state_icon(detail.state, detail.seen, app.status_indicators, p);

                for (row_index, resolved) in rows.iter().take(height as usize).enumerate() {
                    let prefix = agent_indent + if row_index == 0 { 1 } else { 3 };
                    let mut spans = vec![Span::raw(" ".repeat(prefix as usize))];
                    spans.extend(resolved_token_spans(
                        resolved,
                        state_icon,
                        status_style,
                        name_style,
                        agent_style,
                        agent_style,
                        p,
                        body.width.saturating_sub(prefix) as usize,
                    ));
                    frame.render_widget(
                        Paragraph::new(Line::from(spans)).style(row_style),
                        Rect::new(body.x, row_y + row_index as u16, body.width, 1),
                    );
                }
            }
        }
        row_y = row_y
            .saturating_add(height)
            .saturating_add(agent_row_gap(app, &list_rows, index))
            .min(body_bottom);
    }

    if let Some(track) = scrollbar_rect {
        render_scrollbar(frame, metrics, track, p.surface_dim, p.overlay0, "▕");
    }
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
fn space_header_prefix_width(indented: bool, foldered: bool) -> u16 {
    space_header_chevron_indent(indented, foldered) + 2
}

/// Indent applied to agent rows nested under a folder-view space header:
/// one cell past the header's chevron column, so the row's leading state
/// icon lands under the first letter of the header's name — mirroring how
/// the spaces panel nests member icons under their folder's name.
fn space_header_agent_indent(indented: bool, foldered: bool) -> u16 {
    space_header_chevron_indent(indented, foldered) + 1
}

/// Whether the next header row after `idx` (skipping agent rows) is an
/// indented space header, i.e. the current worktree child is not the last of
/// its family in the folder view.
fn next_agent_header_is_indented_space(rows: &[AgentPanelListEntry], idx: usize) -> bool {
    rows[idx.saturating_add(1)..]
        .iter()
        .find_map(|row| match row {
            AgentPanelListEntry::Agent { .. } => None,
            AgentPanelListEntry::SpaceHeader { indented, .. } => Some(*indented),
            AgentPanelListEntry::FolderHeader { .. } => Some(false),
        })
        .unwrap_or(false)
}

pub(crate) fn collapsed_sidebar_toggle_rect(area: Rect) -> Rect {
    let bottom_y = area.y + area.height.saturating_sub(1);
    let content_w = area.width.saturating_sub(1);
    if content_w == 0 || area.height == 0 {
        return Rect::default();
    }
    let x = area.x + content_w / 2;
    Rect::new(x, bottom_y, 1, 1)
}

pub(crate) fn expanded_sidebar_toggle_rect(area: Rect) -> Rect {
    if area.width <= 1 || area.height == 0 {
        return Rect::default();
    }
    Rect::new(
        area.x + area.width.saturating_sub(2),
        area.y + area.height.saturating_sub(1),
        1,
        1,
    )
}

fn render_sidebar_toggle(
    app: &AppState,
    frame: &mut Frame,
    area: Rect,
    collapsed: bool,
    p: &Palette,
) {
    let toggle_area = if collapsed {
        collapsed_sidebar_toggle_rect(area)
    } else {
        expanded_sidebar_toggle_rect(area)
    };
    if toggle_area == Rect::default() {
        return;
    }
    let icon = if collapsed { "»" } else { "«" };
    let icon_style = if collapsed && app.global_menu_attention_badge_visible() {
        Style::default().fg(p.accent).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(p.overlay0)
    };
    frame.render_widget(Paragraph::new(Span::styled(icon, icon_style)), toggle_area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{detect::Agent, layout::PaneId, workspace::Workspace};
    use ratatui::{backend::TestBackend, layout::Direction, Terminal};

    fn row_text(buffer: &ratatui::buffer::Buffer, row: u16, width: u16) -> String {
        (0..width)
            .map(|x| buffer[(x, row)].symbol())
            .collect::<String>()
            .trim_end()
            .to_string()
    }

    fn find_symbol_x(buffer: &ratatui::buffer::Buffer, row: u16, width: u16, symbol: &str) -> u16 {
        (0..width)
            .find(|x| buffer[(*x, row)].symbol() == symbol)
            .unwrap_or_else(|| {
                panic!(
                    "missing symbol {symbol:?} in row {}",
                    row_text(buffer, row, width)
                )
            })
    }

    #[test]
    fn expanded_and_collapsed_sidebars_use_custom_background() {
        let mut app = crate::app::state::AppState::test_new();
        app.workspaces.clear();
        app.active = None;
        app.palette.sidebar_bg = ratatui::style::Color::Rgb(12, 34, 56);
        let area = Rect::new(0, 0, 26, 20);

        let mut expanded = Terminal::new(TestBackend::new(26, 20)).unwrap();
        expanded
            .draw(|frame| render_sidebar(&app, &TerminalRuntimeRegistry::new(), frame, area))
            .unwrap();
        assert!(expanded
            .backend()
            .buffer()
            .content
            .iter()
            .all(|cell| cell.bg == app.palette.sidebar_bg));

        let mut collapsed = Terminal::new(TestBackend::new(26, 20)).unwrap();
        collapsed
            .draw(|frame| render_sidebar_collapsed(&app, frame, area))
            .unwrap();
        assert!(collapsed
            .backend()
            .buffer()
            .content
            .iter()
            .all(|cell| cell.bg == app.palette.sidebar_bg));
    }

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

    #[test]
    fn default_agent_rows_remove_redundant_state_text() {
        let mut app = crate::app::state::AppState::test_new();
        let workspace = Workspace::test_new("one");
        let pane_id = workspace.tabs[0].root_pane;
        app.workspaces = vec![workspace];
        app.ensure_test_terminals();
        app.active = Some(0);
        let terminal_id = app.workspaces[0].tabs[0].panes[&pane_id]
            .attached_terminal_id
            .clone();
        let terminal_state = app.terminals.get_mut(&terminal_id).unwrap();
        terminal_state.detected_agent = Some(Agent::Pi);
        terminal_state.state = AgentState::Working;

        let area = Rect::new(0, 0, 26, 20);
        let mut terminal = Terminal::new(TestBackend::new(26, 20)).unwrap();
        terminal
            .draw(|frame| render_sidebar(&app, &TerminalRuntimeRegistry::new(), frame, area))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let (_, agent_area) = expanded_sidebar_sections(area, app.sidebar_section_split);
        let body = agent_panel_body_rect(agent_area, false);

        let first = row_text(buffer, body.y, 25);
        let second = row_text(buffer, body.y + 1, 25);
        assert!(first.contains("one"));
        assert_eq!(second, "   pi");
        assert!(!first.contains("working"));
        assert!(!second.contains("working"));

        let workspace_x = find_symbol_x(buffer, body.y, body.width, "o");
        let workspace_style = buffer[(workspace_x, body.y)].style();
        assert_eq!(workspace_style.fg, Some(app.palette.text));
        assert!(workspace_style.add_modifier.contains(Modifier::BOLD));
        assert!(!workspace_style.add_modifier.contains(Modifier::DIM));
        assert_eq!(workspace_style.bg, Some(app.palette.active_row_bg));

        let agent_x = find_symbol_x(buffer, body.y + 1, body.width, "p");
        let agent_style = buffer[(agent_x, body.y + 1)].style();
        assert_eq!(agent_style.fg, Some(app.palette.overlay0));
        assert!(agent_style.add_modifier.contains(Modifier::DIM));
        assert!(!agent_style.add_modifier.contains(Modifier::BOLD));
        assert_eq!(agent_style.bg, Some(app.palette.active_row_bg));
    }

    #[test]
    fn occurrence_false_removes_default_workspace_bold_and_agent_dim() {
        let config: crate::config::Config = toml::from_str(
            r##"
[ui.sidebar.agents]
rows = [[{ token = "workspace", bold = false }, { token = "agent", dim = false }]]
"##,
        )
        .unwrap();
        let mut app = crate::app::state::AppState::test_new();
        app.sidebar_agents = config.ui.sidebar.agents;
        let workspace = Workspace::test_new("one");
        let pane_id = workspace.tabs[0].root_pane;
        app.workspaces = vec![workspace];
        app.ensure_test_terminals();
        app.active = Some(0);
        let terminal_id = app.workspaces[0].tabs[0].panes[&pane_id]
            .attached_terminal_id
            .clone();
        app.terminals.get_mut(&terminal_id).unwrap().detected_agent = Some(Agent::Pi);

        let area = Rect::new(0, 0, 26, 20);
        let mut terminal = Terminal::new(TestBackend::new(26, 20)).unwrap();
        terminal
            .draw(|frame| render_sidebar(&app, &TerminalRuntimeRegistry::new(), frame, area))
            .unwrap();
        let (_, agent_area) = expanded_sidebar_sections(area, app.sidebar_section_split);
        let body = agent_panel_body_rect(agent_area, false);
        let buffer = terminal.backend().buffer();
        let workspace = buffer[(find_symbol_x(buffer, body.y, body.width, "o"), body.y)].style();
        let agent = buffer[(find_symbol_x(buffer, body.y, body.width, "p"), body.y)].style();

        assert_eq!(workspace.fg, Some(app.palette.text));
        assert!(!workspace.add_modifier.contains(Modifier::BOLD));
        assert_eq!(agent.fg, Some(app.palette.overlay0));
        assert!(!agent.add_modifier.contains(Modifier::DIM));
    }

    #[test]
    fn default_space_workspace_style_tracks_active_state() {
        let mut app = crate::app::state::AppState::test_new();
        app.workspaces = vec![Workspace::test_new("one"), Workspace::test_new("two")];
        app.active = Some(0);
        app.mode = Mode::Terminal;
        let area = Rect::new(0, 0, 26, 20);
        app.view.workspace_card_areas = compute_workspace_card_areas(&app, area);
        let first_row = app.view.workspace_card_areas[0].rect.y;
        let second_row = app.view.workspace_card_areas[1].rect.y;
        let mut terminal = Terminal::new(TestBackend::new(26, 20)).unwrap();
        terminal
            .draw(|frame| render_sidebar(&app, &TerminalRuntimeRegistry::new(), frame, area))
            .unwrap();
        let buffer = terminal.backend().buffer();

        let active = buffer[(find_symbol_x(buffer, first_row, 25, "o"), first_row)].style();
        assert_eq!(active.fg, Some(app.palette.text));
        assert!(active.add_modifier.contains(Modifier::BOLD));
        assert!(!active.add_modifier.contains(Modifier::DIM));
        assert_eq!(active.bg, Some(app.palette.active_row_bg));

        let inactive = buffer[(find_symbol_x(buffer, second_row, 25, "t"), second_row)].style();
        assert_eq!(inactive.fg, Some(app.palette.subtext0));
        assert!(!inactive
            .add_modifier
            .intersects(Modifier::BOLD | Modifier::DIM));
        assert_eq!(inactive.bg, Some(ratatui::style::Color::Reset));
    }

    #[test]
    fn navigate_selection_keeps_its_existing_background_beside_active_workspace() {
        let mut app = crate::app::state::AppState::test_new();
        app.workspaces = vec![Workspace::test_new("one"), Workspace::test_new("two")];
        app.active = Some(0);
        app.selected = 1;
        app.mode = Mode::Navigate;
        let area = Rect::new(0, 0, 26, 20);
        app.view.workspace_card_areas = compute_workspace_card_areas(&app, area);
        let active_row = app.view.workspace_card_areas[0].rect.y;
        let selected_row = app.view.workspace_card_areas[1].rect.y;
        let mut terminal = Terminal::new(TestBackend::new(26, 20)).unwrap();
        terminal
            .draw(|frame| render_sidebar(&app, &TerminalRuntimeRegistry::new(), frame, area))
            .unwrap();
        let buffer = terminal.backend().buffer();

        assert_eq!(
            buffer[(0, active_row)].bg,
            app.palette.active_row_bg,
            "active workspace should keep its dedicated background"
        );
        assert_eq!(
            buffer[(0, selected_row)].bg,
            app.palette.selection_bg,
            "navigate selection should use its dedicated cursor background"
        );
    }

    #[test]
    fn selected_active_workspace_resolves_expanded_background() {
        let mut app = crate::app::state::AppState::test_new();
        app.palette = crate::app::state::Palette::terminal();
        app.workspaces = vec![Workspace::test_new("one"), Workspace::test_new("two")];
        app.active = Some(0);
        app.selected = 0;
        app.mode = Mode::Navigate;
        let area = Rect::new(0, 0, 26, 20);
        app.view.workspace_card_areas = compute_workspace_card_areas(&app, area);
        let active_row = app.view.workspace_card_areas[0].rect.y;
        let inactive_row = app.view.workspace_card_areas[1].rect.y;
        let mut terminal = Terminal::new(TestBackend::new(26, 20)).unwrap();
        terminal
            .draw(|frame| render_sidebar(&app, &TerminalRuntimeRegistry::new(), frame, area))
            .unwrap();

        assert_eq!(
            terminal.backend().buffer()[(0, active_row)].bg,
            app.palette.active_row_bg
        );

        app.selected = 1;
        terminal
            .draw(|frame| render_sidebar(&app, &TerminalRuntimeRegistry::new(), frame, area))
            .unwrap();
        assert_eq!(
            terminal.backend().buffer()[(0, active_row)].bg,
            app.palette.active_row_bg
        );
        assert_eq!(
            terminal.backend().buffer()[(0, inactive_row)].bg,
            app.palette.selection_bg
        );

        app.palette = crate::app::state::Palette::catppuccin();
        app.selected = 0;
        terminal
            .draw(|frame| render_sidebar(&app, &TerminalRuntimeRegistry::new(), frame, area))
            .unwrap();
        assert_eq!(
            terminal.backend().buffer()[(0, active_row)].bg,
            app.palette.selection_bg
        );
    }

    #[test]
    fn selected_active_workspace_resolves_collapsed_background() {
        let mut app = crate::app::state::AppState::test_new();
        app.palette = crate::app::state::Palette::terminal();
        app.workspaces = vec![Workspace::test_new("one"), Workspace::test_new("two")];
        app.active = Some(0);
        app.selected = 0;
        app.mode = Mode::Navigate;
        let area = Rect::new(0, 0, 5, 8);
        let mut terminal = Terminal::new(TestBackend::new(5, 8)).unwrap();
        terminal
            .draw(|frame| render_sidebar_collapsed(&app, frame, area))
            .unwrap();

        let (workspace_area, _, _) = collapsed_sidebar_sections(area);
        assert_eq!(
            terminal.backend().buffer()[(workspace_area.x, workspace_area.y)].bg,
            app.palette.active_row_bg
        );

        app.selected = 1;
        terminal
            .draw(|frame| render_sidebar_collapsed(&app, frame, area))
            .unwrap();
        assert_eq!(
            terminal.backend().buffer()[(workspace_area.x, workspace_area.y)].bg,
            app.palette.active_row_bg
        );
        assert_eq!(
            terminal.backend().buffer()[(workspace_area.x, workspace_area.y + 1)].bg,
            app.palette.selection_bg
        );

        app.palette = crate::app::state::Palette::catppuccin();
        app.selected = 0;
        terminal
            .draw(|frame| render_sidebar_collapsed(&app, frame, area))
            .unwrap();
        assert_eq!(
            terminal.backend().buffer()[(workspace_area.x, workspace_area.y)].bg,
            app.palette.selection_bg
        );
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

    #[test]
    fn space_occurrence_style_applies_without_styling_separator() {
        let config: crate::config::Config = toml::from_str(
            r##"
[ui.sidebar.spaces]
rows = [[{ token = "$hype", fg = "#abcdef", bold = true, dim = false }, "workspace"]]
"##,
        )
        .unwrap();
        let mut app = crate::app::state::AppState::test_new();
        app.sidebar_spaces = config.ui.sidebar.spaces;
        app.workspaces = vec![Workspace::test_new("one")];
        app.active = Some(0);
        app.mode = Mode::Terminal;
        app.workspaces[0].metadata_tokens.patch(
            std::collections::HashMap::from([("hype".into(), Some("HI".into()))]),
            None,
            std::time::Instant::now(),
        );

        let area = Rect::new(0, 0, 26, 20);
        app.view.workspace_card_areas = compute_workspace_card_areas(&app, area);
        let row = app.view.workspace_card_areas[0].rect.y;
        let mut terminal = Terminal::new(TestBackend::new(26, 20)).unwrap();
        terminal
            .draw(|frame| render_sidebar(&app, &TerminalRuntimeRegistry::new(), frame, area))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let h = buffer[(find_symbol_x(buffer, row, 25, "H"), row)].style();
        let i = buffer[(find_symbol_x(buffer, row, 25, "I"), row)].style();
        let separator = buffer[(find_symbol_x(buffer, row, 25, "·"), row)].style();

        for style in [h, i] {
            assert_eq!(style.fg, Some(ratatui::style::Color::Rgb(0xab, 0xcd, 0xef)));
            assert!(style.add_modifier.contains(Modifier::BOLD));
            assert!(!style.add_modifier.contains(Modifier::DIM));
            assert_eq!(style.bg, Some(app.palette.active_row_bg));
        }
        assert_eq!(separator.fg, Some(app.palette.overlay0));
        assert!(separator.add_modifier.contains(Modifier::DIM));
        assert!(!separator.add_modifier.contains(Modifier::BOLD));
        assert_eq!(separator.bg, Some(app.palette.active_row_bg));
    }

    #[test]
    fn occurrence_foreground_flattens_composite_git_status_colors() {
        let config: crate::config::Config = toml::from_str(
            r##"[ui.sidebar.spaces]
rows = [[{ token = "git_status", fg = "#123456" }]]
"##,
        )
        .unwrap();
        let spans = resolved_token_spans(
            &[ResolvedToken {
                kind: ResolvedTokenKind::GitStatus {
                    ahead: 2,
                    behind: 1,
                },
                style: config.ui.sidebar.spaces.rows[0][0].parts().1,
            }],
            ("", Style::default()),
            Style::default(),
            Style::default(),
            Style::default(),
            Style::default(),
            &crate::app::state::AppState::test_new().palette,
            20,
        );

        assert_eq!(
            spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>(),
            "↑2 ↓1"
        );
        assert!(spans
            .iter()
            .all(|span| { span.style.fg == Some(ratatui::style::Color::Rgb(0x12, 0x34, 0x56)) }));
    }

    #[test]
    fn default_agent_row_gap_packs_rendering_and_scroll_geometry() {
        let mut app = crate::app::state::AppState::test_new();
        app.workspaces = vec![Workspace::test_new("one"), Workspace::test_new("two")];
        app.ensure_test_terminals();
        for (workspace, agent) in app.workspaces.iter().zip([Agent::Pi, Agent::Claude]) {
            let pane_id = workspace.tabs[0].root_pane;
            let terminal_id = workspace.tabs[0].panes[&pane_id]
                .attached_terminal_id
                .clone();
            app.terminals.get_mut(&terminal_id).unwrap().detected_agent = Some(agent);
        }
        app.sidebar_agents.rows = vec![vec![crate::config::AgentSidebarToken::Agent]];
        assert_eq!(app.sidebar_agents.row_gap, 0);

        let area = Rect::new(0, 0, 20, 5);
        let metrics = agent_panel_scroll_metrics(&app, area);
        let body = agent_panel_body_rect(area, false);
        let mut terminal = Terminal::new(TestBackend::new(20, 5)).unwrap();
        terminal
            .draw(|frame| render_agent_detail(&app, &TerminalRuntimeRegistry::new(), frame, area))
            .unwrap();
        let buffer = terminal.backend().buffer();

        assert_eq!(metrics.viewport_rows, 2);
        assert_eq!(metrics.max_offset_from_bottom, 0);
        assert_eq!(row_text(buffer, body.y, body.width), " pi");
        assert_eq!(row_text(buffer, body.y + 1, body.width), " claude");
    }

    #[test]
    fn narrow_agent_rows_preserve_later_tab_tokens() {
        let mut app = crate::app::state::AppState::test_new();
        let mut workspace = Workspace::test_new("very-long-workspace-name");
        let tab_idx = workspace.test_add_tab(Some("logs"));
        let pane_id = workspace.tabs[tab_idx].root_pane;
        app.workspaces = vec![workspace];
        app.ensure_test_terminals();
        let terminal_id = app.workspaces[0].tabs[tab_idx].panes[&pane_id]
            .attached_terminal_id
            .clone();
        app.terminals.get_mut(&terminal_id).unwrap().detected_agent = Some(Agent::Pi);

        let area = Rect::new(0, 0, 18, 20);
        let mut terminal = Terminal::new(TestBackend::new(18, 20)).unwrap();
        terminal
            .draw(|frame| render_sidebar(&app, &TerminalRuntimeRegistry::new(), frame, area))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let (_, agent_area) = expanded_sidebar_sections(area, app.sidebar_section_split);
        let body = agent_panel_body_rect(agent_area, false);
        let first = row_text(buffer, body.y, 17);

        assert!(first.contains("logs"), "rendered row: {first:?}");
        assert!(first.contains('·'), "rendered row: {first:?}");
    }

    #[test]
    fn stripped_terminal_title_renders_with_unicode_width_truncation() {
        let mut app = crate::app::state::AppState::test_new();
        let workspace = Workspace::test_new("one");
        let pane_id = workspace.tabs[0].root_pane;
        app.workspaces = vec![workspace];
        app.ensure_test_terminals();
        let terminal_id = app.workspaces[0].tabs[0].panes[&pane_id]
            .attached_terminal_id
            .clone();
        let terminal = app.terminals.get_mut(&terminal_id).unwrap();
        terminal.detected_agent = Some(Agent::Claude);
        terminal.set_terminal_title(Some("⠋ 修复🙂标题很长".into()));
        app.sidebar_agents.rows = vec![vec![
            crate::config::AgentSidebarToken::TerminalTitleStripped,
        ]];

        let area = Rect::new(0, 0, 10, 12);
        let mut renderer = Terminal::new(TestBackend::new(10, 12)).unwrap();
        renderer
            .draw(|frame| render_sidebar(&app, &TerminalRuntimeRegistry::new(), frame, area))
            .unwrap();
        let (_, agent_area) = expanded_sidebar_sections(area, app.sidebar_section_split);
        let body = agent_panel_body_rect(agent_area, false);
        let rendered = row_text(renderer.backend().buffer(), body.y, 9);

        assert!(!rendered.contains('⠋'));
        assert!(rendered.contains('修') && rendered.contains('复'));

        let spans = resolved_token_spans(
            &[ResolvedToken::unstyled(ResolvedTokenKind::TerminalTitle(
                "修复🙂标题很长".into(),
            ))],
            ("", Style::default()),
            Style::default(),
            Style::default(),
            Style::default(),
            Style::default(),
            &app.palette,
            8,
        );
        let text = spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();
        assert!(display_width(&text) <= 8, "resolved title: {text:?}");
    }

    #[test]
    fn variable_agent_heights_pack_the_bottom_and_reveal_targets() {
        let mut app = crate::app::state::AppState::test_new();
        app.workspaces = vec![
            Workspace::test_new("one"),
            Workspace::test_new("two"),
            Workspace::test_new("three"),
        ];
        app.ensure_test_terminals();
        for workspace in &app.workspaces {
            let pane_id = workspace.tabs[0].root_pane;
            let terminal_id = workspace.tabs[0].panes[&pane_id]
                .attached_terminal_id
                .clone();
            app.terminals.get_mut(&terminal_id).unwrap().detected_agent = Some(Agent::Pi);
        }
        let first_pane = app.workspaces[0].tabs[0].root_pane;
        let first_terminal = app.workspaces[0].tabs[0].panes[&first_pane]
            .attached_terminal_id
            .clone();
        app.terminals
            .get_mut(&first_terminal)
            .unwrap()
            .metadata_tokens
            .patch(
                std::collections::HashMap::from([
                    ("a".into(), Some("a".into())),
                    ("b".into(), Some("b".into())),
                ]),
                None,
                std::time::Instant::now(),
            );
        app.sidebar_agents.rows = vec![
            vec![crate::config::AgentSidebarToken::Agent],
            vec![crate::config::AgentSidebarToken::Custom("a".into())],
            vec![crate::config::AgentSidebarToken::Custom("b".into())],
        ];
        let area = Rect::new(0, 0, 20, 6);

        let metrics = agent_panel_scroll_metrics(&app, area);
        assert_eq!(metrics.max_offset_from_bottom, 1);
        assert_eq!(agent_panel_scroll_for_target(&app, area, 0, 2), 1);
    }

    #[test]
    fn oversized_space_layout_is_clipped_to_the_section_body() {
        let mut app = crate::app::state::AppState::test_new();
        app.workspaces = vec![Workspace::test_new("one"), Workspace::test_new("two")];
        app.sidebar_spaces.rows = vec![vec![crate::config::SpaceSidebarToken::Workspace]; 6];
        let area = Rect::new(0, 0, 20, 10);
        let workspace_area = workspace_list_rect(area, app.sidebar_section_split);
        let body = workspace_list_body_rect(workspace_area, false);

        let metrics = workspace_list_scroll_metrics(&app, workspace_area);
        let (cards, _) = compute_workspace_list_areas(&app, area);

        assert_eq!(metrics.viewport_rows, 1);
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].ws_idx, 0);
        assert_eq!(cards[0].rect.height, body.height);
    }

    #[test]
    fn oversized_agent_override_is_clipped_to_the_panel_body() {
        let mut app = crate::app::state::AppState::test_new();
        let workspace = Workspace::test_new("one");
        let pane_id = workspace.tabs[0].root_pane;
        app.workspaces = vec![workspace];
        app.ensure_test_terminals();
        let terminal_id = app.workspaces[0].tabs[0].panes[&pane_id]
            .attached_terminal_id
            .clone();
        app.terminals.get_mut(&terminal_id).unwrap().detected_agent = Some(Agent::Claude);
        app.sidebar_agents.rows_by_agent.insert(
            "claude".into(),
            vec![vec![crate::config::AgentSidebarToken::Agent]; 6],
        );
        let panel = Rect::new(0, 0, 20, 5);

        let metrics = agent_panel_scroll_metrics(&app, panel);

        assert_eq!(metrics.viewport_rows, 1);
        assert_eq!(metrics.max_offset_from_bottom, 0);
        let entry = agent_panel_entries(&app).pop().unwrap();
        assert_eq!(
            agent_entry_height_in_body(&app, &entry, agent_panel_body_rect(panel, false).height),
            agent_panel_body_rect(panel, false).height
        );
    }

    #[test]
    fn render_sidebar_toggle_draws_expanded_collapse_icon() {
        let app = crate::app::state::AppState::test_new();
        let area = Rect::new(0, 0, 26, 20);
        let mut terminal =
            Terminal::new(TestBackend::new(26, 20)).expect("test terminal should initialize");

        terminal
            .draw(|frame| render_sidebar_toggle(&app, frame, area, false, &app.palette))
            .expect("sidebar toggle should render");

        let toggle = expanded_sidebar_toggle_rect(area);
        assert_eq!(
            terminal.backend().buffer()[(toggle.x, toggle.y)].symbol(),
            "«"
        );
    }

    #[test]
    fn expanded_sidebar_toggle_sits_inside_sidebar_content() {
        let area = Rect::new(0, 0, 26, 20);
        let toggle = expanded_sidebar_toggle_rect(area);

        assert_eq!(toggle.x, area.x + area.width - 2);
        assert_eq!(toggle.y, area.y + area.height - 1);
    }

    #[test]
    fn agent_panel_tab_label_visibility_tracks_tab_identity() {
        let mut app = crate::app::state::AppState::test_new();
        let single_auto = Workspace::test_new("auto");
        let mut single_custom = Workspace::test_new("custom");
        single_custom.tabs[0].set_custom_name("focus".into());
        let mut multi = Workspace::test_new("multi");
        multi.test_add_tab(Some("logs"));

        app.workspaces = vec![single_auto, single_custom, multi];
        app.ensure_test_terminals();
        for (ws_idx, tab_idx, agent) in [
            (0, 0, Agent::Pi),
            (1, 0, Agent::Claude),
            (2, 0, Agent::Codex),
            (2, 1, Agent::Pi),
        ] {
            let pane_id = app.workspaces[ws_idx].tabs[tab_idx].root_pane;
            let terminal_id = app.workspaces[ws_idx].tabs[tab_idx].panes[&pane_id]
                .attached_terminal_id
                .clone();
            app.terminals.get_mut(&terminal_id).unwrap().detected_agent = Some(agent);
        }

        let entries = agent_panel_entries(&app);
        let labels: Vec<_> = entries
            .iter()
            .map(|entry| {
                (
                    entry.primary_label.as_str(),
                    entry.primary_tab_label.as_deref(),
                )
            })
            .collect();

        assert_eq!(
            labels,
            [
                ("auto", None),
                ("custom", Some("focus")),
                ("multi", Some("1")),
                ("multi", Some("logs")),
            ]
        );
    }

    #[test]
    fn priority_agent_panel_sort_uses_attention_then_space_order() {
        let mut app = crate::app::state::AppState::test_new();
        app.workspaces = vec![
            Workspace::test_new("one"),
            Workspace::test_new("two"),
            Workspace::test_new("three"),
            Workspace::test_new("four"),
        ];
        app.ensure_test_terminals();
        app.active = Some(0);
        app.selected = 0;
        app.agent_panel_sort = crate::app::state::AgentPanelSort::Priority;

        let set_state = |app: &mut crate::app::state::AppState, ws_idx: usize, state| {
            let pane = app.workspaces[ws_idx].tabs[0].root_pane;
            let terminal_id = app.workspaces[ws_idx].tabs[0].panes[&pane]
                .attached_terminal_id
                .clone();
            let terminal = app.terminals.get_mut(&terminal_id).unwrap();
            terminal.detected_agent = Some(Agent::Claude);
            terminal.state = state;
        };
        set_state(&mut app, 0, AgentState::Working);
        set_state(&mut app, 1, AgentState::Idle);
        set_state(&mut app, 2, AgentState::Working);
        set_state(&mut app, 3, AgentState::Blocked);

        let done_pane = app.workspaces[1].tabs[0].root_pane;
        app.workspaces[1].tabs[0]
            .panes
            .get_mut(&done_pane)
            .unwrap()
            .seen = false;

        let labels: Vec<String> = agent_panel_entries(&app)
            .into_iter()
            .map(|entry| entry.primary_label)
            .collect();

        assert_eq!(labels, ["four", "two", "one", "three"]);
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

    #[test]
    fn collapsed_sidebar_numbers_grouped_agents_by_list_position() {
        let mut app = crate::app::state::AppState::test_new();
        app.workspaces = vec![Workspace::test_new("one"), Workspace::test_new("two")];
        app.ensure_test_terminals();

        for ws_idx in 0..app.workspaces.len() {
            let pane = app.workspaces[ws_idx].tabs[0].root_pane;
            let terminal_id = app.workspaces[ws_idx].tabs[0].panes[&pane]
                .attached_terminal_id
                .clone();
            app.terminals.get_mut(&terminal_id).unwrap().detected_agent = Some(Agent::Claude);
        }

        let area = Rect::new(0, 0, 4, 12);
        let (_, _, detail_area) = collapsed_sidebar_sections(area);
        let mut terminal = Terminal::new(TestBackend::new(area.width, area.height))
            .expect("test terminal should initialize");

        terminal
            .draw(|frame| render_sidebar_collapsed(&app, frame, area))
            .expect("collapsed sidebar should render");

        let buffer = terminal.backend().buffer();
        assert_eq!(buffer[(detail_area.x, detail_area.y)].symbol(), "1");
        assert_eq!(buffer[(detail_area.x, detail_area.y + 1)].symbol(), "2");
    }

    /// Two agent panes in one workspace plus a second workspace, so the
    /// assertions can tell pane-level highlighting apart from workspace-level.
    fn collapsed_agent_app() -> (crate::app::state::AppState, PaneId, PaneId) {
        let mut app = crate::app::state::AppState::test_new();
        let mut first = Workspace::test_new("one");
        let second_pane = first.test_split(Direction::Horizontal);
        let first_pane = first.tabs[0].root_pane;
        app.workspaces = vec![first, Workspace::test_new("two")];
        app.ensure_test_terminals();

        let terminal_ids: Vec<_> = app
            .workspaces
            .iter()
            .flat_map(|ws| ws.tabs.iter())
            .flat_map(|tab| tab.panes.values())
            .map(|pane| pane.attached_terminal_id.clone())
            .collect();
        for terminal_id in terminal_ids {
            app.terminals.get_mut(&terminal_id).unwrap().detected_agent = Some(Agent::Claude);
        }

        (app, first_pane, second_pane)
    }

    fn collapsed_agent_row_styles(
        app: &crate::app::state::AppState,
        area: Rect,
        detail_area: Rect,
        rows: u16,
    ) -> Vec<Vec<ratatui::style::Style>> {
        let mut terminal = Terminal::new(TestBackend::new(area.width, area.height))
            .expect("test terminal should initialize");
        terminal
            .draw(|frame| render_sidebar_collapsed(app, frame, area))
            .expect("collapsed sidebar should render");
        let buffer = terminal.backend().buffer();
        (0..rows)
            .map(|row| {
                (detail_area.x..detail_area.x + detail_area.width)
                    .map(|x| buffer[(x, detail_area.y + row)].style())
                    .collect()
            })
            .collect()
    }

    #[test]
    fn collapsed_sidebar_highlights_only_the_focused_agent_pane() {
        let (mut app, first_pane, second_pane) = collapsed_agent_app();
        app.active = Some(0);
        app.workspaces[0].tabs[0].layout.focus_pane(second_pane);
        assert!(app.is_active_pane(0, 0, second_pane));
        assert!(!app.is_active_pane(0, 0, first_pane));

        let area = Rect::new(0, 0, 4, 14);
        let (_, _, detail_area) = collapsed_sidebar_sections(area);
        let rows = collapsed_agent_row_styles(&app, area, detail_area, 3);

        let highlighted: Vec<_> = rows
            .iter()
            .filter(|cells| {
                cells
                    .iter()
                    .all(|style| style.bg == Some(app.palette.active_row_bg))
            })
            .collect();
        assert_eq!(
            highlighted.len(),
            1,
            "only the focused agent pane should be highlighted, across the whole row"
        );
        assert_eq!(highlighted[0][0].fg, Some(app.palette.text));

        let muted = rows
            .iter()
            .filter(|cells| cells[0].fg == Some(app.palette.overlay0))
            .count();
        assert_eq!(
            muted, 2,
            "the sibling pane in the active workspace and the other workspace stay muted"
        );
    }

    #[test]
    fn collapsed_sidebar_does_not_highlight_agents_without_active_workspace() {
        let (mut app, _, _) = collapsed_agent_app();
        app.active = None;

        let area = Rect::new(0, 0, 4, 14);
        let (_, _, detail_area) = collapsed_sidebar_sections(area);
        let rows = collapsed_agent_row_styles(&app, area, detail_area, 3);

        for cells in rows {
            assert_eq!(cells[0].fg, Some(app.palette.overlay0));
            for style in cells {
                assert_ne!(style.bg, Some(app.palette.active_row_bg));
            }
        }
    }

    #[test]
    fn collapsed_sidebar_keeps_workspace_status_visible_for_two_digit_positions() {
        let mut app = crate::app::state::AppState::test_new();
        app.workspaces = (1..=10)
            .map(|idx| Workspace::test_new(&format!("workspace-{idx}")))
            .collect();
        app.ensure_test_terminals();

        for ws_idx in 0..app.workspaces.len() {
            let pane = app.workspaces[ws_idx].tabs[0].root_pane;
            let terminal_id = app.workspaces[ws_idx].tabs[0].panes[&pane]
                .attached_terminal_id
                .clone();
            app.terminals.get_mut(&terminal_id).unwrap().detected_agent = Some(Agent::Claude);
        }

        let area = Rect::new(0, 0, 4, 25);
        let (workspace_area, _, _) = collapsed_sidebar_sections(area);
        let mut terminal = Terminal::new(TestBackend::new(area.width, area.height))
            .expect("test terminal should initialize");

        terminal
            .draw(|frame| render_sidebar_collapsed(&app, frame, area))
            .expect("collapsed sidebar should render");

        let tenth_row = workspace_area.y + 9;
        let buffer = terminal.backend().buffer();
        assert_eq!(buffer[(workspace_area.x, workspace_area.y)].symbol(), "1");
        assert_eq!(
            buffer[(workspace_area.x + 1, workspace_area.y)].symbol(),
            " "
        );
        assert_eq!(
            buffer[(workspace_area.x + 2, workspace_area.y)].symbol(),
            "·"
        );
        assert_eq!(buffer[(workspace_area.x, tenth_row)].symbol(), "1");
        assert_eq!(buffer[(workspace_area.x + 1, tenth_row)].symbol(), "0");
        assert_eq!(buffer[(workspace_area.x + 2, tenth_row)].symbol(), "·");
    }

    #[test]
    fn collapsed_sidebar_keeps_status_visible_for_two_digit_positions() {
        let mut app = crate::app::state::AppState::test_new();
        app.workspaces = (1..=10)
            .map(|idx| Workspace::test_new(&format!("workspace-{idx}")))
            .collect();
        app.ensure_test_terminals();

        for ws_idx in 0..app.workspaces.len() {
            let pane = app.workspaces[ws_idx].tabs[0].root_pane;
            let terminal_id = app.workspaces[ws_idx].tabs[0].panes[&pane]
                .attached_terminal_id
                .clone();
            app.terminals.get_mut(&terminal_id).unwrap().detected_agent = Some(Agent::Claude);
        }

        let area = Rect::new(0, 0, 4, 25);
        let (_, _, detail_area) = collapsed_sidebar_sections(area);
        let mut terminal = Terminal::new(TestBackend::new(area.width, area.height))
            .expect("test terminal should initialize");

        terminal
            .draw(|frame| render_sidebar_collapsed(&app, frame, area))
            .expect("collapsed sidebar should render");

        let tenth_row = detail_area.y + 9;
        let buffer = terminal.backend().buffer();
        assert_eq!(buffer[(detail_area.x, tenth_row)].symbol(), "1");
        assert_eq!(buffer[(detail_area.x + 1, tenth_row)].symbol(), "0");
        assert_eq!(buffer[(detail_area.x + 2, tenth_row)].symbol(), "·");
    }

    #[test]
    fn collapsed_sidebar_numbers_priority_agents_by_list_position() {
        let first = Workspace::test_new("one");
        let first_pane = first.tabs[0].root_pane;
        let mut second = Workspace::test_new("two");
        let second_pane = second.tabs[0].root_pane;
        let urgent_pane = second.test_split(ratatui::layout::Direction::Horizontal);

        let mut app = crate::app::state::AppState::test_new();
        app.workspaces = vec![first, second];
        app.ensure_test_terminals();
        app.agent_panel_sort = crate::app::state::AgentPanelSort::Priority;
        app.status_indicators = crate::config::StatusIndicatorStyle::Symbols;

        let set_state = |app: &mut crate::app::state::AppState, ws_idx: usize, pane_id, state| {
            let terminal_id = app.workspaces[ws_idx].tabs[0].panes[&pane_id]
                .attached_terminal_id
                .clone();
            let terminal = app.terminals.get_mut(&terminal_id).unwrap();
            terminal.detected_agent = Some(Agent::Claude);
            terminal.state = state;
        };
        set_state(&mut app, 0, first_pane, AgentState::Idle);
        set_state(&mut app, 1, second_pane, AgentState::Working);
        set_state(&mut app, 1, urgent_pane, AgentState::Blocked);
        app.workspaces[0].tabs[0]
            .panes
            .get_mut(&first_pane)
            .unwrap()
            .seen = false;

        assert_eq!(app.workspaces[1].public_pane_number(urgent_pane), Some(2));
        assert_eq!(agent_panel_entries(&app)[0].pane_id, urgent_pane);

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
        assert_eq!(buffer[(detail_area.x + 2, detail_area.y)].symbol(), "×");
        assert_eq!(
            buffer[(detail_area.x + 2, detail_area.y)].style().fg,
            Some(app.palette.red)
        );
        assert_eq!(buffer[(detail_area.x + 2, detail_area.y + 1)].symbol(), "✓");
        assert_eq!(
            buffer[(detail_area.x + 2, detail_area.y + 1)].style().fg,
            Some(app.palette.teal)
        );
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

    #[cfg(unix)]
    #[tokio::test]
    async fn all_workspaces_agent_panel_entries_use_live_root_runtime_cwd_for_workspace_label() {
        let unique = format!(
            "herdr-agent-panel-runtime-cwd-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let root = std::env::temp_dir().join(unique);
        let stale_cwd = root.join("issue-264-nix-support");
        let live_cwd = root.join("herdr");
        std::fs::create_dir_all(stale_cwd.join(".git")).unwrap();
        std::fs::create_dir_all(live_cwd.join(".git")).unwrap();

        let mut app = crate::app::state::AppState::test_new();
        let mut workspace = Workspace::test_new("stale-name");
        workspace.custom_name = None;
        workspace.identity_cwd = stale_cwd.clone();
        let pane = workspace.tabs[0].root_pane;

        app.workspaces = vec![workspace];
        app.ensure_test_terminals();
        let terminal_id = app.workspaces[0].tabs[0].panes[&pane]
            .attached_terminal_id
            .clone();
        let terminal = app.terminals.get_mut(&terminal_id).unwrap();
        terminal.cwd = stale_cwd;
        terminal.detected_agent = Some(Agent::Pi);
        app.active = Some(0);
        app.selected = 0;

        let (events, _) = tokio::sync::mpsc::channel(4);
        let runtime = crate::terminal::TerminalRuntime::spawn(
            pane,
            24,
            80,
            live_cwd.clone(),
            0,
            crate::terminal_theme::TerminalTheme::default(),
            None,
            crate::pane::PaneShellConfig::new("/bin/sh", crate::config::ShellModeConfig::NonLogin),
            &crate::pane::PaneLaunchEnv::default(),
            events,
            std::sync::Arc::new(tokio::sync::Notify::new()),
            std::sync::Arc::new(crate::render_signal::RenderSignal::new()),
        )
        .unwrap();

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while runtime.cwd() != Some(live_cwd.clone()) && std::time::Instant::now() < deadline {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }

        let mut runtime_registry = TerminalRuntimeRegistry::new();
        runtime_registry.insert(terminal_id, runtime);
        let entries = agent_panel_entries_from(&app, &runtime_registry);
        let primary_label = entries[0].primary_label.clone();

        for (_, runtime) in runtime_registry.drain() {
            runtime.shutdown();
        }
        let _ = std::fs::remove_dir_all(root);

        assert_eq!(primary_label, "herdr");
    }

    #[test]
    fn all_workspaces_agent_panel_entries_prefer_agent_names_for_agent_identity() {
        let mut app = crate::app::state::AppState::test_new();
        let workspace = Workspace::test_new("bridge");
        let first_pane = workspace.tabs[0].root_pane;

        app.workspaces = vec![workspace];
        app.ensure_test_terminals();
        let first_terminal_id = app.workspaces[0].tabs[0].panes[&first_pane]
            .attached_terminal_id
            .clone();
        app.terminals
            .get_mut(&first_terminal_id)
            .unwrap()
            .detected_agent = Some(Agent::Pi);
        app.terminals
            .get_mut(&first_terminal_id)
            .unwrap()
            .set_agent_name("planner".into());
        app.active = Some(0);
        app.selected = 0;

        let entries = agent_panel_entries(&app);
        assert_eq!(entries[0].primary_label, "bridge");
        assert_eq!(entries[0].agent_label.as_deref(), Some("planner"));
    }

    #[test]
    fn expanded_sidebar_sections_handle_tiny_heights() {
        let (ws_area, detail_area) = expanded_sidebar_sections(Rect::new(0, 0, 20, 5), 0.9);

        assert_eq!(ws_area, Rect::new(0, 0, 19, 3));
        assert_eq!(detail_area, Rect::new(0, 3, 19, 2));
    }

    #[test]
    fn sidebar_section_divider_is_hidden_for_tiny_heights() {
        let divider = sidebar_section_divider_rect(Rect::new(0, 0, 20, 5), 0.5);

        assert_eq!(divider, Rect::default());
    }

    #[test]
    fn grouped_child_label_keeps_custom_workspace_name() {
        assert_eq!(
            grouped_child_display_label("renamed issue", Some("worktree/issue-137"), true),
            "renamed issue"
        );
    }

    #[test]
    fn grouped_child_label_uses_short_branch_for_auto_named_workspace() {
        assert_eq!(
            grouped_child_display_label("herdr-issue", Some("worktree/issue-137"), false),
            "issue-137"
        );
    }

    #[test]
    fn workspace_list_truncates_cjk_branch_without_panic() {
        let mut app = crate::app::state::AppState::test_new();
        let mut ws = Workspace::test_new("repo");
        ws.cached_git_branch = Some("feature/中文-分支-644".into());
        app.workspaces = vec![ws];
        app.active = Some(0);
        app.selected = 0;
        app.mode = Mode::Terminal;
        app.view.workspace_card_areas = vec![crate::app::state::WorkspaceCardArea {
            ws_idx: 0,
            rect: Rect::new(0, 1, 15, 2),
            indented: false,
            foldered: false,
        }];

        let mut terminal = Terminal::new(TestBackend::new(15, 6)).expect("test terminal");
        let runtimes = crate::terminal::TerminalRuntimeRegistry::new();

        terminal
            .draw(|frame| {
                render_workspace_list(&app, &runtimes, frame, Rect::new(0, 0, 15, 6), false)
            })
            .expect("workspace list should render");
    }

    fn workspace_with_worktree_space(
        name: &str,
        key: Option<&str>,
        checkout_key: &str,
    ) -> crate::workspace::Workspace {
        let mut ws = crate::workspace::Workspace::test_new(name);
        if let Some(key) = key {
            ws.worktree_space = Some(crate::workspace::WorktreeSpaceMembership {
                key: key.into(),
                label: "herdr".into(),
                repo_root: std::path::PathBuf::from("/repo/herdr"),
                checkout_path: std::path::PathBuf::from(checkout_key),
                is_linked_worktree: name != "main",
            });
        }
        ws
    }

    fn workspace_with_git_space(name: &str, key: &str) -> crate::workspace::Workspace {
        let mut ws = crate::workspace::Workspace::test_new(name);
        ws.cached_git_space = Some(crate::workspace::GitSpaceMetadata {
            key: key.into(),
            checkout_key: format!("/repo/{name}"),
            repo_name: "herdr".into(),
            repo_root: std::path::PathBuf::from(format!("/repo/{name}")),
            is_linked_worktree: false,
        });
        ws
    }

    #[test]
    fn desktop_worktree_tree_aligns_parents_and_marks_children() {
        let mut app = AppState::test_new();
        app.workspaces = vec![
            workspace_with_worktree_space("main", Some("repo-key"), "/repo/herdr"),
            workspace_with_worktree_space("issue", Some("repo-key"), "/repo/herdr-issue"),
            workspace_with_worktree_space("review", Some("repo-key"), "/repo/herdr-review"),
            Workspace::test_new("notes"),
        ];
        app.sidebar_spaces.rows = vec![vec![
            crate::config::SpaceSidebarToken::StateIcon,
            crate::config::SpaceSidebarToken::Workspace,
        ]];
        app.sidebar_spaces.row_gap = 0;
        let area = Rect::new(0, 0, 30, 20);
        app.view.workspace_card_areas = compute_workspace_card_areas(&app, area);
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
        let parent_name_x = find_symbol_x(buffer, cards[0].rect.y, cards[0].rect.width, "m");
        let plain_name_x = find_symbol_x(buffer, cards[3].rect.y, cards[3].rect.width, "n");
        assert_eq!(parent_name_x, plain_name_x);
        assert_eq!(buffer[(cards[1].rect.x + 3, cards[1].rect.y)].symbol(), "├");
        assert_eq!(buffer[(cards[2].rect.x + 3, cards[2].rect.y)].symbol(), "└");
        // The group chevron stays at the parent card's right edge.
        assert_eq!(
            buffer[(cards[0].rect.x + cards[0].rect.width - 1, cards[0].rect.y)].symbol(),
            "▾"
        );
    }

    #[test]
    fn desktop_worktree_connector_uses_full_list_at_viewport_boundary() {
        let mut app = AppState::test_new();
        app.workspaces = vec![
            workspace_with_worktree_space("main", Some("repo-key"), "/repo/herdr"),
            workspace_with_worktree_space("issue", Some("repo-key"), "/repo/herdr-issue"),
            workspace_with_worktree_space("review", Some("repo-key"), "/repo/herdr-review"),
        ];
        app.sidebar_spaces.rows = vec![vec![crate::config::SpaceSidebarToken::Workspace]];
        app.sidebar_spaces.row_gap = 0;
        let area = Rect::new(0, 0, 30, 10);
        app.view.workspace_card_areas = compute_workspace_card_areas(&app, area);
        assert_eq!(app.view.workspace_card_areas.len(), 2);
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

        let child = app.view.workspace_card_areas[1];
        assert_eq!(
            terminal.backend().buffer()[(child.rect.x + 3, child.rect.y)].symbol(),
            "├"
        );
    }

    #[test]
    fn parent_workspace_row_stays_clickable_when_grouped() {
        let mut app = AppState::test_new();
        app.workspaces = vec![
            workspace_with_worktree_space("main", Some("repo-key"), "/repo/herdr"),
            workspace_with_worktree_space("issue", Some("repo-key"), "/repo/herdr-issue"),
        ];
        app.sidebar_spaces.row_gap = 1;

        let (cards, headers) = compute_workspace_list_areas(&app, Rect::new(0, 0, 30, 20));

        assert!(headers.is_empty());
        assert_eq!(cards[0].ws_idx, 0);
        assert!(!cards[0].indented);
        assert_eq!(cards[1].ws_idx, 1);
        assert!(cards[1].indented);
        assert_eq!(cards[1].rect.y, cards[0].rect.y + cards[0].rect.height);
    }

    #[test]
    fn space_row_gap_preserves_compact_worktree_children() {
        let mut app = AppState::test_new();
        app.workspaces = vec![
            workspace_with_worktree_space("main", Some("repo-key"), "/repo/herdr"),
            workspace_with_worktree_space("issue", Some("repo-key"), "/repo/herdr-issue"),
            workspace_with_worktree_space("review", Some("repo-key"), "/repo/herdr-review"),
            Workspace::test_new("notes"),
        ];
        app.sidebar_spaces.rows = vec![vec![crate::config::SpaceSidebarToken::Workspace]];
        app.sidebar_spaces.row_gap = 2;

        let (spacious, _) = compute_workspace_list_areas(&app, Rect::new(0, 0, 30, 30));
        assert_eq!(
            spacious[1].rect.y,
            spacious[0].rect.y + spacious[0].rect.height
        );
        assert_eq!(
            spacious[2].rect.y,
            spacious[1].rect.y + spacious[1].rect.height
        );
        assert_eq!(
            spacious[3].rect.y,
            spacious[2].rect.y + spacious[2].rect.height + 2
        );
        let spacious_metrics = workspace_list_scroll_metrics(&app, Rect::new(0, 0, 30, 7));
        assert_eq!(spacious_metrics.viewport_rows, 3);
        assert_eq!(spacious_metrics.max_offset_from_bottom, 2);

        app.sidebar_spaces.row_gap = 0;
        let (packed, _) = compute_workspace_list_areas(&app, Rect::new(0, 0, 30, 30));
        assert!(packed
            .windows(2)
            .all(|pair| pair[1].rect.y == pair[0].rect.y + pair[0].rect.height));
        let packed_metrics = workspace_list_scroll_metrics(&app, Rect::new(0, 0, 30, 7));
        assert_eq!(packed_metrics.viewport_rows, 4);
        assert_eq!(packed_metrics.max_offset_from_bottom, 0);
    }

    #[test]
    fn packed_workspace_drag_indicator_overlays_an_internal_boundary() {
        let mut app = AppState::test_new();
        app.workspaces = vec![
            Workspace::test_new("a"),
            Workspace::test_new("b"),
            Workspace::test_new("c"),
        ];
        app.sidebar_spaces.rows = vec![vec![crate::config::SpaceSidebarToken::Workspace]];
        app.sidebar_spaces.row_gap = 0;
        let area = Rect::new(0, 0, 30, 20);
        app.view.workspace_card_areas = compute_workspace_card_areas(&app, area);
        let list_area = workspace_list_rect(area, app.sidebar_section_split);
        let indicator_row = workspace_drop_indicator_row(
            &app,
            &app.view.workspace_card_areas,
            &app.view.folder_header_areas,
            list_area,
            &crate::app::state::WorkspaceDropTarget::Before(2),
        )
        .unwrap();
        assert_eq!(indicator_row, app.view.workspace_card_areas[1].rect.y);
        app.drag = Some(crate::app::state::DragState {
            target: crate::app::state::DragTarget::WorkspaceReorder {
                source_id: 0,
                source_ws_idx: 0,
                drop_target: Some(crate::app::state::WorkspaceDropTarget::Before(2)),
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

        assert_eq!(
            terminal.backend().buffer()[(list_area.x, indicator_row)].symbol(),
            "─"
        );
    }

    #[test]
    fn linked_only_worktree_members_do_not_form_parentless_group() {
        let mut app = AppState::test_new();
        app.workspaces = vec![
            workspace_with_worktree_space("issue", Some("repo-key"), "/repo/herdr-issue"),
            workspace_with_worktree_space("review", Some("repo-key"), "/repo/herdr-review"),
        ];

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
            ]
        );
    }

    #[test]
    fn compact_space_group_scroll_clamps_when_all_entries_fit() {
        let mut app = AppState::test_new();
        app.workspaces = vec![
            workspace_with_worktree_space("main", Some("repo-key"), "/repo/herdr"),
            workspace_with_worktree_space("one", Some("repo-key"), "/repo/herdr-one"),
            workspace_with_worktree_space("two", Some("repo-key"), "/repo/herdr-two"),
        ];
        let area = Rect::new(0, 0, 30, 20);
        app.workspace_scroll = normalized_workspace_scroll(&app, area, 2);

        let (cards, headers) = compute_workspace_list_areas(&app, area);

        assert!(headers.is_empty());
        assert_eq!(app.workspace_scroll, 0);
        assert_eq!(cards.len(), 3);
        assert_eq!(cards[2].ws_idx, 2);
    }

    #[test]
    fn workspace_scroll_metrics_count_display_entries_not_raw_workspaces() {
        let mut app = AppState::test_new();
        app.workspaces = vec![
            workspace_with_worktree_space("main", Some("repo-key"), "/repo/herdr"),
            workspace_with_worktree_space("issue", Some("repo-key"), "/repo/herdr-issue"),
            Workspace::test_new("notes"),
        ];
        for workspace in &mut app.workspaces {
            workspace.cached_git_branch = Some("main".into());
        }
        app.collapsed_space_keys.insert("repo-key".into());
        app.active = None;
        app.mode = Mode::Terminal;

        let ws_area = Rect::new(0, 0, 30, 6);
        let metrics = workspace_list_scroll_metrics(&app, ws_area);

        assert_eq!(metrics.viewport_rows, 1);
        assert_eq!(metrics.max_offset_from_bottom, 1);
        assert_eq!(metrics.offset_from_bottom, 1);
    }

    #[test]
    fn workspace_scroll_offset_applies_to_group_children() {
        let mut app = AppState::test_new();
        app.workspaces = vec![
            workspace_with_worktree_space("main", Some("repo-key"), "/repo/herdr"),
            workspace_with_worktree_space("issue", Some("repo-key"), "/repo/herdr-issue"),
            Workspace::test_new("notes"),
        ];
        app.collapsed_space_keys.insert("repo-key".into());
        app.active = None;
        app.mode = Mode::Terminal;
        app.workspace_scroll = 1;

        let (cards, headers) = compute_workspace_list_areas(&app, Rect::new(0, 0, 30, 12));

        assert!(headers.is_empty());
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].ws_idx, 2);
    }

    #[test]
    fn workspace_list_entries_group_multiple_workspaces_in_same_git_space() {
        let mut app = AppState::test_new();
        app.workspaces = vec![
            workspace_with_worktree_space("main", Some("repo-key"), "/repo/herdr"),
            workspace_with_worktree_space("issue", Some("repo-key"), "/repo/herdr-issue"),
        ];

        assert_eq!(
            workspace_list_entries(&app),
            vec![
                WorkspaceListEntry::Workspace {
                    ws_idx: 0,
                    indented: false,
                    foldered: false,
                },
                WorkspaceListEntry::Workspace {
                    ws_idx: 1,
                    indented: true,
                    foldered: false,
                },
            ]
        );
    }

    #[test]
    fn workspace_list_entries_group_non_contiguous_explicit_members() {
        let mut app = AppState::test_new();
        app.workspaces = vec![
            workspace_with_worktree_space("main", Some("repo-key"), "/repo/herdr"),
            workspace_with_git_space("normal", "other-key"),
            workspace_with_worktree_space("issue", Some("repo-key"), "/repo/herdr-issue"),
        ];

        assert_eq!(
            workspace_list_entries(&app),
            vec![
                WorkspaceListEntry::Workspace {
                    ws_idx: 0,
                    indented: false,
                    foldered: false,
                },
                WorkspaceListEntry::Workspace {
                    ws_idx: 2,
                    indented: true,
                    foldered: false,
                },
                WorkspaceListEntry::Workspace {
                    ws_idx: 1,
                    indented: false,
                    foldered: false,
                },
            ]
        );
    }

    #[test]
    fn workspace_list_entries_do_not_group_normal_git_workspaces() {
        let mut app = AppState::test_new();
        app.workspaces = vec![
            workspace_with_git_space("one", "repo-key"),
            workspace_with_git_space("two", "repo-key"),
        ];

        assert_eq!(
            workspace_list_entries(&app),
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
            ]
        );
    }

    #[test]
    fn workspace_list_entries_do_not_auto_attach_normal_git_workspace_to_group() {
        let mut app = AppState::test_new();
        app.workspaces = vec![
            workspace_with_worktree_space("main", Some("repo-key"), "/repo/herdr"),
            workspace_with_git_space("scratch", "repo-key"),
            workspace_with_worktree_space("issue", Some("repo-key"), "/repo/herdr-issue"),
        ];

        assert_eq!(
            workspace_list_entries(&app),
            vec![
                WorkspaceListEntry::Workspace {
                    ws_idx: 0,
                    indented: false,
                    foldered: false,
                },
                WorkspaceListEntry::Workspace {
                    ws_idx: 2,
                    indented: true,
                    foldered: false,
                },
                WorkspaceListEntry::Workspace {
                    ws_idx: 1,
                    indented: false,
                    foldered: false,
                },
            ]
        );
    }

    #[test]
    fn workspace_list_entries_leave_single_git_and_non_git_workspaces_flat() {
        let mut app = AppState::test_new();
        app.workspaces = vec![
            workspace_with_git_space("one", "repo-key"),
            workspace_with_worktree_space("notes", None, "/notes"),
        ];

        assert_eq!(
            workspace_list_entries(&app),
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
            ]
        );
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
    fn collapsed_group_hides_inactive_children_but_keeps_active_visible() {
        let mut app = AppState::test_new();
        app.workspaces = vec![
            workspace_with_worktree_space("main", Some("repo-key"), "/repo/herdr"),
            workspace_with_worktree_space("issue", Some("repo-key"), "/repo/herdr-issue"),
        ];
        app.active = Some(1);
        app.mode = Mode::Terminal;
        app.collapsed_space_keys.insert("repo-key".into());

        assert_eq!(
            workspace_list_entries(&app),
            vec![
                WorkspaceListEntry::Workspace {
                    ws_idx: 0,
                    indented: false,
                    foldered: false,
                },
                WorkspaceListEntry::Workspace {
                    ws_idx: 1,
                    indented: true,
                    foldered: false,
                },
            ]
        );

        app.active = None;
        app.mode = Mode::Terminal;
        assert_eq!(
            workspace_list_entries(&app),
            vec![WorkspaceListEntry::Workspace {
                ws_idx: 0,
                indented: false,
                foldered: false,
            }]
        );
    }

    #[test]
    fn collapsed_group_keeps_selected_child_visible_in_navigate_mode() {
        let mut app = AppState::test_new();
        app.workspaces = vec![
            workspace_with_worktree_space("main", Some("repo-key"), "/repo/herdr"),
            workspace_with_worktree_space("issue", Some("repo-key"), "/repo/herdr-issue"),
        ];
        app.mode = Mode::Navigate;
        app.selected = 1;
        app.active = Some(1);
        app.collapsed_space_keys.insert("repo-key".into());

        assert_eq!(
            workspace_list_entries(&app),
            vec![
                WorkspaceListEntry::Workspace {
                    ws_idx: 0,
                    indented: false,
                    foldered: false,
                },
                WorkspaceListEntry::Workspace {
                    ws_idx: 1,
                    indented: true,
                    foldered: false,
                },
            ]
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
