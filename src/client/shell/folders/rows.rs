//! Folder header rows and the folder-aware drop indicator.

use super::*;

/// A rendered folder header, for clicks, menus and drags.
#[derive(Clone, Debug)]
pub(in crate::client::shell) struct FolderHeaderHit {
    pub(in crate::client::shell) rect: Rect,
    pub(in crate::client::shell) endpoint_id: ClientEndpointId,
    pub(in crate::client::shell) folder_id: String,
}

/// Visual state of one header row.
#[derive(Clone, Copy, Debug, Default)]
pub(in crate::client::shell) struct HeaderLook {
    /// The machine is active, so its focused workspace counts.
    pub(in crate::client::shell) focus_visible: bool,
    pub(in crate::client::shell) drop_target: bool,
    pub(in crate::client::shell) dragged: bool,
    pub(in crate::client::shell) dim: bool,
}

/// Most urgent known agent status among the given workspaces.
pub(in crate::client::shell) fn rollup_status(
    statuses: impl Iterator<Item = crate::api::schema::AgentStatus>,
) -> Option<crate::api::schema::AgentStatus> {
    statuses
        .max_by_key(|status| status_priority(*status))
        .filter(|status| *status != crate::api::schema::AgentStatus::Unknown)
}

/// Draws a folder header across `rect`.
#[allow(clippy::too_many_arguments)] // Mirrors the sidebar row renderers' explicit inputs.
pub(in crate::client::shell) fn render_folder_header(
    buffer: &mut Buffer,
    rect: Rect,
    endpoint_id: &ClientEndpointId,
    snapshot: &ClientShellSnapshot,
    view: &FolderView<'_>,
    folder: usize,
    look: HeaderLook,
    config: &ClientShellConfig,
) -> Option<FolderHeaderHit> {
    let layout_folder = view.layout.folders.get(folder)?;
    let palette = &config.palette;
    let collapsed = view.is_collapsed(folder);
    let members = || {
        snapshot
            .workspaces
            .iter()
            .filter(|workspace| view.folder_index_of(&workspace.workspace_id) == Some(folder))
    };
    let hidden_focus =
        collapsed && look.focus_visible && members().any(|workspace| workspace.focused);
    let background = if hidden_focus {
        Some(palette.active_row_bg)
    } else if look.dragged || look.drop_target {
        Some(palette.surface1)
    } else {
        None
    };
    if let Some(background) = background {
        buffer.set_style(rect, Style::default().bg(background));
    }
    let name_color = if look.drop_target {
        palette.accent
    } else if hidden_focus || look.dragged {
        palette.text
    } else {
        palette.subtext0
    };
    let arrow_color = if hidden_focus {
        palette.text
    } else {
        palette.overlay0
    };
    let x = rect.x;
    let right = rect.right();
    put_text(
        buffer,
        x.saturating_add(1),
        rect.y,
        right.saturating_sub(x.saturating_add(1)),
        if collapsed { "▸" } else { "▾" },
        Style::default().fg(arrow_color),
    );
    let name_x = x.saturating_add(3);
    let name_width = right.saturating_sub(name_x).saturating_sub(2);
    let name = crate::ui::truncate_end(&layout_folder.name, usize::from(name_width));
    put_text(
        buffer,
        name_x,
        rect.y,
        name_width,
        &name,
        Style::default().fg(name_color).add_modifier(Modifier::BOLD),
    );
    if collapsed {
        if let Some(status) = rollup_status(members().map(|workspace| workspace.agent_status)) {
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
    if look.dim {
        buffer.set_style(
            rect,
            Style::default()
                .fg(palette.overlay0)
                .add_modifier(Modifier::DIM),
        );
    }
    Some(FolderHeaderHit {
        rect,
        endpoint_id: endpoint_id.clone(),
        folder_id: layout_folder.id.clone(),
    })
}

/// Draws the drop line for a folder-aware drag.
pub(in crate::client::shell) fn render_folder_drop_indicator(
    buffer: &mut Buffer,
    workspace_area: Rect,
    indicator: Option<(u16, u16)>,
    right: u16,
    palette: &Palette,
) {
    let Some((row, x)) = indicator else {
        return;
    };
    if row < workspace_area.y.saturating_add(1)
        || row >= workspace_area.bottom().saturating_sub(1)
        || x >= right
    {
        return;
    }
    let width = right - x;
    put_text(
        buffer,
        x,
        row,
        width,
        &"─".repeat(usize::from(width)),
        Style::default().fg(palette.accent),
    );
}
