use super::render::put_text;
use super::*;

pub(super) fn render_collapsed(
    buffer: &mut Buffer,
    area: Rect,
    endpoints: &[ClientShellEndpoint],
    active_endpoint_id: &ClientEndpointId,
    config: &ClientShellConfig,
    hits: &mut ShellHitMap,
) {
    let rows = agent_rows(endpoints, active_endpoint_id, config);
    for (index, row) in rows.into_iter().take(area.height as usize).enumerate() {
        let rect = Rect::new(area.x, area.y + index as u16, area.width, 1);
        if row.agent.focused {
            buffer.set_style(rect, Style::default().bg(config.palette.active_row_bg));
        }
        let initial = row.machine_label.chars().next().unwrap_or('?');
        put_text(
            buffer,
            rect.x,
            rect.y,
            rect.width,
            &format!(
                "{initial}{}",
                status_icon(row.agent.status, config.status_indicators)
            ),
            Style::default()
                .fg(if row.stale {
                    config.palette.overlay0
                } else {
                    status_color(row.agent.status, &config.palette)
                })
                .add_modifier(if row.stale {
                    Modifier::DIM
                } else {
                    Modifier::empty()
                }),
        );
        hits.endpoint_agents
            .push((rect, row.endpoint_id, row.agent.pane_id));
    }
}

pub(super) fn render_expanded(
    buffer: &mut Buffer,
    area: Rect,
    agent_view_label: Option<&str>,
    endpoints: &[ClientShellEndpoint],
    active_endpoint_id: &ClientEndpointId,
    config: &ClientShellConfig,
    collapse_by_endpoint: &HashMap<String, super::folders::FolderCollapseState>,
    agent_scroll: &mut usize,
    hits: &mut ShellHitMap,
) {
    if !super::agent_sidebar::render_agent_panel_header(
        buffer,
        area,
        agent_view_label,
        config,
        hits,
    ) {
        return;
    }
    if super::folders::agent_folder_view_enabled(agent_view_label, config) {
        let rows = folder_view_rows(endpoints, active_endpoint_id, config, collapse_by_endpoint);
        super::folders::render_agent_folder_rows(
            buffer,
            area,
            &rows,
            config,
            agent_scroll,
            hits,
            super::folders::AgentHitTarget::Endpoint,
        );
        return;
    }
    let rows = agent_rows(endpoints, active_endpoint_id, config);
    super::agent_sidebar::render_agent_list(
        buffer,
        area,
        &rows,
        agent_view_label.map(|_| " no matching agents"),
        config,
        agent_scroll,
        hits,
        |row| row.agent.rows.len(),
        |buffer, rect, row, hits| {
            super::agent_sidebar::render_agent_row(buffer, rect, &row.agent, config);
            if row.stale {
                buffer.set_style(
                    rect,
                    Style::default()
                        .fg(config.palette.overlay0)
                        .add_modifier(Modifier::DIM),
                );
            }
            hits.endpoint_agents
                .push((rect, row.endpoint_id.clone(), row.agent.pane_id.clone()));
        },
    );
}

struct EndpointAgentRow {
    endpoint_id: ClientEndpointId,
    machine_label: String,
    stale: bool,
    agent: super::agent_sidebar::AgentRow,
}

/// Folder-view rows for every endpoint with a snapshot, each section headed
/// by its machine. Collapse state is looked up per endpoint because folder and
/// workspace ids are server-scoped.
fn folder_view_rows<'a>(
    endpoints: &'a [ClientShellEndpoint],
    active_endpoint_id: &ClientEndpointId,
    config: &ClientShellConfig,
    collapse_by_endpoint: &HashMap<String, super::folders::FolderCollapseState>,
) -> Vec<super::folders::AgentFolderViewRow<'a>> {
    let mut rows = Vec::new();
    for endpoint in endpoints {
        let Some(snapshot) = endpoint.snapshot.as_deref() else {
            continue;
        };
        let active = &endpoint.endpoint_id == active_endpoint_id;
        let stale = endpoint.status != ClientEndpointStatus::Online;
        let collapse =
            super::folders::FolderCollapseState::of(collapse_by_endpoint, &endpoint.endpoint_id);
        let folders = super::folders::FolderRenderState {
            endpoint_id: &endpoint.endpoint_id,
            collapsed_folders: &collapse.folders,
            collapsed_agent_spaces: &collapse.agent_spaces,
            collapse_by_endpoint,
            dragged_folder_id: None,
            drop_into_folder_id: None,
            drop_indicator_indent: 0,
        };
        let section = super::folders::agent_folder_view_rows(snapshot, config, &folders);
        if section.is_empty() {
            continue;
        }
        rows.push(super::folders::AgentFolderViewRow {
            endpoint_id: &endpoint.endpoint_id,
            stale,
            row: super::folders::AgentPanelRow::MachineHeader {
                label: endpoint.label.clone(),
                status: endpoint.status,
            },
        });
        rows.extend(section.into_iter().map(|mut row| {
            if !active {
                row.clear_active_highlight();
            }
            super::folders::AgentFolderViewRow {
                endpoint_id: &endpoint.endpoint_id,
                stale,
                row,
            }
        }));
    }
    rows
}

fn agent_rows(
    endpoints: &[ClientShellEndpoint],
    active_endpoint_id: &ClientEndpointId,
    config: &ClientShellConfig,
) -> Vec<EndpointAgentRow> {
    let mut rendered_rows = endpoints
        .iter()
        .filter_map(|endpoint| {
            endpoint.snapshot.as_deref().map(|snapshot| {
                super::agent_sidebar::agent_rows(snapshot, config, Some(&endpoint.label))
                    .into_iter()
                    .map(|agent| ((endpoint.endpoint_id.clone(), agent.pane_id.clone()), agent))
                    .collect::<Vec<_>>()
            })
        })
        .flatten()
        .collect::<HashMap<_, _>>();

    super::aggregate_navigation::aggregate_agent_rows(endpoints, config.agent_panel_sort)
        .into_iter()
        .filter_map(|row| {
            let key = (row.endpoint.endpoint_id.clone(), row.agent.pane_id.clone());
            let mut agent = rendered_rows.remove(&key)?;
            agent.focused &= row.endpoint.endpoint_id == active_endpoint_id;
            Some(EndpointAgentRow {
                endpoint_id: row.endpoint.endpoint_id.clone(),
                machine_label: row.endpoint.label.to_owned(),
                stale: row.endpoint.stale(),
                agent,
            })
        })
        .collect()
}
