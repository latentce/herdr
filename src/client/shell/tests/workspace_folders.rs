//! Spaces panel, menus, drag and drop, keyboard and agents
//! panel behavior of workspace folders.

use super::super::folders::{
    drag, folder_menu_items, menus, prefs, AgentPanelMode, FolderAgentHeader, FolderMenuOverlay,
};
use super::*;
use crate::folders::protocol::{EndpointFolderLayout, FolderLayout, FolderLayoutFolder};

const COLS: u16 = 106;
const ROWS: u16 = 30;

fn workspace(id: &str, label: &str) -> ClientShellWorkspace {
    let mut workspace = snapshot().workspaces[0].clone();
    workspace.workspace_id = id.into();
    workspace.label = label.into();
    workspace.focused = id == "ws_1";
    workspace
}

/// A snapshot with `ws_1` focused plus the given extra workspaces.
fn folder_snapshot(extra: &[(&str, &str)]) -> ClientShellSnapshot {
    let mut projected = snapshot();
    projected.workspaces[0].label = "api".into();
    projected
        .workspaces
        .extend(extra.iter().map(|(id, label)| workspace(id, label)));
    projected
}

fn layout(folders: &[(&str, &str, &[&str])], order: &[&str]) -> FolderLayout {
    FolderLayout {
        folders: folders
            .iter()
            .map(|(id, name, members)| FolderLayoutFolder {
                id: (*id).into(),
                name: (*name).into(),
                workspace_ids: members.iter().map(|member| (*member).into()).collect(),
            })
            .collect(),
        order: order.iter().map(|id| (*id).into()).collect(),
    }
}

fn all_methods() -> Vec<String> {
    crate::server::client_commands::supported_client_shell_method_names()
        .iter()
        .map(|method| (*method).to_owned())
        .collect()
}

fn give_layout(state: &mut ClientShellState, endpoint_id: &ClientEndpointId, layout: FolderLayout) {
    let endpoint = state
        .endpoints
        .iter()
        .find(|endpoint| &endpoint.endpoint_id == endpoint_id)
        .expect("endpoint");
    let snapshot = endpoint.snapshot.as_deref().expect("snapshot");
    let received = EndpointFolderLayout {
        boot_id: snapshot.boot_id.clone(),
        revision: snapshot.revision,
        layout,
    };
    let generation = endpoint.snapshot_generation;
    state
        .folders
        .receive_layout(endpoint_id, generation, received);
}

fn folder_state(projected: ClientShellSnapshot, folders: FolderLayout) -> ClientShellState {
    folder_state_with(Config::default(), projected, folders)
}

fn folder_state_with(
    config: Config,
    projected: ClientShellSnapshot,
    folders: FolderLayout,
) -> ClientShellState {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&config));
    state.set_endpoint_methods(Some(all_methods()));
    state.set_snapshot(Box::new(projected));
    state.set_pane_surface(surface());
    give_layout(&mut state, &ClientEndpointId::Local, folders);
    state
}

/// backend = [ws_2, ws_3] between api (ws_1) and docs (ws_4).
fn backend_state() -> ClientShellState {
    folder_state(
        folder_snapshot(&[("ws_2", "auth"), ("ws_3", "billing"), ("ws_4", "docs")]),
        layout(
            &[("f1", "backend", &["ws_2", "ws_3"])],
            &["ws_1", "f1", "ws_4"],
        ),
    )
}

fn styled(state: &mut ClientShellState) -> Buffer {
    state
        .compose(COLS, ROWS)
        .expect("frame")
        .to_ratatui_buffer()
        .expect("buffer")
}

fn compose(state: &mut ClientShellState) -> Vec<String> {
    frame_rows(&state.compose(COLS, ROWS).expect("frame"))
}

fn mouse(
    state: &mut ClientShellState,
    kind: MouseEventKind,
    column: u16,
    row: u16,
) -> ClientShellInput {
    state.handle_raw_events(vec![RawInputEvent::Mouse(MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::empty(),
    })])
}

fn click(state: &mut ClientShellState, column: u16, row: u16) -> ClientShellInput {
    let mut outcome = mouse(state, MouseEventKind::Down(MouseButton::Left), column, row);
    let up = mouse(state, MouseEventKind::Up(MouseButton::Left), column, row);
    outcome.actions.extend(up.actions);
    outcome
}

fn header(state: &ClientShellState, folder_id: &str) -> Rect {
    state
        .hits
        .folder_headers
        .iter()
        .find(|hit| hit.folder_id == folder_id)
        .map(|hit| hit.rect)
        .unwrap_or_else(|| panic!("header {folder_id}"))
}

fn row_of(state: &ClientShellState, workspace_id: &str) -> Rect {
    state
        .hits
        .workspaces
        .iter()
        .find(|hit| hit.workspace_id == workspace_id)
        .map(|hit| hit.rect)
        .unwrap_or_else(|| panic!("workspace {workspace_id}"))
}

fn methods(outcome: &ClientShellInput) -> Vec<crate::api::schema::Method> {
    outcome
        .actions
        .iter()
        .filter_map(|action| match action {
            ClientShellAction::Endpoint { request, .. } => Some(request.method.clone()),
            _ => None,
        })
        .collect()
}

fn sidebar_text(rows: &[String], y: u16) -> String {
    rows[y as usize].chars().take(26).collect()
}

fn drop_target(state: &ClientShellState, column: u16, row: u16) -> drag::DropTarget {
    state
        .folder_drop_target_at(&drag::DragSource::Workspace("ws_4".into()), (column, row))
        .unwrap_or_else(|| panic!("drop target at {column},{row}"))
}

fn top(before: Option<&str>, after: Option<&str>) -> drag::DropPlacement {
    drag::DropPlacement::Top {
        before: before.map(str::to_owned),
        after: after.map(str::to_owned),
    }
}

fn into(folder: &str, before: Option<&str>) -> drag::DropPlacement {
    drag::DropPlacement::Into {
        folder_id: folder.into(),
        before: before.map(str::to_owned),
    }
}

#[test]
fn folder_headers_render_between_workspaces_with_members_indented() {
    let mut state = backend_state();
    let rows = compose(&mut state);
    let backend = header(&state, "f1");
    assert!(sidebar_text(&rows, backend.y).starts_with(" ▾ backend"));
    assert!(
        sidebar_text(&rows, row_of(&state, "ws_1").y).starts_with(" ● api")
            || sidebar_text(&rows, row_of(&state, "ws_1").y).starts_with(" ○ api")
    );
    let auth = row_of(&state, "ws_2");
    assert_eq!(auth.y, backend.y + 1);
    assert!(sidebar_text(&rows, auth.y).starts_with("   "));
    assert!(sidebar_text(&rows, auth.y)[3..].contains("auth"));
    assert!(row_of(&state, "ws_4").y > row_of(&state, "ws_3").y);
    let buffer = styled(&mut state);
    let cell = &buffer[(3, backend.y)];
    assert!(cell.modifier.contains(Modifier::BOLD));
    assert_eq!(cell.fg, state.config.palette.subtext0);
    assert!(!sidebar_text(&rows, backend.y).contains('2'));
}

#[test]
fn collapsed_folder_hides_members_shows_rollup_and_hidden_focus() {
    let mut projected = folder_snapshot(&[("ws_2", "auth"), ("ws_3", "billing")]);
    projected.workspaces[1].agent_status = crate::api::schema::AgentStatus::Blocked;
    projected.workspaces[2].agent_status = crate::api::schema::AgentStatus::Working;
    let mut state = folder_state(
        projected,
        layout(&[("f1", "backend", &["ws_2", "ws_3"])], &["ws_1", "f1"]),
    );
    compose(&mut state);
    let backend = header(&state, "f1");
    let rows = compose(&mut state);
    assert_eq!(
        rows[backend.y as usize]
            .chars()
            .nth(backend.right() as usize - 1),
        Some(' ')
    );

    state.folders.toggle_folder(&ClientEndpointId::Local, "f1");
    let rows = compose(&mut state);
    let backend = header(&state, "f1");
    assert!(sidebar_text(&rows, backend.y).starts_with(" ▸ backend"));
    assert!(state
        .hits
        .workspaces
        .iter()
        .all(|hit| hit.workspace_id == "ws_1"));
    let buffer = styled(&mut state);
    let rollup = &buffer[(backend.right() - 1, backend.y)];
    assert_eq!(rollup.symbol(), "●");
    assert_eq!(rollup.fg, state.config.palette.red);
    assert_ne!(
        buffer[(3, backend.y)].bg,
        state.config.palette.active_row_bg
    );

    let mut projected = state.snapshot.as_deref().expect("snapshot").clone();
    projected.revision += 1;
    for workspace in &mut projected.workspaces {
        workspace.focused = workspace.workspace_id == "ws_3";
    }
    projected.focused_workspace_id = Some("ws_3".into());
    let revision = projected.revision;
    state.set_snapshot(Box::new(projected));
    let mut matching = surface();
    matching.projection_revision = revision;
    state.set_pane_surface(matching);
    let buffer = styled(&mut state);
    let backend = header(&state, "f1");
    let name = &buffer[(3, backend.y)];
    assert_eq!(name.bg, state.config.palette.active_row_bg);
    assert_eq!(name.fg, state.config.palette.text);
    assert!(state
        .hits
        .workspaces
        .iter()
        .all(|hit| hit.workspace_id != "ws_3"));
    assert!(state
        .folders
        .is_folder_collapsed(&ClientEndpointId::Local, "f1"));

    state.folders.toggle_folder(&ClientEndpointId::Local, "f1");
    let buffer = styled(&mut state);
    let backend = header(&state, "f1");
    assert_ne!(
        buffer[(3, backend.y)].bg,
        state.config.palette.active_row_bg
    );
    assert!(state
        .hits
        .workspaces
        .iter()
        .any(|hit| hit.workspace_id == "ws_3"));
}

#[test]
fn empty_and_unknown_status_folders_show_nothing_at_the_edge() {
    let mut state = folder_state(
        folder_snapshot(&[("ws_2", "auth")]),
        layout(
            &[("f1", "empty", &[]), ("f2", "quiet", &["ws_2"])],
            &["ws_1", "f1", "f2"],
        ),
    );
    state.folders.toggle_folder(&ClientEndpointId::Local, "f2");
    let mut projected = state.snapshot.as_deref().expect("snapshot").clone();
    projected.workspaces[1].agent_status = crate::api::schema::AgentStatus::Unknown;
    state.set_snapshot(Box::new(projected));
    let rows = compose(&mut state);
    let empty = header(&state, "f1");
    let quiet = header(&state, "f2");
    assert_eq!(
        quiet.y,
        empty.y + 1,
        "an expanded empty folder has no placeholder row"
    );
    let quiet_row = rows[quiet.y as usize].chars().take(25).collect::<String>();
    assert_eq!(quiet_row.trim_end(), " ▸ quiet");
}

#[test]
fn long_folder_names_are_cut_off() {
    let mut state = folder_state(
        folder_snapshot(&[]),
        layout(
            &[("f1", "a very long folder name that does not fit", &[])],
            &["ws_1", "f1"],
        ),
    );
    let rows = compose(&mut state);
    let text = sidebar_text(&rows, header(&state, "f1").y);
    assert!(text.contains('…'), "{text}");
}

#[test]
fn machines_without_folder_support_show_no_folder_ui() {
    let mut state = backend_state();
    state.set_endpoint_methods(Some(
        all_methods()
            .into_iter()
            .filter(|method| !method.starts_with("folder."))
            .collect(),
    ));
    compose(&mut state);
    assert!(state.hits.folder_headers.is_empty());
    assert_eq!(state.hits.workspaces.len(), 4);
    let new = state.hits.new_workspace;
    let outcome = click(&mut state, new.x + 1, new.y);
    assert!(matches!(
        &methods(&outcome)[..],
        [crate::api::schema::Method::WorkspaceCreate(_)]
    ));
    assert!(state.overlay.is_none());
    let api = row_of(&state, "ws_1");
    mouse(
        &mut state,
        MouseEventKind::Down(MouseButton::Right),
        api.x + 2,
        api.y,
    );
    let Some(ClientShellOverlay::ContextMenu(menu)) = state.overlay.as_ref() else {
        panic!("workspace menu");
    };
    assert!(menu
        .items()
        .iter()
        .all(|item| item.action != ClientContextMenuAction::MoveToFolder));
}

#[test]
fn footer_new_menu_creates_workspaces_and_folders() {
    let mut state = backend_state();
    compose(&mut state);
    let new = state.hits.new_workspace;
    click(&mut state, new.x + 1, new.y);
    assert!(matches!(
        state.overlay,
        Some(ClientShellOverlay::FolderMenu(FolderMenuOverlay {
            kind: menus::FolderMenuKind::New,
            ..
        }))
    ));
    compose(&mut state);
    let (workspace_row, _) = state.hits.folder_menu_rows[0];
    let outcome = click(&mut state, workspace_row.x + 1, workspace_row.y);
    assert!(matches!(
        &methods(&outcome)[..],
        [crate::api::schema::Method::WorkspaceCreate(_)]
    ));

    compose(&mut state);
    click(&mut state, new.x + 1, new.y);
    compose(&mut state);
    let (folder_row, _) = state.hits.folder_menu_rows[1];
    click(&mut state, folder_row.x + 1, folder_row.y);
    assert!(matches!(
        &state.overlay,
        Some(ClientShellOverlay::Rename(rename)) if rename.title == "new folder"
    ));
    state.handle_input_bytes(b"   ");
    let outcome = state.handle_input_bytes(b"\r");
    assert!(methods(&outcome).is_empty());
    assert!(matches!(state.overlay, Some(ClientShellOverlay::Rename(_))));
    state.handle_input_bytes(b" tools ");
    let outcome = state.handle_input_bytes(b"\r");
    assert!(matches!(
        &methods(&outcome)[..],
        [crate::api::schema::Method::FolderCreate(params)]
            if params.name == "tools" && params.workspace_ids.is_empty()
    ));
    assert!(state.overlay.is_none());
}

#[test]
fn new_workspace_keybinding_still_creates_directly() {
    let mut state = backend_state();
    compose(&mut state);
    let mut outcome = ClientShellInput::default();
    state.record_binding(
        crate::input::KeybindMatch::Action(crate::input::KeybindAction::NewWorkspace),
        &mut outcome,
    );
    assert!(matches!(
        &methods(&outcome)[..],
        [crate::api::schema::Method::WorkspaceCreate(_)]
    ));
}

#[test]
fn header_clicks_toggle_collapse_and_menus_manage_the_folder() {
    let mut state = backend_state();
    compose(&mut state);
    let backend = header(&state, "f1");
    click(&mut state, backend.x + 10, backend.y);
    assert!(state
        .folders
        .is_folder_collapsed(&ClientEndpointId::Local, "f1"));
    compose(&mut state);
    click(&mut state, backend.x + 1, backend.y);
    assert!(!state
        .folders
        .is_folder_collapsed(&ClientEndpointId::Local, "f1"));

    compose(&mut state);
    mouse(
        &mut state,
        MouseEventKind::Down(MouseButton::Right),
        backend.x + 4,
        backend.y,
    );
    compose(&mut state);
    let labels = folder_menu_items(
        &state.folders,
        &state.endpoints,
        &state.active_endpoint_id,
        match state.overlay.as_ref() {
            Some(ClientShellOverlay::FolderMenu(menu)) => menu,
            _ => panic!("folder menu"),
        },
    )
    .into_iter()
    .map(|item| item.label)
    .collect::<Vec<_>>();
    assert_eq!(
        labels,
        ["Rename", "Collapse", "New workspace here", "Delete folder"]
    );

    state.handle_input_bytes(b"\r");
    assert!(matches!(
        &state.overlay,
        Some(ClientShellOverlay::Rename(rename))
            if rename.title == "rename folder" && rename.input.trim() == "backend"
    ));
    let mut outcome = ClientShellInput::default();
    if let Some(ClientShellOverlay::Rename(rename)) = state.overlay.as_mut() {
        rename.input.clear();
    }
    state.save_rename_overlay(&mut outcome);
    assert!(methods(&outcome).is_empty());
    assert!(state.overlay.is_none());

    compose(&mut state);
    mouse(
        &mut state,
        MouseEventKind::Down(MouseButton::Right),
        backend.x + 4,
        backend.y,
    );
    state.handle_input_bytes(b"\x1b[B\x1b[B");
    let outcome = state.handle_input_bytes(b"\r");
    assert!(matches!(
        &methods(&outcome)[..],
        [crate::api::schema::Method::FolderCreateWorkspace(params)]
            if params.folder_id == "f1" && params.workspace.focus
    ));

    compose(&mut state);
    mouse(
        &mut state,
        MouseEventKind::Down(MouseButton::Right),
        backend.x + 4,
        backend.y,
    );
    state.handle_input_bytes(b"\x1b[B\x1b[B\x1b[B");
    let outcome = state.handle_input_bytes(b"\r");
    assert!(matches!(
        &methods(&outcome)[..],
        [crate::api::schema::Method::FolderDelete(target)] if target.folder_id == "f1"
    ));
}

#[test]
fn move_to_folder_submenu_lists_folders_and_acts() {
    let mut state = folder_state(
        folder_snapshot(&[("ws_2", "auth"), ("ws_3", "billing")]),
        layout(
            &[("f1", "backend", &["ws_2"]), ("f2", "frontend", &[])],
            &["ws_1", "f1", "f2", "ws_3"],
        ),
    );
    compose(&mut state);
    let auth = row_of(&state, "ws_2");
    mouse(
        &mut state,
        MouseEventKind::Down(MouseButton::Right),
        auth.x + 4,
        auth.y,
    );
    let Some(ClientShellOverlay::ContextMenu(menu)) = state.overlay.as_ref() else {
        panic!("workspace menu");
    };
    let move_index = menu
        .items()
        .iter()
        .position(|item| item.action == ClientContextMenuAction::MoveToFolder)
        .expect("Move to folder item");
    for _ in 0..move_index {
        state.handle_input_bytes(b"\x1b[B");
    }
    state.handle_input_bytes(b"\x1b[C");
    compose(&mut state);
    let items = folder_menu_items(
        &state.folders,
        &state.endpoints,
        &state.active_endpoint_id,
        match state.overlay.as_ref() {
            Some(ClientShellOverlay::FolderMenu(menu)) => menu,
            _ => panic!("submenu"),
        },
    );
    assert_eq!(
        items
            .iter()
            .map(|item| (item.label.as_str(), item.enabled))
            .collect::<Vec<_>>(),
        [
            ("backend", false),
            ("frontend", true),
            ("New folder…", true),
            ("Remove from folder", true)
        ]
    );
    let outcome = state.handle_input_bytes(b"\r");
    assert!(matches!(
        &methods(&outcome)[..],
        [crate::api::schema::Method::FolderAddWorkspace(params)]
            if params.folder_id == "f2" && params.workspace_id == "ws_2"
    ));

    compose(&mut state);
    mouse(
        &mut state,
        MouseEventKind::Down(MouseButton::Right),
        auth.x + 4,
        auth.y,
    );
    for _ in 0..move_index {
        state.handle_input_bytes(b"\x1b[B");
    }
    state.handle_input_bytes(b"\r");
    assert!(matches!(
        state.overlay,
        Some(ClientShellOverlay::FolderMenu(_))
    ));
    state.handle_input_bytes(b"\x1b[D");
    assert!(matches!(
        state.overlay,
        Some(ClientShellOverlay::ContextMenu(_))
    ));

    compose(&mut state);
    state.overlay = None;
    let billing = row_of(&state, "ws_3");
    mouse(
        &mut state,
        MouseEventKind::Down(MouseButton::Right),
        billing.x + 4,
        billing.y,
    );
    let index = match state.overlay.as_ref() {
        Some(ClientShellOverlay::ContextMenu(menu)) => menu
            .items()
            .iter()
            .position(|item| item.action == ClientContextMenuAction::MoveToFolder)
            .expect("item"),
        _ => panic!("menu"),
    };
    assert!(state.open_folder_submenu(index));
    let items = folder_menu_items(
        &state.folders,
        &state.endpoints,
        &state.active_endpoint_id,
        match state.overlay.as_ref() {
            Some(ClientShellOverlay::FolderMenu(menu)) => menu,
            _ => panic!("submenu"),
        },
    );
    assert_eq!(
        items.last().map(|item| item.label.as_str()),
        Some("New folder…")
    );
    state.handle_input_bytes(b"\x1b[B\x1b[B");
    state.handle_input_bytes(b"\r");
    state.handle_input_bytes(b"ops");
    let outcome = state.handle_input_bytes(b"\r");
    assert!(matches!(
        &methods(&outcome)[..],
        [crate::api::schema::Method::FolderCreate(params)]
            if params.name == "ops" && params.workspace_ids == ["ws_3"]
    ));
}

#[test]
fn submenu_opens_on_hover_and_flips_left_without_room() {
    let mut state = backend_state();
    compose(&mut state);
    let api = row_of(&state, "ws_1");
    mouse(
        &mut state,
        MouseEventKind::Down(MouseButton::Right),
        api.x + 2,
        api.y,
    );
    if let Some(ClientShellOverlay::ContextMenu(menu)) = state.overlay.as_mut() {
        menu.x = COLS - 3;
    }
    let move_row = {
        compose(&mut state);
        let index = match state.overlay.as_ref() {
            Some(ClientShellOverlay::ContextMenu(menu)) => menu
                .items()
                .iter()
                .position(|item| item.action == ClientContextMenuAction::MoveToFolder)
                .expect("item"),
            _ => panic!("menu"),
        };
        state
            .hits
            .context_menu_rows
            .iter()
            .find(|(_, row)| *row == index)
            .map(|(rect, _)| *rect)
            .expect("row")
    };
    mouse(
        &mut state,
        MouseEventKind::Moved,
        move_row.x + 1,
        move_row.y,
    );
    assert!(matches!(
        state.overlay,
        Some(ClientShellOverlay::FolderMenu(_))
    ));
    compose(&mut state);
    let parent_x = state
        .hits
        .context_menu_rows
        .first()
        .map(|(rect, _)| rect.x)
        .expect("parent rows");
    assert!(
        state.hits.folder_menu_area.right() <= parent_x,
        "flipped left"
    );
}

#[test]
fn worktree_children_get_no_move_item_but_lone_linked_worktrees_do() {
    let worktree = |linked| crate::protocol::ClientShellWorktree {
        key: "repo".into(),
        label: "repo".into(),
        is_linked_worktree: linked,
    };
    let mut projected = folder_snapshot(&[("ws_2", "child"), ("ws_3", "lone")]);
    projected.workspaces[0].worktree = Some(worktree(false));
    projected.workspaces[1].worktree = Some(worktree(true));
    projected.workspaces[2].worktree = Some(crate::protocol::ClientShellWorktree {
        key: "other".into(),
        label: "other".into(),
        is_linked_worktree: true,
    });
    let mut state = folder_state(projected, layout(&[], &[]));
    let snapshot = state.snapshot.as_deref().expect("snapshot").clone();
    assert!(state.workspace_can_move_to_folder(&snapshot, &snapshot.workspaces[0]));
    assert!(!state.workspace_can_move_to_folder(&snapshot, &snapshot.workspaces[1]));
    assert!(state.workspace_can_move_to_folder(&snapshot, &snapshot.workspaces[2]));
    compose(&mut state);
    let lone = row_of(&state, "ws_3");
    mouse(
        &mut state,
        MouseEventKind::Down(MouseButton::Left),
        lone.x + 2,
        lone.y,
    );
    mouse(
        &mut state,
        MouseEventKind::Drag(MouseButton::Left),
        lone.x + 2,
        lone.y + 3,
    );
    assert!(state.chrome_drag.is_none());
}

#[test]
fn drop_targets_follow_the_outliner_column_rule() {
    let mut state = backend_state();
    compose(&mut state);
    let backend = header(&state, "f1");
    let auth = row_of(&state, "ws_2");
    let billing = row_of(&state, "ws_3");
    let docs = row_of(&state, "ws_4");
    let divider = state.hits.workspace_body.x + 3;
    let left = state.hits.workspace_body.x;

    let target = drop_target(&state, divider + 2, backend.y);
    assert_eq!(target.placement, into("f1", None));
    assert_eq!(target.header.as_deref(), Some("f1"));
    let target = drop_target(&state, left, backend.y);
    assert_eq!(target.placement, top(Some("f1"), None));
    assert_eq!(target.indicator, Some((backend.y - 1, left)));
    assert_eq!(
        drop_target(&state, left, auth.y).placement,
        into("f1", Some("ws_2"))
    );
    let target = drop_target(&state, left, billing.y);
    assert_eq!(target.placement, into("f1", Some("ws_3")));
    assert_eq!(target.indicator, Some((billing.y - 1, divider)));
    for row in [billing.y + 1, docs.y] {
        let target = drop_target(&state, divider + 1, row);
        assert_eq!(target.placement, into("f1", None));
        assert_eq!(target.indicator, Some((docs.y - 1, divider)));
        let target = drop_target(&state, left, row);
        assert_eq!(target.placement, top(Some("ws_4"), None));
        assert_eq!(target.indicator, Some((docs.y - 1, left)));
    }
    assert_eq!(
        drop_target(&state, left, docs.bottom() + 1).placement,
        top(None, Some("ws_4"))
    );
}

#[test]
fn bottom_edge_of_a_last_folder_and_collapsed_or_empty_folders() {
    let mut state = folder_state(
        folder_snapshot(&[("ws_2", "auth"), ("ws_4", "docs")]),
        layout(
            &[
                ("f1", "backend", &["ws_2"]),
                ("f2", "empty", &[]),
                ("f3", "last", &["ws_4"]),
            ],
            &["ws_1", "f1", "f2", "f3"],
        ),
    );
    compose(&mut state);
    let divider = state.hits.workspace_body.x + 3;
    let left = state.hits.workspace_body.x;
    let docs = row_of(&state, "ws_4");
    assert_eq!(
        drop_target(&state, divider, docs.bottom() + 1).placement,
        into("f3", None)
    );
    assert_eq!(
        drop_target(&state, left, docs.bottom() + 1).placement,
        top(None, Some("f3"))
    );
    let auth = row_of(&state, "ws_2");
    assert_eq!(
        drop_target(&state, divider, auth.y + 1).placement,
        into("f1", None)
    );
    assert_eq!(
        drop_target(&state, left, auth.y + 1).placement,
        top(Some("f2"), None)
    );
    let empty = header(&state, "f2");
    assert_eq!(
        drop_target(&state, divider, empty.y).placement,
        into("f2", None)
    );
    assert_eq!(
        drop_target(&state, left, empty.y).placement,
        top(Some("f2"), None)
    );

    state.folders.toggle_folder(&ClientEndpointId::Local, "f1");
    compose(&mut state);
    let empty = header(&state, "f2");
    let backend = header(&state, "f1");
    assert_eq!(empty.y, backend.y + 1);
    assert_eq!(
        drop_target(&state, divider, backend.y).placement,
        into("f1", None)
    );
}

#[test]
fn one_line_rows_keep_the_rules() {
    let config: Config =
        toml::from_str("[ui.sidebar.spaces]\nrows = [[\"state_icon\", \"workspace\"]]\n")
            .expect("config");
    let mut state = folder_state_with(
        config,
        folder_snapshot(&[("ws_2", "auth"), ("ws_3", "billing"), ("ws_4", "docs")]),
        layout(
            &[("f1", "backend", &["ws_2", "ws_3"])],
            &["ws_1", "f1", "ws_4"],
        ),
    );
    compose(&mut state);
    let auth = row_of(&state, "ws_2");
    assert_eq!(auth.height, 1);
    let left = state.hits.workspace_body.x;
    let divider = left + 3;
    assert_eq!(
        drop_target(&state, left, auth.y).placement,
        into("f1", Some("ws_3"))
    );
    let billing = row_of(&state, "ws_3");
    assert_eq!(
        drop_target(&state, divider, billing.y).placement,
        into("f1", None)
    );
    assert_eq!(
        drop_target(&state, left, billing.y).placement,
        top(Some("ws_4"), None)
    );
}

#[test]
fn dropping_a_workspace_sends_folder_methods() {
    let mut state = backend_state();
    compose(&mut state);
    let api = row_of(&state, "ws_1");
    let backend = header(&state, "f1");
    let divider = state.hits.workspace_body.x + 3;
    mouse(
        &mut state,
        MouseEventKind::Down(MouseButton::Left),
        api.x + 2,
        api.y,
    );
    mouse(
        &mut state,
        MouseEventKind::Drag(MouseButton::Left),
        divider + 3,
        backend.y,
    );
    let buffer = styled(&mut state);
    assert_eq!(buffer[(3, backend.y)].bg, state.config.palette.surface1);
    let outcome = mouse(
        &mut state,
        MouseEventKind::Up(MouseButton::Left),
        divider + 3,
        backend.y,
    );
    assert!(matches!(
        &methods(&outcome)[..],
        [crate::api::schema::Method::FolderAddWorkspace(params)]
            if params.folder_id == "f1" && params.workspace_id == "ws_1" && params.before.is_none()
    ));

    compose(&mut state);
    let auth = row_of(&state, "ws_2");
    let docs = row_of(&state, "ws_4");
    let left = state.hits.workspace_body.x;
    mouse(
        &mut state,
        MouseEventKind::Down(MouseButton::Left),
        auth.x + 4,
        auth.y,
    );
    mouse(
        &mut state,
        MouseEventKind::Drag(MouseButton::Left),
        left,
        docs.y,
    );
    let rows = compose(&mut state);
    assert!(sidebar_text(&rows, docs.y - 1).starts_with("─────"));
    let outcome = mouse(
        &mut state,
        MouseEventKind::Up(MouseButton::Left),
        left,
        docs.y,
    );
    assert!(matches!(
        &methods(&outcome)[..],
        [crate::api::schema::Method::FolderRemoveWorkspace(params)]
            if params.workspace_id == "ws_2" && params.before.as_deref() == Some("ws_4")
    ));
}

#[test]
fn collapsed_folders_stay_collapsed_when_dropped_on() {
    let mut state = backend_state();
    state.folders.toggle_folder(&ClientEndpointId::Local, "f1");
    compose(&mut state);
    let docs = row_of(&state, "ws_4");
    let backend = header(&state, "f1");
    mouse(
        &mut state,
        MouseEventKind::Down(MouseButton::Left),
        docs.x + 4,
        docs.y,
    );
    mouse(
        &mut state,
        MouseEventKind::Drag(MouseButton::Left),
        backend.x + 8,
        backend.y,
    );
    compose(&mut state);
    assert!(state
        .folders
        .is_folder_collapsed(&ClientEndpointId::Local, "f1"));
    let outcome = mouse(
        &mut state,
        MouseEventKind::Up(MouseButton::Left),
        backend.x + 8,
        backend.y,
    );
    assert!(matches!(
        &methods(&outcome)[..],
        [crate::api::schema::Method::FolderAddWorkspace(params)] if params.folder_id == "f1"
    ));
    assert!(state
        .folders
        .is_folder_collapsed(&ClientEndpointId::Local, "f1"));
}

#[test]
fn dragging_a_folder_header_moves_the_folder_between_top_level_items() {
    let mut state = folder_state(
        folder_snapshot(&[("ws_2", "auth"), ("ws_3", "billing"), ("ws_4", "docs")]),
        layout(
            &[("f1", "backend", &["ws_2"]), ("f2", "other", &["ws_3"])],
            &["ws_1", "f1", "f2", "ws_4"],
        ),
    );
    compose(&mut state);
    let backend = header(&state, "f1");
    let other = header(&state, "f2");
    let billing = row_of(&state, "ws_3");
    let docs = row_of(&state, "ws_4");
    mouse(
        &mut state,
        MouseEventKind::Down(MouseButton::Left),
        backend.x + 5,
        backend.y,
    );
    mouse(
        &mut state,
        MouseEventKind::Drag(MouseButton::Left),
        backend.x + 5,
        docs.y + 1,
    );
    let buffer = styled(&mut state);
    assert_eq!(buffer[(3, backend.y)].bg, state.config.palette.surface1);
    assert_ne!(buffer[(3, other.y)].bg, state.config.palette.surface1);
    assert!(matches!(
        state.chrome_drag.as_ref(),
        Some(ClientChromeDrag::Folders(drag)) if drag.target.as_ref().is_some_and(|target| target.placement == top(None, Some("ws_4")))
    ));
    state.update_folder_drag((other.x + 9, other.y));
    assert!(matches!(
        state.chrome_drag.as_ref(),
        Some(ClientChromeDrag::Folders(drag)) if drag.target.as_ref().is_some_and(|target| target.placement == top(Some("f2"), None))
    ));
    state.update_folder_drag((other.x + 9, billing.y + 1));
    assert!(matches!(
        state.chrome_drag.as_ref(),
        Some(ClientChromeDrag::Folders(drag)) if drag.target.as_ref().is_some_and(|target| target.placement == top(Some("ws_4"), None))
    ));
    let outcome = mouse(
        &mut state,
        MouseEventKind::Up(MouseButton::Left),
        other.x + 9,
        billing.y + 1,
    );
    assert!(matches!(
        &methods(&outcome)[..],
        [crate::api::schema::Method::FolderMove(params)]
            if params.folder_id == "f1" && params.before.as_deref() == Some("ws_4")
    ));
    compose(&mut state);
    click(&mut state, backend.x + 5, backend.y);
    assert!(state
        .folders
        .is_folder_collapsed(&ClientEndpointId::Local, "f1"));
}

#[test]
fn digits_and_next_previous_skip_collapsed_folders() {
    let mut state = backend_state();
    state.folders.toggle_folder(&ClientEndpointId::Local, "f1");
    compose(&mut state);
    let snapshot = state.snapshot.as_deref().expect("snapshot").clone();
    let ids = state
        .navigation_workspace_entries(&snapshot)
        .iter()
        .map(|entry| snapshot.workspaces[entry.index].workspace_id.clone())
        .collect::<Vec<_>>();
    assert_eq!(ids, ["ws_1", "ws_4"]);

    let mut projected = snapshot.clone();
    projected.revision += 1;
    projected.focused_workspace_id = Some("ws_3".into());
    for workspace in &mut projected.workspaces {
        workspace.focused = workspace.workspace_id == "ws_3";
    }
    state.set_snapshot(Box::new(projected.clone()));
    let mut matching = surface();
    matching.projection_revision = projected.revision;
    state.set_pane_surface(matching);
    let entries = state.navigation_workspace_entries(&projected);
    assert_eq!(
        state.folder_hidden_focus_step(&projected, &entries, "ws_3", 1),
        Some(1)
    );
    assert_eq!(
        state.folder_hidden_focus_step(&projected, &entries, "ws_3", -1),
        Some(0)
    );
    assert_eq!(
        state.folder_hidden_focus_step(&projected, &entries, "ws_1", 1),
        None
    );

    state.sidebar_collapsed = true;
    let compact = state
        .navigation_workspace_entries(&projected)
        .iter()
        .map(|entry| projected.workspaces[entry.index].workspace_id.clone())
        .collect::<Vec<_>>();
    assert_eq!(compact, ["ws_1", "ws_2", "ws_3", "ws_4"]);
    let entries = state.navigation_workspace_entries(&projected);
    assert_eq!(
        state.folder_hidden_focus_step(&projected, &entries, "ws_3", 1),
        None
    );
}

#[test]
fn only_accepted_snapshots_prune_agent_workspace_collapse() {
    let mut state = backend_state();
    state
        .folders
        .collapsed_agent_workspaces
        .entry(ClientEndpointId::Local)
        .or_default()
        .insert("ws_4".into());
    let current = state.snapshot.as_deref().expect("snapshot").clone();
    let without_docs = |revision| {
        let mut projected = current.clone();
        projected.revision = revision;
        projected
            .workspaces
            .retain(|workspace| workspace.workspace_id != "ws_4");
        Box::new(projected)
    };
    let collapsed = |state: &ClientShellState| {
        state
            .folders
            .collapsed_agent_workspaces
            .get(&ClientEndpointId::Local)
            .is_some_and(|collapsed| collapsed.contains("ws_4"))
    };

    state.set_snapshot(without_docs(current.revision - 1));
    assert!(collapsed(&state));
    state.set_snapshot(without_docs(current.revision + 1));
    assert!(!collapsed(&state));
}

#[test]
fn agents_folders_mode_shows_the_tree_and_toggles_headers() {
    let mut projected = folder_snapshot(&[("ws_2", "auth"), ("ws_3", "billing"), ("ws_4", "docs")]);
    let agent = |pane: &str, workspace: &str, status| ClientShellAgent {
        pane_id: pane.into(),
        workspace_id: workspace.into(),
        tab_id: "tab_1".into(),
        name: Some("claude".into()),
        display_agent: None,
        agent: Some("claude".into()),
        title: None,
        terminal_title: None,
        terminal_title_stripped: None,
        agent_status: status,
        state_change_seq: 1,
        state_labels: Vec::new(),
        tokens: Vec::new(),
        focused: false,
    };
    projected.agents = vec![
        agent("p_api", "ws_1", crate::api::schema::AgentStatus::Working),
        agent("p_auth", "ws_2", crate::api::schema::AgentStatus::Blocked),
        agent("p_docs", "ws_4", crate::api::schema::AgentStatus::Idle),
    ];
    let mut state = folder_state(
        projected,
        layout(
            &[("f1", "backend", &["ws_2", "ws_3"])],
            &["ws_1", "f1", "ws_4"],
        ),
    );
    compose(&mut state);
    let toggle = state.hits.agent_sort_toggle;
    click(&mut state, toggle.x, toggle.y);
    click(&mut state, toggle.x, toggle.y);
    assert_eq!(state.folders.agent_mode, AgentPanelMode::Folders);
    let rows = compose(&mut state);
    let labels = state
        .hits
        .folder_agent_headers
        .iter()
        .map(|(rect, header)| (rect.y, header.clone()))
        .collect::<Vec<_>>();
    assert_eq!(
        labels
            .iter()
            .map(|(_, header)| header.clone())
            .collect::<Vec<_>>(),
        [
            FolderAgentHeader::Workspace(ClientEndpointId::Local, "ws_1".into()),
            FolderAgentHeader::Folder(ClientEndpointId::Local, "f1".into()),
            FolderAgentHeader::Workspace(ClientEndpointId::Local, "ws_2".into()),
            FolderAgentHeader::Workspace(ClientEndpointId::Local, "ws_4".into()),
        ],
        "billing has no agents, so its header is hidden"
    );
    let toggle = state.hits.agent_sort_toggle;
    assert!(rows[toggle.y as usize].contains("folders"));
    let auth_agent = state
        .hits
        .agents
        .iter()
        .find(|(_, pane)| pane == "p_auth")
        .map(|(rect, _)| *rect)
        .expect("agent row");
    assert!(!rows[auth_agent.y as usize].contains("auth"));

    let (folder_rect, _) = state.hits.folder_agent_headers[1].clone();
    click(&mut state, folder_rect.x + 4, folder_rect.y);
    assert!(state
        .folders
        .is_folder_collapsed(&ClientEndpointId::Local, "f1"));
    compose(&mut state);
    assert!(state
        .hits
        .workspaces
        .iter()
        .all(|hit| hit.workspace_id != "ws_2"));
    assert!(state.hits.agents.iter().all(|(_, pane)| pane != "p_auth"));
    assert_eq!(
        state.folder_agent_target(&crate::input::KeybindAction::FocusAgent(1)),
        Some(Some((ClientEndpointId::Local, "p_docs".into())))
    );
    let (api_rect, _) = state.hits.folder_agent_headers[0].clone();
    click(&mut state, api_rect.x + 4, api_rect.y);
    assert!(state
        .folders
        .collapsed_agent_workspaces
        .get(&ClientEndpointId::Local)
        .is_some_and(|collapsed| collapsed.contains("ws_1")));
    compose(&mut state);
    assert!(state.hits.agents.iter().all(|(_, pane)| pane != "p_api"));
    assert!(state
        .hits
        .workspaces
        .iter()
        .any(|hit| hit.workspace_id == "ws_1"));

    let toggle = state.hits.agent_sort_toggle;
    click(&mut state, toggle.x, toggle.y);
    assert_eq!(state.folders.agent_mode, AgentPanelMode::Grouped);
    compose(&mut state);
    assert_eq!(state.hits.agents.len(), 3);
}

#[test]
fn fork_state_is_saved_apart_from_upstream_preferences() {
    let dir = std::env::temp_dir().join(format!(
        "herdr-folder-prefs-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos())
    ));
    std::fs::create_dir_all(&dir).expect("dir");
    let upstream = dir.join("local-test.json");
    std::fs::write(
        &upstream,
        r#"{"agent_panel_sort":"priority","collapsed_groups":["repo"]}"#,
    )
    .expect("upstream prefs");
    let config =
        ClientShellConfig::from_config(&Config::default()).with_preferences_path(upstream.clone());
    let mut state = ClientShellState::new(config);
    assert_eq!(state.folders.agent_mode, AgentPanelMode::Priority);
    state.set_endpoint_methods(Some(all_methods()));
    state.set_snapshot(Box::new(folder_snapshot(&[("ws_2", "auth")])));
    state.set_pane_surface(surface());
    give_layout(
        &mut state,
        &ClientEndpointId::Local,
        layout(&[("f1", "x", &["ws_2"])], &["ws_1", "f1"]),
    );
    compose(&mut state);
    let toggle = state.hits.agent_sort_toggle;
    click(&mut state, toggle.x, toggle.y);
    assert_eq!(state.folders.agent_mode, AgentPanelMode::Folders);
    let backend = header(&state, "f1");
    click(&mut state, backend.x + 4, backend.y);
    state.collapsed_endpoints.insert(ClientEndpointId::Local);
    state.machine_collapse_changed();

    let saved: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&upstream).expect("read")).expect("json");
    assert_eq!(saved["agent_panel_sort"], "priority");
    assert!(serde_json::from_value::<preferences::ClientChromePreferences>(saved).is_ok());
    let fork_path = prefs::fork_state_path(&upstream);
    let fork: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&fork_path).expect("fork state"))
            .expect("json");
    assert_eq!(fork["agent_panel_mode"], "folders");
    assert_eq!(
        fork["collapsed_folders"]["local"],
        serde_json::json!(["f1"])
    );
    assert_eq!(fork["collapsed_machines"], serde_json::json!(["local"]));

    std::fs::write(&upstream, r#"{"sidebar_width":30}"#).expect("rewrite");
    let config = ClientShellConfig::from_config(&Config::default()).with_preferences_path(upstream);
    let restored = ClientShellState::new(config);
    assert_eq!(restored.folders.agent_mode, AgentPanelMode::Folders);
    assert!(restored
        .folders
        .is_folder_collapsed(&ClientEndpointId::Local, "f1"));
    assert!(restored
        .collapsed_endpoints
        .contains(&ClientEndpointId::Local));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn fork_config_key_selects_folders_mode_and_manual_choice_wins() {
    let config: Config = toml::from_str("[ui]\nagent_panel_mode = \"folders\"\n").expect("config");
    let state = ClientShellState::new(ClientShellConfig::from_config(&config));
    assert_eq!(state.folders.agent_mode, AgentPanelMode::Folders);
    assert_eq!(
        state.config.agent_panel_sort,
        crate::config::AgentPanelSortConfig::Spaces
    );
    let config: Config = toml::from_str("[ui]\nagent_panel_sort = \"priority\"\n").expect("config");
    let state = ClientShellState::new(ClientShellConfig::from_config(&config));
    assert_eq!(state.folders.agent_mode, AgentPanelMode::Priority);
    assert!(toml::from_str::<Config>("[ui]\nagent_panel_sort = \"folders\"\n").is_err());
}

fn remote_id() -> ClientEndpointId {
    ClientEndpointId::Ssh(
        crate::client::endpoint::ProfileId::parse("0123456789abcdef0123456789abcdef")
            .expect("profile id"),
    )
}

/// Local machine plus a remote "Build" machine, both with folders.
fn machines_state(local_folders: bool) -> ClientShellState {
    let mut state = backend_state();
    if !local_folders {
        state.set_endpoint_methods(Some(
            all_methods()
                .into_iter()
                .filter(|method| !method.starts_with("folder."))
                .collect(),
        ));
    }
    let remote = remote_id();
    let ClientEndpointId::Ssh(profile) = remote.clone() else {
        unreachable!("remote id");
    };
    state.set_endpoint_catalog(&[crate::client::endpoint::SavedSshEndpoint {
        id: profile,
        label: "Build".into(),
        target: "dev@build.example".into(),
        session: "agents".into(),
        enabled: true,
    }]);
    state.set_endpoint_status(&remote, ClientEndpointStatus::Online);
    let mut projected = folder_snapshot(&[("ws_r2", "remote-two")]);
    projected.boot_id = "remote-boot".into();
    projected.workspaces[0].label = "remote-one".into();
    state.set_endpoint_snapshot(&remote, Box::new(projected));
    state.set_endpoint_methods_for(&remote, Some(all_methods()));
    give_layout(
        &mut state,
        &remote,
        layout(&[("f1", "remote-folder", &["ws_r2"])], &["ws_1", "f1"]),
    );
    state.sidebar_section_split = 0.85;
    state
}

#[test]
fn machines_layout_nests_folders_and_toggles_any_machine_but_menus_only_active() {
    let mut state = machines_state(true);
    let rows = compose(&mut state);
    let remote = remote_id();
    let remote_header = state
        .hits
        .folder_headers
        .iter()
        .find(|hit| hit.endpoint_id == remote)
        .map(|hit| hit.rect)
        .expect("remote folder header");
    assert!(sidebar_text(&rows, remote_header.y).starts_with(" ▾ remote-folder"));
    let local_header = header(&state, "f1");
    assert!(sidebar_text(&rows, local_header.y).starts_with(" ▾ backend"));
    assert!(sidebar_text(&rows, row_of(&state, "ws_2").y).starts_with("   ○"));

    click(&mut state, remote_header.x + 6, remote_header.y);
    assert!(state.folders.is_folder_collapsed(&remote, "f1"));
    assert!(!state
        .folders
        .is_folder_collapsed(&ClientEndpointId::Local, "f1"));
    compose(&mut state);
    let remote_header = state
        .hits
        .folder_headers
        .iter()
        .find(|hit| hit.endpoint_id == remote)
        .map(|hit| hit.rect)
        .expect("remote folder header");
    mouse(
        &mut state,
        MouseEventKind::Down(MouseButton::Right),
        remote_header.x + 6,
        remote_header.y,
    );
    assert!(!matches!(
        state.overlay,
        Some(ClientShellOverlay::FolderMenu(_))
    ));
    state.overlay = None;
    mouse(
        &mut state,
        MouseEventKind::Down(MouseButton::Right),
        local_header.x + 6,
        local_header.y,
    );
    assert!(matches!(
        state.overlay,
        Some(ClientShellOverlay::FolderMenu(_))
    ));
}

#[test]
fn machines_layout_draws_drop_lines_for_machines_without_folders() {
    let mut state = machines_state(false);
    compose(&mut state);
    assert!(state
        .hits
        .folder_headers
        .iter()
        .all(|hit| hit.endpoint_id != ClientEndpointId::Local));
    let api = row_of(&state, "ws_1");
    let docs = row_of(&state, "ws_4");
    mouse(
        &mut state,
        MouseEventKind::Down(MouseButton::Left),
        api.x + 4,
        api.y,
    );
    mouse(
        &mut state,
        MouseEventKind::Drag(MouseButton::Left),
        docs.x + 4,
        docs.y + 1,
    );
    assert!(matches!(
        state.chrome_drag,
        Some(ClientChromeDrag::Workspace { .. })
    ));
    let buffer = styled(&mut state);
    let rows = compose(&mut state);
    let line = rows
        .iter()
        .position(|row| row.chars().take(25).collect::<String>().contains("────"))
        .expect("drop line drawn in the machines layout");
    assert!(line as u16 >= docs.y);
    assert_eq!(buffer[(api.x + 4, api.y)].bg, state.config.palette.surface1);
    let outcome = mouse(
        &mut state,
        MouseEventKind::Up(MouseButton::Left),
        docs.x + 4,
        docs.y + 1,
    );
    assert!(matches!(
        &methods(&outcome)[..],
        [crate::api::schema::Method::WorkspaceMove(_)]
    ));
}

#[test]
fn compact_sidebar_draws_no_folders() {
    let mut state = backend_state();
    state.sidebar_collapsed = true;
    compose(&mut state);
    assert!(state.hits.folder_headers.is_empty());
    assert_eq!(state.hits.workspaces.len(), 4);
}

#[test]
fn agent_view_overrides_folders_mode_and_multi_machine_tree_has_machine_headers() {
    let mut state = machines_state(true);
    state.sidebar_section_split = 0.4;
    state.folders.agent_mode = AgentPanelMode::Folders;
    let remote = remote_id();
    let mut projected = state
        .endpoints
        .iter()
        .find(|endpoint| endpoint.endpoint_id == remote)
        .and_then(|endpoint| endpoint.snapshot.as_deref().cloned())
        .expect("remote snapshot");
    projected.revision += 1;
    projected.agents = vec![ClientShellAgent {
        pane_id: "p_remote".into(),
        workspace_id: "ws_r2".into(),
        tab_id: "tab_1".into(),
        name: Some("codex".into()),
        display_agent: None,
        agent: Some("codex".into()),
        title: None,
        terminal_title: None,
        terminal_title_stripped: None,
        agent_status: crate::api::schema::AgentStatus::Working,
        state_change_seq: 1,
        state_labels: Vec::new(),
        tokens: Vec::new(),
        focused: false,
    }];
    state.set_endpoint_snapshot(&remote, Box::new(projected));
    compose(&mut state);
    let headers = state
        .hits
        .folder_agent_headers
        .iter()
        .map(|(_, header)| header.clone())
        .collect::<Vec<_>>();
    assert_eq!(
        headers,
        [
            FolderAgentHeader::Machine(remote.clone()),
            FolderAgentHeader::Folder(remote.clone(), "f1".into()),
            FolderAgentHeader::Workspace(remote.clone(), "ws_r2".into()),
        ]
    );
    let (machine_rect, _) = state.hits.folder_agent_headers[0].clone();
    let outcome = click(&mut state, machine_rect.x + 4, machine_rect.y);
    assert!(state.collapsed_endpoints.contains(&remote));
    assert!(outcome
        .actions
        .iter()
        .all(|action| !matches!(action, ClientShellAction::ActivateEndpoint { .. })));

    let mut local = state.snapshot.as_deref().expect("snapshot").clone();
    local.revision += 1;
    local.agent_view_label = Some("working".into());
    state.set_snapshot(Box::new(local));
    let mut matching = surface();
    matching.projection_revision = state.snapshot.as_deref().expect("snapshot").revision;
    state.set_pane_surface(matching);
    compose(&mut state);
    assert!(state.hits.folder_agent_headers.is_empty());
}

#[test]
fn multi_machine_next_previous_skips_collapsed_folders() {
    let mut state = machines_state(true);
    state.folders.toggle_folder(&ClientEndpointId::Local, "f1");
    compose(&mut state);
    let mut outcome = ClientShellInput::default();
    assert!(state.folder_multi_machine_step(true, &mut outcome));
    assert!(matches!(
        &outcome.actions[..],
        [ClientShellAction::ActivateEndpoint {
            target: Some(ClientEndpointFocusTarget::Workspace(workspace_id)),
            ..
        }] if workspace_id == "ws_4"
    ));

    state.sidebar_collapsed = true;
    let mut outcome = ClientShellInput::default();
    assert!(!state.folder_multi_machine_step(true, &mut outcome));
}

#[test]
fn multi_machine_reveal_scrolls_to_a_collapsed_folder_hiding_focus() {
    let mut state = machines_state(true);
    let loose = (2..=20)
        .map(|index| (format!("ws_{index}"), format!("loose {index}")))
        .collect::<Vec<_>>();
    let mut extra = loose
        .iter()
        .map(|(id, label)| (id.as_str(), label.as_str()))
        .collect::<Vec<_>>();
    extra.push(("ws_last", "hidden"));
    let mut projected = folder_snapshot(&extra);
    projected.revision += 1;
    projected.focused_workspace_id = Some("ws_last".into());
    for workspace in &mut projected.workspaces {
        workspace.focused = workspace.workspace_id == "ws_last";
    }
    state.set_snapshot(Box::new(projected.clone()));
    let mut matching = surface();
    matching.projection_revision = projected.revision;
    state.set_pane_surface(matching);
    let mut order = vec!["ws_1"];
    order.extend(loose.iter().map(|(id, _)| id.as_str()));
    order.push("f1");
    give_layout(
        &mut state,
        &ClientEndpointId::Local,
        layout(&[("f1", "tail", &["ws_last"])], &order),
    );
    state.folders.toggle_folder(&ClientEndpointId::Local, "f1");
    state.workspace_scroll = 0;
    state.reveal_focused_workspace = true;
    compose(&mut state);
    assert!(
        state
            .hits
            .folder_headers
            .iter()
            .any(|hit| hit.endpoint_id == ClientEndpointId::Local && hit.folder_id == "f1"),
        "the collapsed header hiding focus is scrolled into view"
    );
}

#[test]
fn navigate_mode_skips_collapsed_folders_and_starts_at_a_hidden_focus() {
    let mut state = backend_state();
    state.folders.toggle_folder(&ClientEndpointId::Local, "f1");
    compose(&mut state);
    let target = |state: &ClientShellState| {
        state
            .navigate_workspace_id
            .as_ref()
            .map(|target| target.workspace_id.clone())
    };
    state.move_navigate_workspace(1);
    assert_eq!(target(&state).as_deref(), Some("ws_1"));
    state.move_navigate_workspace(1);
    assert_eq!(
        target(&state).as_deref(),
        Some("ws_4"),
        "collapsed members are skipped"
    );
    state.move_navigate_workspace(-1);
    assert_eq!(target(&state).as_deref(), Some("ws_1"));
    let mut hidden = state.navigate_workspace_id.clone().expect("target");
    hidden.workspace_id = "ws_3".into();
    state.navigate_workspace_id = Some(hidden.clone());
    state.move_navigate_workspace(1);
    assert_eq!(target(&state).as_deref(), Some("ws_4"));
    state.navigate_workspace_id = Some(hidden);
    state.move_navigate_workspace(-1);
    assert_eq!(target(&state).as_deref(), Some("ws_1"));
    assert!(state
        .folders
        .is_folder_collapsed(&ClientEndpointId::Local, "f1"));
}

#[test]
fn long_move_to_folder_lists_scroll() {
    let names = (1..=30)
        .map(|index| (format!("f{index}"), format!("folder {index}")))
        .collect::<Vec<_>>();
    let folders = names
        .iter()
        .map(|(id, name)| (id.as_str(), name.as_str(), &[][..]))
        .collect::<Vec<(&str, &str, &[&str])>>();
    let mut order = vec!["ws_1"];
    order.extend(names.iter().map(|(id, _)| id.as_str()));
    let mut state = folder_state(folder_snapshot(&[]), layout(&folders, &order));
    compose(&mut state);
    let api = row_of(&state, "ws_1");
    mouse(
        &mut state,
        MouseEventKind::Down(MouseButton::Right),
        api.x + 2,
        api.y,
    );
    let index = match state.overlay.as_ref() {
        Some(ClientShellOverlay::ContextMenu(menu)) => menu
            .items()
            .iter()
            .position(|item| item.action == ClientContextMenuAction::MoveToFolder)
            .expect("item"),
        _ => panic!("menu"),
    };
    assert!(state.open_folder_submenu(index));
    compose(&mut state);
    let visible = state.hits.folder_menu_rows.len();
    assert!(visible < 31, "the list is cut to the screen");
    for _ in 0..29 {
        state.handle_input_bytes(b"\x1b[B");
    }
    compose(&mut state);
    let shown = state
        .hits
        .folder_menu_rows
        .iter()
        .map(|(_, index)| *index)
        .collect::<Vec<_>>();
    assert!(
        shown.contains(&29),
        "scrolled to the highlighted item: {shown:?}"
    );
    assert!(!shown.contains(&0));
}
