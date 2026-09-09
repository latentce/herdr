//! Sidebar folder tests.

use super::*;
use crate::protocol::{ClientShellFolder, ClientShellSpaceOrderEntry};

fn workspace(id: &str, label: &str, folder_id: Option<&str>) -> ClientShellWorkspace {
    ClientShellWorkspace {
        workspace_id: id.into(),
        active_tab_id: format!("{id}:tab"),
        new_workspace_cwd: "/repo".into(),
        number: 0,
        label: label.into(),
        custom_label: false,
        branch: None,
        git_ahead_behind: None,
        tokens: Vec::new(),
        worktree: None,
        focused: false,
        agent_status: AgentStatus::Idle,
        folder_id: folder_id.map(str::to_owned),
    }
}

/// Spaces `a`, `[work: b, c]`, `d`, with `a` focused.
fn foldered_snapshot() -> ClientShellSnapshot {
    let mut snapshot = snapshot();
    snapshot.workspaces = vec![
        workspace("ws_a", "alpha", None),
        workspace("ws_b", "bravo", Some("f_1")),
        workspace("ws_c", "charlie", Some("f_1")),
        workspace("ws_d", "delta", None),
    ];
    snapshot.workspaces[0].focused = true;
    snapshot.focused_workspace_id = Some("ws_a".into());
    snapshot.focused_tab_id = None;
    snapshot.focused_pane_id = None;
    snapshot.tabs.clear();
    snapshot.panes.clear();
    snapshot.space_order = vec![
        ClientShellSpaceOrderEntry::Workspace("ws_a".into()),
        ClientShellSpaceOrderEntry::Folder("f_1".into()),
        ClientShellSpaceOrderEntry::Workspace("ws_d".into()),
    ];
    snapshot.folders = vec![ClientShellFolder {
        folder_id: "f_1".into(),
        name: "work".into(),
        members: vec!["ws_b".into(), "ws_c".into()],
    }];
    snapshot
}

fn state_with(snapshot: ClientShellSnapshot) -> ClientShellState {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(snapshot));
    state.set_pane_surface(surface());
    state
}

fn click(
    state: &mut ClientShellState,
    kind: MouseEventKind,
    column: u16,
    row: u16,
) -> ClientShellInput {
    state.handle_raw_events(vec![RawInputEvent::Mouse(crossterm::event::MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::empty(),
    })])
}

fn row_text(frame: &FrameData, row: u16) -> String {
    let width = frame.width as usize;
    frame.cells[row as usize * width..(row as usize + 1) * width]
        .iter()
        .map(|cell| cell.symbol.as_str())
        .collect::<String>()
}

fn entry_ids(snapshot: &ClientShellSnapshot, entries: &[folders::SidebarEntry]) -> Vec<String> {
    entries
        .iter()
        .map(|entry| match entry {
            folders::SidebarEntry::Folder { folder_index } => {
                format!("folder:{}", snapshot.folders[*folder_index].folder_id)
            }
            folders::SidebarEntry::Workspace { entry, foldered } => format!(
                "{}{}",
                if *foldered { "  " } else { "" },
                snapshot.workspaces[entry.index].workspace_id
            ),
        })
        .collect()
}

#[test]
fn sidebar_entries_nest_folder_members_under_their_header() {
    let snapshot = foldered_snapshot();
    let entries = folders::sidebar_entries(&snapshot, &HashSet::new(), &HashSet::new(), false);
    assert_eq!(
        entry_ids(&snapshot, &entries),
        ["ws_a", "folder:f_1", "  ws_b", "  ws_c", "ws_d"]
    );
}

#[test]
fn collapsed_folder_hides_every_member_and_navigation_skips_them() {
    let snapshot = foldered_snapshot();
    let collapsed = HashSet::from(["f_1".to_string()]);
    let entries = folders::sidebar_entries(&snapshot, &HashSet::new(), &collapsed, false);
    assert_eq!(
        entry_ids(&snapshot, &entries),
        ["ws_a", "folder:f_1", "ws_d"]
    );

    let mut state = state_with(snapshot);
    state.folder_collapse_mut().folders = collapsed;
    state.mode = ClientShellMode::Navigate;
    state.navigate_workspace_id = Some("ws_b".into());
    state.compose(106, 24).expect("frame");
    state.handle_raw_events(vec![RawInputEvent::Key(crate::input::TerminalKey::new(
        KeyCode::Down,
        KeyModifiers::empty(),
    ))]);
    assert_eq!(state.navigate_workspace_id.as_deref(), Some("ws_d"));
    state.navigate_workspace_id = Some("ws_c".into());
    state.handle_raw_events(vec![RawInputEvent::Key(crate::input::TerminalKey::new(
        KeyCode::Up,
        KeyModifiers::empty(),
    ))]);
    assert_eq!(state.navigate_workspace_id.as_deref(), Some("ws_a"));
}

#[test]
fn spaces_panel_renders_folder_header_and_toggles_collapse_on_click() {
    let mut state = state_with(foldered_snapshot());
    let frame = state.compose(106, 24).expect("frame");

    let header = state
        .hits
        .folders
        .headers
        .iter()
        .find(|header| header.folder_id == "f_1")
        .map(|header| header.rect)
        .expect("folder header hit");
    let text = row_text(&frame, header.y);
    assert!(text.contains("▾ work"), "header row: {text:?}");
    let member = state
        .hits
        .workspaces
        .iter()
        .find(|hit| hit.workspace_id == "ws_b")
        .expect("member hit");
    assert!(member.foldered);
    let loose = state
        .hits
        .workspaces
        .iter()
        .find(|hit| hit.workspace_id == "ws_a")
        .expect("loose hit");
    assert!(!loose.foldered);
    let loose_text = row_text(&frame, loose.rect.y);
    let member_text = row_text(&frame, member.rect.y);
    assert_eq!(
        loose_text.find("alpha").map(|x| x + 2),
        member_text.find("bravo")
    );

    click(
        &mut state,
        MouseEventKind::Down(MouseButton::Left),
        header.x + 6,
        header.y,
    );
    click(
        &mut state,
        MouseEventKind::Up(MouseButton::Left),
        header.x + 6,
        header.y,
    );
    assert!(state.collapsed_folders().contains("f_1"));
    let frame = state.compose(106, 24).expect("collapsed frame");
    assert!(row_text(&frame, header.y).contains("▸ work"));
    assert!(!state
        .hits
        .workspaces
        .iter()
        .any(|hit| hit.workspace_id == "ws_b"));
    assert!(state
        .hits
        .workspaces
        .iter()
        .any(|hit| hit.workspace_id == "ws_d"));
}

#[test]
fn folder_context_menu_renames_and_deletes_through_the_api() {
    let mut state = state_with(foldered_snapshot());
    state.compose(106, 24).expect("frame");
    let header = state.hits.folders.headers[0].rect;

    click(
        &mut state,
        MouseEventKind::Down(MouseButton::Right),
        header.x + 4,
        header.y,
    );
    let labels = match state.overlay.as_ref() {
        Some(ClientShellOverlay::ContextMenu(menu)) => menu
            .items()
            .iter()
            .map(|item| item.label.to_string())
            .collect::<Vec<_>>(),
        other => panic!("folder context menu, got {other:?}"),
    };
    assert_eq!(labels, ["Rename", "Delete"]);

    let mut outcome = ClientShellInput::default();
    state.activate_context_menu_item(0, &mut outcome);
    assert!(matches!(
        state.overlay.as_ref(),
        Some(ClientShellOverlay::Rename(ClientRenameOverlay {
            title: "rename folder",
            target: ClientRenameTarget::Folder { folder_id },
            ..
        })) if folder_id == "f_1"
    ));
    assert!(state.handle_input_bytes(&[0x15]).actions.is_empty());
    assert!(state.handle_input_bytes(b"active").actions.is_empty());
    let save = state.handle_input_bytes(b"\r");
    let [ClientShellAction::Endpoint { request, .. }] = &save.actions[..] else {
        panic!("folder rename should use the endpoint API");
    };
    assert!(matches!(
        &request.method,
        crate::api::schema::Method::FolderRename(params)
            if params.folder_id == "f_1" && params.name == "active"
    ));

    state.open_folder_context_menu("f_1".into(), header.x, header.y);
    let mut outcome = ClientShellInput::default();
    state.activate_context_menu_item(1, &mut outcome);
    let [ClientShellAction::Endpoint { request, .. }] = &outcome.actions[..] else {
        panic!("folder delete should use the endpoint API");
    };
    assert!(matches!(
        &request.method,
        crate::api::schema::Method::FolderDelete(target) if target.folder_id == "f_1"
    ));
}

#[test]
fn space_context_menu_moves_between_folders_and_creates_new_ones() {
    let mut state = state_with(foldered_snapshot());
    state.compose(106, 24).expect("frame");

    state.open_workspace_context_menu("ws_a".into(), 4, 4);
    let loose_items = match state.overlay.as_ref() {
        Some(ClientShellOverlay::ContextMenu(menu)) => menu.items(),
        _ => panic!("space context menu"),
    };
    assert_eq!(loose_items[0].label, "Rename");
    assert_eq!(loose_items[1].label, folders::MENU_ITEM_MOVE_TO_FOLDER);
    assert!(!loose_items
        .iter()
        .any(|item| item.action == ClientContextMenuAction::RemoveFromFolder));

    state.open_workspace_context_menu("ws_b".into(), 4, 4);
    let foldered_items = match state.overlay.as_ref() {
        Some(ClientShellOverlay::ContextMenu(menu)) => menu.items(),
        _ => panic!("space context menu"),
    };
    let remove = foldered_items
        .iter()
        .position(|item| item.action == ClientContextMenuAction::RemoveFromFolder)
        .expect("remove from folder item");
    let mut outcome = ClientShellInput::default();
    state.activate_context_menu_item(remove, &mut outcome);
    let [ClientShellAction::Endpoint { request, .. }] = &outcome.actions[..] else {
        panic!("remove from folder should use the endpoint API");
    };
    assert!(matches!(
        &request.method,
        crate::api::schema::Method::FolderAssign(params)
            if params.workspace_id == "ws_b" && params.folder_id.is_none() && params.position.is_none()
    ));

    state.open_workspace_context_menu("ws_a".into(), 4, 4);
    let mut outcome = ClientShellInput::default();
    state.activate_context_menu_item(1, &mut outcome);
    let submenu = match state.overlay.as_ref() {
        Some(ClientShellOverlay::ContextMenu(menu)) => menu
            .items()
            .iter()
            .map(|item| item.label.to_string())
            .collect::<Vec<_>>(),
        _ => panic!("move-to-folder submenu"),
    };
    assert_eq!(submenu, ["work", folders::MENU_ITEM_NEW_FOLDER]);
    let mut outcome = ClientShellInput::default();
    state.activate_context_menu_item(0, &mut outcome);
    let [ClientShellAction::Endpoint { request, .. }] = &outcome.actions[..] else {
        panic!("move to folder should use the endpoint API");
    };
    assert!(matches!(
        &request.method,
        crate::api::schema::Method::FolderAssign(params)
            if params.workspace_id == "ws_a" && params.folder_id.as_deref() == Some("f_1")
    ));

    state.open_move_to_folder_menu("ws_a".into(), 4, 4);
    let mut outcome = ClientShellInput::default();
    state.activate_context_menu_item(1, &mut outcome);
    assert!(matches!(
        state.overlay.as_ref(),
        Some(ClientShellOverlay::Rename(ClientRenameOverlay {
            title: "new folder",
            target: ClientRenameTarget::NewFolder {
                move_workspace_id: Some(workspace_id),
            },
            ..
        })) if workspace_id == "ws_a"
    ));
    assert!(state.handle_input_bytes(b"projects").actions.is_empty());
    let save = state.handle_input_bytes(b"\r");
    let [ClientShellAction::Endpoint { request, .. }] = &save.actions[..] else {
        panic!("folder create should use the endpoint API");
    };
    assert!(matches!(
        &request.method,
        crate::api::schema::Method::FolderCreate(params) if params.name == "projects"
    ));
    let (_, follow_up) = state.handle_endpoint_result(
        "boot-1",
        &request.id,
        Ok(crate::api::schema::ResponseResult::FolderCreated {
            folder_id: "f_2".into(),
        }),
    );
    let [ClientShellAction::Endpoint { request, .. }] = &follow_up[..] else {
        panic!("create-and-move should file the space into the new folder");
    };
    assert!(matches!(
        &request.method,
        crate::api::schema::Method::FolderAssign(params)
            if params.workspace_id == "ws_a" && params.folder_id.as_deref() == Some("f_2")
    ));
}

#[test]
fn spaces_panel_background_menu_creates_an_empty_folder() {
    let mut state = state_with(foldered_snapshot());
    state.compose(106, 40).expect("frame");
    let body = state.hits.workspace_body;
    let last_row = state
        .hits
        .workspaces
        .iter()
        .map(|hit| hit.rect.bottom())
        .max()
        .expect("workspace rows");
    let empty_row = last_row + 2;
    assert!(
        empty_row < body.bottom(),
        "panel must have empty rows below the list"
    );

    click(
        &mut state,
        MouseEventKind::Down(MouseButton::Right),
        body.x + 2,
        empty_row,
    );
    assert!(matches!(
        state.overlay.as_ref(),
        Some(ClientShellOverlay::ContextMenu(ClientContextMenuOverlay {
            target: ClientContextMenuTarget::SpacesPanel,
            ..
        }))
    ));
    let mut outcome = ClientShellInput::default();
    state.activate_context_menu_item(0, &mut outcome);
    assert!(matches!(
        state.overlay.as_ref(),
        Some(ClientShellOverlay::Rename(ClientRenameOverlay {
            target: ClientRenameTarget::NewFolder {
                move_workspace_id: None
            },
            ..
        }))
    ));
    let save = state.handle_input_bytes(b"\r");
    assert!(save.actions.is_empty());
}

#[test]
fn spaces_panel_title_row_right_click_opens_the_panel_menu() {
    let mut state = state_with(foldered_snapshot());
    state.compose(106, 40).expect("frame");
    let body = state.hits.workspace_body;
    let title_row = body.y - WORKSPACE_HEADER_ROWS;

    click(
        &mut state,
        MouseEventKind::Down(MouseButton::Right),
        body.x + 2,
        title_row,
    );
    assert!(
        matches!(
            state.overlay.as_ref(),
            Some(ClientShellOverlay::ContextMenu(ClientContextMenuOverlay {
                target: ClientContextMenuTarget::SpacesPanel,
                ..
            }))
        ),
        "the ' spaces' title row is part of the panel background"
    );
}

#[test]
fn drop_slot_rows_settle_without_collisions_and_keep_end_reachable() {
    use folders::{settle_slot_rows, SpaceDropTarget as T};
    let in_folder_end = || T::InFolderEnd {
        folder_id: "f_1".into(),
    };
    let rows = |slots: &[folders::SpaceDropSlot]| {
        slots
            .iter()
            .map(|slot| (slot.target.clone(), slot.row))
            .collect::<Vec<_>>()
    };

    // Two slots want the boundary row; the top-level one moves down.
    let settled = settle_slot_rows(
        vec![
            (in_folder_end(), 5),
            (T::Before("ws_d".into()), 5),
            (T::End, 6),
        ],
        20,
    );
    assert_eq!(
        rows(&settled),
        [
            (in_folder_end(), Some(5)),
            (T::Before("ws_d".into()), Some(6)),
            (T::End, Some(7)),
        ]
    );

    // One free row below the last folder: the end-of-folder slot yields to `End`.
    let settled = settle_slot_rows(
        vec![
            (T::BeforeFolder("f_1".into()), 1),
            (in_folder_end(), 5),
            (T::End, 5),
        ],
        6,
    );
    assert_eq!(
        rows(&settled),
        [(T::BeforeFolder("f_1".into()), Some(1)), (T::End, Some(5)),]
    );

    // Slots past the limit are dropped.
    let settled = settle_slot_rows(vec![(T::Before("ws_a".into()), 3), (T::End, 4)], 4);
    assert_eq!(rows(&settled), [(T::Before("ws_a".into()), Some(3))]);

    // `End` naturally sits on the limit row; earlier end-of-folder slots survive.
    let in_folder_end_2 = || T::InFolderEnd {
        folder_id: "f_2".into(),
    };
    let settled = settle_slot_rows(
        vec![
            (T::BeforeFolder("f_1".into()), 1),
            (in_folder_end(), 4),
            (T::BeforeFolder("f_2".into()), 5),
            (in_folder_end_2(), 9),
            (T::Before("ws_z".into()), 9),
            (T::End, 12),
        ],
        12,
    );
    assert_eq!(
        rows(&settled),
        [
            (T::BeforeFolder("f_1".into()), Some(1)),
            (in_folder_end(), Some(4)),
            (T::BeforeFolder("f_2".into()), Some(5)),
            (in_folder_end_2(), Some(9)),
            (T::Before("ws_z".into()), Some(10)),
        ]
    );

    // Only the end-of-folder slot in the colliding run is sacrificed.
    let settled = settle_slot_rows(
        vec![
            (in_folder_end(), 2),
            (T::Before("ws_m".into()), 4),
            (in_folder_end_2(), 8),
            (T::Before("ws_z".into()), 8),
            (T::End, 9),
        ],
        10,
    );
    assert_eq!(
        rows(&settled),
        [
            (in_folder_end(), Some(2)),
            (T::Before("ws_m".into()), Some(4)),
            (T::Before("ws_z".into()), Some(8)),
            (T::End, Some(9)),
        ]
    );
}

#[test]
fn dragging_onto_the_first_member_row_reaches_in_folder_position_zero() {
    let mut state = state_with(foldered_snapshot());
    state.compose(106, 30).expect("frame");
    let rect_of = |state: &ClientShellState, id: &str| {
        state
            .hits
            .workspaces
            .iter()
            .find(|hit| hit.workspace_id == id)
            .map(|hit| hit.rect)
            .expect("workspace hit")
    };
    let source = rect_of(&state, "ws_d");
    let first_member = rect_of(&state, "ws_b");

    click(
        &mut state,
        MouseEventKind::Down(MouseButton::Left),
        source.x + 3,
        source.y,
    );
    click(
        &mut state,
        MouseEventKind::Drag(MouseButton::Left),
        first_member.x + 3,
        first_member.y,
    );
    assert!(matches!(
        state.chrome_drag.as_ref(),
        Some(ClientChromeDrag::SpaceOrder {
            target: Some(folders::SpaceDropSlot {
                target: folders::SpaceDropTarget::InFolderBefore { folder_id, workspace_id },
                row: Some(_),
            }),
            ..
        }) if folder_id == "f_1" && workspace_id == "ws_b"
    ));
    let drop = click(
        &mut state,
        MouseEventKind::Up(MouseButton::Left),
        first_member.x + 3,
        first_member.y,
    );
    let [ClientShellAction::Endpoint { request, .. }] = &drop.actions[..] else {
        panic!("dropping on the first member's row should assign at position 0");
    };
    assert!(matches!(
        &request.method,
        crate::api::schema::Method::FolderAssign(params)
            if params.workspace_id == "ws_d"
                && params.folder_id.as_deref() == Some("f_1")
                && params.position == Some(0)
    ));
}

#[test]
fn every_drop_slot_is_reachable_with_single_row_cards_and_no_row_gap() {
    use folders::SpaceDropTarget as T;
    let mut state = state_with(foldered_snapshot());
    state.compose(106, 30).expect("frame");
    let body = state.hits.workspace_body;
    let source = folders::SpaceDragSource::Workspace("ws_a".into());

    let mut reached = Vec::new();
    for row in body.y.saturating_sub(1)..body.bottom() {
        if let Some(slot) = state.space_drop_slot_at((body.x + 3, row), &source) {
            if reached.last() != Some(&slot.target) {
                reached.push(slot.target);
            }
        }
    }
    assert_eq!(
        reached,
        [
            T::Before("ws_a".into()),
            T::BeforeFolder("f_1".into()),
            T::IntoFolder("f_1".into()),
            T::InFolderBefore {
                folder_id: "f_1".into(),
                workspace_id: "ws_b".into(),
            },
            T::InFolderBefore {
                folder_id: "f_1".into(),
                workspace_id: "ws_c".into(),
            },
            T::InFolderEnd {
                folder_id: "f_1".into(),
            },
            T::Before("ws_d".into()),
            T::End,
        ]
    );
}

#[test]
fn folder_dragged_to_the_end_moves_after_the_last_top_level_entry() {
    let state = state_with(foldered_snapshot());
    assert!(matches!(
        state.space_drop_method(
            &folders::SpaceDragSource::Folder("f_1".into()),
            &folders::SpaceDropTarget::End,
        ),
        Some(crate::api::schema::Method::FolderMove(params))
            if params.folder_id == "f_1" && params.position == 2
    ));

    let mut snapshot = foldered_snapshot();
    snapshot.space_order = vec![
        ClientShellSpaceOrderEntry::Workspace("ws_a".into()),
        ClientShellSpaceOrderEntry::Workspace("ws_d".into()),
        ClientShellSpaceOrderEntry::Folder("f_1".into()),
    ];
    let state = state_with(snapshot);
    assert!(state
        .space_drop_method(
            &folders::SpaceDragSource::Folder("f_1".into()),
            &folders::SpaceDropTarget::End,
        )
        .is_none());
}

#[test]
fn folder_press_that_moves_without_a_slot_does_not_toggle_collapse_on_release() {
    let mut state = state_with(foldered_snapshot());
    state.compose(106, 30).expect("frame");
    let header = state.hits.folders.headers[0].rect;

    click(
        &mut state,
        MouseEventKind::Down(MouseButton::Left),
        header.x + 5,
        header.y,
    );
    // Drag above the spaces panel, where no slot exists.
    let body = state.hits.workspace_body;
    assert!(body.y >= 2, "the panel sits below the top bar");
    let outside = (header.x + 5, body.y - 2);
    click(
        &mut state,
        MouseEventKind::Drag(MouseButton::Left),
        outside.0,
        outside.1,
    );
    assert!(matches!(
        state.chrome_drag.as_ref(),
        Some(ClientChromeDrag::SpaceOrder {
            source: folders::SpaceDragSource::Folder(folder_id),
            target: None,
        }) if folder_id == "f_1"
    ));
    let release = click(
        &mut state,
        MouseEventKind::Up(MouseButton::Left),
        outside.0,
        outside.1,
    );
    assert!(release.actions.is_empty());
    assert!(state.chrome_drag.is_none());
    assert!(
        !state.collapsed_folders().contains("f_1"),
        "a moved press is a drag, not a click"
    );
}

#[test]
fn dragging_a_space_onto_a_folder_header_files_it_and_folder_drags_reorder() {
    let mut state = state_with(foldered_snapshot());
    state.compose(106, 30).expect("frame");
    let header = state.hits.folders.headers[0].rect;
    let source = state
        .hits
        .workspaces
        .iter()
        .find(|hit| hit.workspace_id == "ws_d")
        .map(|hit| hit.rect)
        .expect("delta hit");

    click(
        &mut state,
        MouseEventKind::Down(MouseButton::Left),
        source.x + 3,
        source.y,
    );
    click(
        &mut state,
        MouseEventKind::Drag(MouseButton::Left),
        header.x + 3,
        header.y,
    );
    assert!(matches!(
        state.chrome_drag.as_ref(),
        Some(ClientChromeDrag::SpaceOrder {
            source: folders::SpaceDragSource::Workspace(id),
            target: Some(folders::SpaceDropSlot {
                target: folders::SpaceDropTarget::IntoFolder(folder_id),
                row: None,
            }),
        }) if id == "ws_d" && folder_id == "f_1"
    ));
    let drop = click(
        &mut state,
        MouseEventKind::Up(MouseButton::Left),
        header.x + 3,
        header.y,
    );
    let [ClientShellAction::Endpoint { request, .. }] = &drop.actions[..] else {
        panic!("dropping onto a folder header should assign through the API");
    };
    assert!(matches!(
        &request.method,
        crate::api::schema::Method::FolderAssign(params)
            if params.workspace_id == "ws_d"
                && params.folder_id.as_deref() == Some("f_1")
                && params.position.is_none()
    ));
    assert!(state.chrome_drag.is_none());

    state.compose(106, 30).expect("frame");
    let header = state.hits.folders.headers[0].rect;
    let first = state
        .hits
        .workspaces
        .iter()
        .find(|hit| hit.workspace_id == "ws_a")
        .map(|hit| hit.rect)
        .expect("alpha hit");
    click(
        &mut state,
        MouseEventKind::Down(MouseButton::Left),
        header.x + 5,
        header.y,
    );
    click(
        &mut state,
        MouseEventKind::Drag(MouseButton::Left),
        first.x + 5,
        first.y.saturating_sub(1),
    );
    assert!(matches!(
        state.chrome_drag.as_ref(),
        Some(ClientChromeDrag::SpaceOrder {
            source: folders::SpaceDragSource::Folder(folder_id),
            target: Some(folders::SpaceDropSlot {
                target: folders::SpaceDropTarget::Before(before),
                ..
            }),
        }) if folder_id == "f_1" && before == "ws_a"
    ));
    let drop = click(
        &mut state,
        MouseEventKind::Up(MouseButton::Left),
        first.x + 5,
        first.y.saturating_sub(1),
    );
    let [ClientShellAction::Endpoint { request, .. }] = &drop.actions[..] else {
        panic!("folder drop should move through the API");
    };
    assert!(matches!(
        &request.method,
        crate::api::schema::Method::FolderMove(params)
            if params.folder_id == "f_1" && params.position == 0
    ));
    // Releasing after a drag never toggles collapse.
    assert!(!state.collapsed_folders().contains("f_1"));
}

#[test]
fn in_folder_and_top_level_drops_compute_container_positions() {
    let state = state_with(foldered_snapshot());
    let assign = |target| {
        state.space_drop_method(&folders::SpaceDragSource::Workspace("ws_a".into()), &target)
    };
    assert!(matches!(
        assign(folders::SpaceDropTarget::InFolderBefore {
            folder_id: "f_1".into(),
            workspace_id: "ws_c".into(),
        }),
        Some(crate::api::schema::Method::FolderAssign(params))
            if params.folder_id.as_deref() == Some("f_1") && params.position == Some(1)
    ));
    assert!(matches!(
        assign(folders::SpaceDropTarget::End),
        Some(crate::api::schema::Method::FolderAssign(params))
            if params.folder_id.is_none() && params.position.is_none()
    ));
    assert!(matches!(
        assign(folders::SpaceDropTarget::Before("ws_d".into())),
        Some(crate::api::schema::Method::FolderAssign(params))
            if params.folder_id.is_none() && params.position == Some(1)
    ));
    assert!(assign(folders::SpaceDropTarget::BeforeFolder("f_1".into())).is_none());
    assert!(state
        .space_drop_method(
            &folders::SpaceDragSource::Workspace("ws_b".into()),
            &folders::SpaceDropTarget::IntoFolder("f_1".into()),
        )
        .is_some());
    assert!(state
        .space_drop_method(
            &folders::SpaceDragSource::Workspace("ws_c".into()),
            &folders::SpaceDropTarget::IntoFolder("f_1".into()),
        )
        .is_none());
    assert!(state
        .space_drop_method(
            &folders::SpaceDragSource::Folder("f_1".into()),
            &folders::SpaceDropTarget::InFolderEnd {
                folder_id: "f_1".into()
            },
        )
        .is_none());
}

#[test]
fn agents_panel_folder_view_nests_headers_and_collapses_agent_lists() {
    let mut snapshot = foldered_snapshot();
    for (index, workspace_id) in ["ws_a", "ws_b", "ws_d"].into_iter().enumerate() {
        let pane_id = format!("pane_{workspace_id}");
        snapshot.agents.push(ClientShellAgent {
            pane_id: pane_id.clone(),
            workspace_id: workspace_id.into(),
            tab_id: format!("{workspace_id}:tab"),
            agent: Some("codex".into()),
            display_agent: None,
            name: None,
            title: None,
            terminal_title: None,
            terminal_title_stripped: None,
            agent_status: AgentStatus::Working,
            state_change_seq: index as u64,
            state_labels: Vec::new(),
            tokens: Vec::new(),
            focused: false,
        });
    }
    let mut state = state_with(snapshot);
    state.config.agent_panel_sort = crate::config::AgentPanelSortConfig::Folders;
    let frame = state.compose(106, 40).expect("frame");

    let folder_header = state
        .hits
        .folders
        .agent_folder_headers
        .iter()
        .find(|header| header.folder_id == "f_1")
        .map(|header| header.rect)
        .expect("agents panel folder header");
    assert!(row_text(&frame, folder_header.y).contains("▾ work"));
    let space_headers = state
        .hits
        .folders
        .agent_space_headers
        .iter()
        .map(|hit| hit.workspace_id.clone())
        .collect::<Vec<_>>();
    assert_eq!(space_headers, ["ws_a", "ws_b", "ws_d"]);
    assert_eq!(state.hits.agents.len(), 3);

    let bravo = state
        .hits
        .folders
        .agent_space_headers
        .iter()
        .find(|hit| hit.workspace_id == "ws_b")
        .map(|hit| hit.rect)
        .expect("bravo header");
    click(
        &mut state,
        MouseEventKind::Down(MouseButton::Left),
        bravo.x + 8,
        bravo.y,
    );
    assert!(state.folder_collapse().agent_spaces.contains("ws_b"));
    state.compose(106, 40).expect("frame");
    assert_eq!(state.hits.agents.len(), 2);
    assert_eq!(
        folders::expanded_workspace_ranks(state.snapshot.as_deref().expect("snapshot")).len(),
        4
    );
    let ordered = agent_sidebar::ordered_agent_pane_ids(
        state.snapshot.as_deref().expect("snapshot"),
        crate::config::AgentPanelSortConfig::Folders,
    );
    assert_eq!(ordered, ["pane_ws_a", "pane_ws_b", "pane_ws_d"]);

    click(
        &mut state,
        MouseEventKind::Down(MouseButton::Left),
        folder_header.x + 8,
        folder_header.y,
    );
    assert!(state.collapsed_folders().contains("f_1"));
    state.compose(106, 40).expect("frame");
    assert!(!state
        .hits
        .folders
        .agent_space_headers
        .iter()
        .any(|hit| hit.workspace_id == "ws_b"));
    assert!(!state
        .hits
        .workspaces
        .iter()
        .any(|hit| hit.workspace_id == "ws_b"));
}

#[test]
fn collapse_state_round_trips_through_client_preferences_and_prunes_dangling_ids() {
    let directory = std::env::temp_dir().join(format!(
        "herdr-folders-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default()
    ));
    let path = directory.join("prefs.json");
    let mut config = ClientShellConfig::from_config(&Config::default());
    config.preferences_path = Some(path.clone());
    let mut state = ClientShellState::new(config);
    state.set_snapshot(Box::new(foldered_snapshot()));
    state.set_pane_surface(surface());
    let mut outcome = ClientShellInput::default();
    state.toggle_folder_collapse("f_1", &mut outcome);
    state.toggle_agent_space_collapse("ws_b", &mut outcome);

    let stored = preferences::load(&path).expect("stored preferences");
    let local = stored
        .folder_collapse
        .get("local")
        .expect("local endpoint entry");
    assert_eq!(local.collapsed_folders, ["f_1"]);
    assert_eq!(local.collapsed_agent_spaces, ["ws_b"]);
    assert!(
        stored.collapsed_folders.is_empty() && stored.collapsed_agent_spaces.is_empty(),
        "the legacy flat fields are no longer written"
    );

    let mut config = ClientShellConfig::from_config(&Config::default());
    config.preferences = stored;
    let mut restored = ClientShellState::new(config);
    assert!(restored.collapsed_folders().contains("f_1"));
    assert!(restored.folder_collapse().agent_spaces.contains("ws_b"));

    restored.set_snapshot(Box::new(snapshot()));
    assert!(restored.collapsed_folders().is_empty());
    assert!(restored.folder_collapse().agent_spaces.is_empty());
    let _ = std::fs::remove_dir_all(directory);
}

#[test]
fn legacy_flat_collapse_preferences_load_as_the_local_endpoints() {
    let mut config = ClientShellConfig::from_config(&Config::default());
    config.preferences.collapsed_folders = vec!["f_1".into()];
    config.preferences.collapsed_agent_spaces = vec!["ws_b".into()];
    let state = ClientShellState::new(config);

    assert!(state.collapsed_folders().contains("f_1"));
    assert!(state.folder_collapse().agent_spaces.contains("ws_b"));
    assert_eq!(state.folder_collapse.len(), 1);
    assert!(state.folder_collapse.contains_key("local"));
}

#[test]
fn collapse_state_is_scoped_to_its_endpoint_across_activation() {
    use crate::client::endpoint::{
        ClientEndpointId, ClientEndpointStatus, ProfileId, SavedSshEndpoint,
    };

    let directory = std::env::temp_dir().join(format!(
        "herdr-folders-endpoints-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default()
    ));
    let path = directory.join("prefs.json");
    let mut config = ClientShellConfig::from_config(&Config::default());
    config.preferences_path = Some(path.clone());
    let mut state = ClientShellState::new(config);
    let profile = SavedSshEndpoint {
        id: ProfileId::parse("0123456789abcdef0123456789abcdef").expect("profile id"),
        label: "Build".into(),
        target: "dev@build.example".into(),
        session: "agents".into(),
        enabled: true,
    };
    let remote_id = ClientEndpointId::Ssh(profile.id.clone());
    state.set_endpoint_catalog(&[profile]);
    state.set_endpoint_status(&remote_id, ClientEndpointStatus::Online);
    state.set_snapshot(Box::new(foldered_snapshot()));
    state.set_pane_surface(surface());
    let mut remote_snapshot = foldered_snapshot();
    remote_snapshot.boot_id = "remote-boot".into();
    remote_snapshot.folders[0].members = vec!["ws_c".into()];
    remote_snapshot
        .workspaces
        .retain(|ws| ws.workspace_id != "ws_b");
    state.set_endpoint_snapshot(&remote_id, Box::new(remote_snapshot));

    let mut outcome = ClientShellInput::default();
    state.toggle_folder_collapse("f_1", &mut outcome);
    state.toggle_agent_space_collapse("ws_b", &mut outcome);

    assert!(state.activate_endpoint_projection(&remote_id));
    assert!(
        state.collapsed_folders().is_empty() && state.folder_collapse().agent_spaces.is_empty(),
        "a matching folder id on another server must not start collapsed"
    );
    let stored = preferences::load(&path).expect("stored preferences");
    let local = stored
        .folder_collapse
        .get("local")
        .expect("local entry survives the switch");
    assert_eq!(local.collapsed_folders, ["f_1"]);
    assert_eq!(local.collapsed_agent_spaces, ["ws_b"]);
    assert!(
        !stored
            .folder_collapse
            .contains_key(&remote_id.storage_key()),
        "nothing was collapsed on the remote"
    );

    state.toggle_folder_collapse("f_1", &mut outcome);
    let stored = preferences::load(&path).expect("stored preferences");
    assert_eq!(
        stored
            .folder_collapse
            .get(&remote_id.storage_key())
            .map(|entry| entry.collapsed_folders.clone()),
        Some(vec!["f_1".to_string()])
    );

    assert!(state.activate_endpoint_projection(&ClientEndpointId::Local));
    assert!(state.collapsed_folders().contains("f_1"));
    assert!(state.folder_collapse().agent_spaces.contains("ws_b"));
    let _ = std::fs::remove_dir_all(directory);
}

#[test]
fn multi_machine_sidebar_keeps_local_folders() {
    use crate::client::endpoint::{ProfileId, SavedSshEndpoint};

    let mut state = state_with(foldered_snapshot());
    state.set_endpoint_catalog(&[SavedSshEndpoint {
        id: ProfileId::parse("0123456789abcdef0123456789abcdef").expect("profile id"),
        label: "Build".into(),
        target: "dev@build.example".into(),
        session: "agents".into(),
        enabled: false,
    }]);
    assert!(state.multi_endpoint_active());

    let frame = state.compose(106, 30).expect("frame");
    let header = state
        .hits
        .folders
        .headers
        .iter()
        .find(|header| header.folder_id == "f_1")
        .expect("local folder header hit");
    assert!(header.endpoint_id.is_local());
    let header = header.rect;
    assert!(row_text(&frame, header.y).contains("▾ work"));
    let member = state
        .hits
        .workspaces
        .iter()
        .find(|hit| hit.workspace_id == "ws_b")
        .expect("member hit");
    assert!(member.foldered);
    let loose = state
        .hits
        .workspaces
        .iter()
        .find(|hit| hit.workspace_id == "ws_a")
        .expect("loose hit");
    assert!(!loose.foldered);
    assert_eq!(
        row_text(&frame, loose.rect.y).find("alpha").map(|x| x + 2),
        row_text(&frame, member.rect.y).find("bravo")
    );

    click(
        &mut state,
        MouseEventKind::Down(MouseButton::Left),
        header.x + 6,
        header.y,
    );
    click(
        &mut state,
        MouseEventKind::Up(MouseButton::Left),
        header.x + 6,
        header.y,
    );
    assert!(state.collapsed_folders().contains("f_1"));
    let frame = state.compose(106, 30).expect("collapsed frame");
    assert!(row_text(&frame, header.y).contains("▸ work"));
    assert!(!state
        .hits
        .workspaces
        .iter()
        .any(|hit| hit.workspace_id == "ws_b"));
    assert!(state
        .hits
        .workspaces
        .iter()
        .any(|hit| hit.workspace_id == "ws_d"));

    click(
        &mut state,
        MouseEventKind::Down(MouseButton::Right),
        header.x + 6,
        header.y,
    );
    assert!(matches!(
        state.overlay.as_ref(),
        Some(ClientShellOverlay::ContextMenu(ClientContextMenuOverlay {
            target: ClientContextMenuTarget::Folder { folder_id },
            ..
        })) if folder_id == "f_1"
    ));
}

#[test]
fn multi_machine_sidebar_renders_and_collapses_remote_folders_in_place() {
    use crate::client::endpoint::{
        ClientEndpointId, ClientEndpointStatus, ProfileId, SavedSshEndpoint,
    };

    let mut state = state_with(foldered_snapshot());
    let profile = SavedSshEndpoint {
        id: ProfileId::parse("0123456789abcdef0123456789abcdef").expect("profile id"),
        label: "Build".into(),
        target: "dev@build.example".into(),
        session: "agents".into(),
        enabled: true,
    };
    let remote_id = ClientEndpointId::Ssh(profile.id.clone());
    state.set_endpoint_catalog(&[profile]);
    state.set_endpoint_status(&remote_id, ClientEndpointStatus::Online);
    let mut remote_snapshot = foldered_snapshot();
    remote_snapshot.boot_id = "remote-boot".into();
    remote_snapshot.folders[0].name = "remote work".into();
    state.set_endpoint_snapshot(&remote_id, Box::new(remote_snapshot));

    let frame = state.compose(106, 40).expect("frame");
    let headers = state
        .hits
        .folders
        .headers
        .iter()
        .map(|header| (header.endpoint_id.clone(), header.rect))
        .collect::<Vec<_>>();
    assert_eq!(headers.len(), 2, "one header per endpoint section");
    let remote_header = headers
        .iter()
        .find(|(endpoint_id, _)| *endpoint_id == remote_id)
        .map(|(_, rect)| *rect)
        .expect("remote folder header");
    assert!(row_text(&frame, remote_header.y).contains("▾ remote work"));

    click(
        &mut state,
        MouseEventKind::Down(MouseButton::Left),
        remote_header.x + 6,
        remote_header.y,
    );
    click(
        &mut state,
        MouseEventKind::Up(MouseButton::Left),
        remote_header.x + 6,
        remote_header.y,
    );
    assert!(
        !state.collapsed_folders().contains("f_1"),
        "the active endpoint's matching folder id stays expanded"
    );
    assert!(
        folders::FolderCollapseState::of(&state.folder_collapse, &remote_id)
            .folders
            .contains("f_1")
    );
    let frame = state.compose(106, 40).expect("frame");
    assert!(row_text(&frame, remote_header.y).contains("▸ remote work"));
    let remote_members = state
        .hits
        .workspaces
        .iter()
        .filter(|hit| hit.endpoint_id == remote_id && hit.foldered)
        .count();
    assert_eq!(remote_members, 0);
    let local_members = state
        .hits
        .workspaces
        .iter()
        .filter(|hit| hit.endpoint_id.is_local() && hit.foldered)
        .count();
    assert_eq!(local_members, 2);

    click(
        &mut state,
        MouseEventKind::Down(MouseButton::Right),
        remote_header.x + 6,
        remote_header.y,
    );
    assert!(!matches!(
        state.overlay.as_ref(),
        Some(ClientShellOverlay::ContextMenu(ClientContextMenuOverlay {
            target: ClientContextMenuTarget::Folder { .. },
            ..
        }))
    ));
}

fn agent(workspace_id: &str, seq: u64, focused: bool) -> ClientShellAgent {
    ClientShellAgent {
        pane_id: format!("pane_{workspace_id}"),
        workspace_id: workspace_id.into(),
        tab_id: format!("{workspace_id}:tab"),
        agent: Some("codex".into()),
        display_agent: None,
        name: None,
        title: None,
        terminal_title: None,
        terminal_title_stripped: None,
        agent_status: AgentStatus::Working,
        state_change_seq: seq,
        state_labels: Vec::new(),
        tokens: Vec::new(),
        focused,
    }
}

#[test]
fn multi_machine_agents_panel_renders_folder_view_per_machine() {
    use crate::client::endpoint::{
        ClientEndpointId, ClientEndpointStatus, ProfileId, SavedSshEndpoint,
    };

    let mut local_snapshot = foldered_snapshot();
    local_snapshot.agents.push(agent("ws_a", 0, true));
    local_snapshot.agents.push(agent("ws_b", 1, false));
    let mut state = state_with(local_snapshot);
    state.config.agent_panel_sort = crate::config::AgentPanelSortConfig::Folders;
    let profile = SavedSshEndpoint {
        id: ProfileId::parse("0123456789abcdef0123456789abcdef").expect("profile id"),
        label: "Build".into(),
        target: "dev@build.example".into(),
        session: "agents".into(),
        enabled: true,
    };
    let remote_id = ClientEndpointId::Ssh(profile.id.clone());
    state.set_endpoint_catalog(&[profile]);
    state.set_endpoint_status(&remote_id, ClientEndpointStatus::Online);
    let mut remote_snapshot = foldered_snapshot();
    remote_snapshot.boot_id = "remote-boot".into();
    remote_snapshot.folders[0].name = "remote work".into();
    // The remote's own focus must not paint as this client's active row.
    remote_snapshot.agents.push(agent("ws_c", 0, true));
    state.set_endpoint_snapshot(&remote_id, Box::new(remote_snapshot));
    assert!(state.multi_endpoint_active());

    let frame = state.compose(106, 50).expect("frame");

    assert!(
        state.hits.agents.is_empty(),
        "multi-machine agents register endpoint-qualified hits"
    );
    let agent_hits = state
        .hits
        .endpoint_agents
        .iter()
        .map(|(_, endpoint_id, pane_id)| (endpoint_id.clone(), pane_id.clone()))
        .collect::<Vec<_>>();
    assert_eq!(
        agent_hits,
        [
            (ClientEndpointId::Local, "pane_ws_a".to_string()),
            (ClientEndpointId::Local, "pane_ws_b".to_string()),
            (remote_id.clone(), "pane_ws_c".to_string()),
        ]
    );

    let folder_headers = state
        .hits
        .folders
        .agent_folder_headers
        .iter()
        .map(|header| (header.endpoint_id.clone(), header.rect))
        .collect::<Vec<_>>();
    assert_eq!(folder_headers.len(), 2, "one folder header per machine");
    let (local_folder, remote_folder) = (folder_headers[0].1, folder_headers[1].1);
    assert!(folder_headers[0].0.is_local());
    assert_eq!(folder_headers[1].0, remote_id);
    assert!(row_text(&frame, local_folder.y).contains("▾ work"));
    assert!(row_text(&frame, remote_folder.y).contains("▾ remote work"));

    let space_headers = state
        .hits
        .folders
        .agent_space_headers
        .iter()
        .map(|hit| (hit.endpoint_id.clone(), hit.workspace_id.clone()))
        .collect::<Vec<_>>();
    assert_eq!(
        space_headers,
        [
            (ClientEndpointId::Local, "ws_a".to_string()),
            (ClientEndpointId::Local, "ws_b".to_string()),
            (remote_id.clone(), "ws_c".to_string()),
        ]
    );

    // Each machine's section starts one row below its machine header: the
    // local section with the loose `alpha` space, the remote with its folder.
    let local_first_row = state
        .hits
        .folders
        .agent_space_headers
        .iter()
        .find(|hit| hit.endpoint_id.is_local() && hit.workspace_id == "ws_a")
        .map(|hit| hit.rect.y)
        .expect("local alpha header");
    let local_machine_row = local_first_row - 1;
    let remote_machine_row = remote_folder.y - 1;
    assert!(
        row_text(&frame, local_machine_row).contains("Local"),
        "local section is headed by its machine: {:?}",
        row_text(&frame, local_machine_row)
    );
    assert!(
        row_text(&frame, remote_machine_row).contains("Build"),
        "remote section is headed by its machine: {:?}",
        row_text(&frame, remote_machine_row)
    );

    let buffer = frame.to_ratatui_buffer().expect("frame should reconstruct");
    let (remote_agent_rect, _, _) = state.hits.endpoint_agents[2];
    assert_ne!(
        buffer[(remote_agent_rect.x, remote_agent_rect.y)].bg,
        state.config.palette.active_row_bg,
        "an inactive machine's focused agent is not highlighted"
    );
    let (local_agent_rect, _, _) = state.hits.endpoint_agents[0];
    assert_eq!(
        buffer[(local_agent_rect.x, local_agent_rect.y)].bg,
        state.config.palette.active_row_bg,
        "the active machine's focused agent is highlighted"
    );

    // Collapsing the remote folder touches only the remote's collapse state.
    click(
        &mut state,
        MouseEventKind::Down(MouseButton::Left),
        remote_folder.x + 6,
        remote_folder.y,
    );
    assert!(!state.collapsed_folders().contains("f_1"));
    assert!(
        folders::FolderCollapseState::of(&state.folder_collapse, &remote_id)
            .folders
            .contains("f_1")
    );
    let frame = state.compose(106, 50).expect("frame");
    assert!(row_text(&frame, remote_folder.y).contains("▸ remote work"));
    assert!(!state
        .hits
        .endpoint_agents
        .iter()
        .any(|(_, endpoint_id, _)| *endpoint_id == remote_id));
    assert_eq!(
        state
            .hits
            .endpoint_agents
            .iter()
            .filter(|(_, endpoint_id, _)| endpoint_id.is_local())
            .count(),
        2
    );

    // Collapsing a remote space header is likewise scoped to the remote.
    let remote_snapshot_expanded = {
        let mut outcome = ClientShellInput::default();
        state.toggle_folder_collapse_for(&remote_id, "f_1", &mut outcome);
        state.compose(106, 50).expect("frame")
    };
    let remote_space = state
        .hits
        .folders
        .agent_space_headers
        .iter()
        .find(|hit| hit.endpoint_id == remote_id && hit.workspace_id == "ws_c")
        .map(|hit| hit.rect)
        .expect("remote space header");
    assert!(row_text(&remote_snapshot_expanded, remote_space.y).contains("▾ charlie"));
    click(
        &mut state,
        MouseEventKind::Down(MouseButton::Left),
        remote_space.x + 8,
        remote_space.y,
    );
    assert!(!state.folder_collapse().agent_spaces.contains("ws_c"));
    assert!(
        folders::FolderCollapseState::of(&state.folder_collapse, &remote_id)
            .agent_spaces
            .contains("ws_c")
    );
    state.compose(106, 50).expect("frame");
    assert!(!state
        .hits
        .endpoint_agents
        .iter()
        .any(|(_, _, pane_id)| pane_id == "pane_ws_c"));
}

#[test]
fn collapsing_a_family_parent_in_the_agents_panel_hides_its_worktree_children() {
    use crate::protocol::ClientShellWorktree;

    let mut snapshot = foldered_snapshot();
    snapshot.workspaces[1].worktree = Some(ClientShellWorktree {
        key: "repo".into(),
        label: "repo".into(),
        is_linked_worktree: false,
    });
    snapshot.workspaces[2].worktree = Some(ClientShellWorktree {
        key: "repo".into(),
        label: "repo".into(),
        is_linked_worktree: true,
    });
    snapshot.workspaces[2].focused = true;
    snapshot.workspaces[0].focused = false;
    snapshot.focused_workspace_id = Some("ws_c".into());
    for (index, workspace_id) in ["ws_b", "ws_c"].into_iter().enumerate() {
        snapshot.agents.push(ClientShellAgent {
            pane_id: format!("pane_{workspace_id}"),
            workspace_id: workspace_id.into(),
            tab_id: format!("{workspace_id}:tab"),
            agent: Some("codex".into()),
            display_agent: None,
            name: None,
            title: None,
            terminal_title: None,
            terminal_title_stripped: None,
            agent_status: AgentStatus::Working,
            state_change_seq: index as u64,
            state_labels: Vec::new(),
            tokens: Vec::new(),
            focused: workspace_id == "ws_c",
        });
    }
    let mut state = state_with(snapshot);
    state.config.agent_panel_sort = crate::config::AgentPanelSortConfig::Folders;
    state.compose(106, 40).expect("frame");
    let headers = |state: &ClientShellState| {
        state
            .hits
            .folders
            .agent_space_headers
            .iter()
            .map(|hit| hit.workspace_id.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(headers(&state), ["ws_b", "ws_c"]);
    assert_eq!(state.hits.agents.len(), 2);

    let mut outcome = ClientShellInput::default();
    state.toggle_agent_space_collapse("ws_b", &mut outcome);
    let frame = state.compose(106, 40).expect("collapsed frame");

    assert_eq!(
        headers(&state),
        ["ws_b"],
        "the child header is hidden with its parent"
    );
    assert!(state.hits.agents.is_empty());
    let parent = state.hits.folders.agent_space_headers[0].rect;
    assert!(row_text(&frame, parent.y).contains("▸ bravo"));
    let buffer = frame.to_ratatui_buffer().expect("frame should reconstruct");
    assert_eq!(
        buffer[(parent.x, parent.y)].bg,
        state.config.palette.active_row_bg,
        "the parent header indicates the hidden focused child"
    );

    state.toggle_agent_space_collapse("ws_b", &mut outcome);
    state.compose(106, 40).expect("expanded frame");
    assert_eq!(headers(&state), ["ws_b", "ws_c"]);
    assert_eq!(state.hits.agents.len(), 2);
}
