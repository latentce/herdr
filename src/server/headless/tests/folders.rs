//! Public folder requests project to attached clients like the workspace
//! requests they build on.

use super::*;
use crate::api::schema::{
    FolderCreateParams, FolderCreateWorkspaceParams, Method, WorkspaceCreateParams,
};

/// The newest client snapshot among the queued control messages.
fn latest_snapshot(
    receiver: &std::sync::mpsc::Receiver<Vec<u8>>,
) -> Option<Box<protocol::ClientShellSnapshot>> {
    let mut latest = None;
    while let Ok(bytes) = receiver.recv_timeout(Duration::from_millis(200)) {
        if let ServerMessage::EndpointControl { kind, data } = read_server_message(bytes) {
            if kind == protocol::endpoint::ENDPOINT_SNAPSHOT_KIND {
                latest = Some(serde_json::from_str(&data).expect("decode snapshot"));
            }
        }
    }
    latest
}

fn public_request(server: &mut HeadlessServer, method: Method) {
    let (respond_to, response_rx) = std::sync::mpsc::channel();
    server.handle_api_request_with_shutdown_check(api::ApiRequestMessage {
        request: api::schema::Request {
            id: "test.folders".into(),
            method,
        },
        respond_to,
        response_write_complete: None,
    });
    let response = response_rx.recv().expect("response");
    assert!(!response.contains("\"error\""), "{response}");
}

#[tokio::test]
async fn public_folder_workspace_create_with_focus_moves_every_client() {
    let mut server = test_headless_server();
    server.app.state.workspaces = vec![crate::workspace::Workspace::test_new("first")];
    server.app.state.ensure_test_terminals();
    server.app.state.active = Some(0);
    server.app.state.selected = 0;
    server.app.state.mode = crate::app::Mode::Terminal;

    let (writer, control_rx, _render_rx) = test_client_writer();
    assert!(
        server.handle_server_event(ServerEvent::ClientShellConnected {
            surface_reuse: false,
            surface_delta: false,
            surface_scroll: false,
            client_id: 9,
            surface_cols: 80,
            surface_rows: 23,
            cell_width_px: 0,
            cell_height_px: 0,
            pixel_mouse: false,
            direct_graphics: false,
            endpoint_keybindings: false,
            mouse_capture: false,
            surface_active: true,
            writer,
        })
    );
    latest_snapshot(&control_rx).expect("initial snapshot");

    public_request(
        &mut server,
        Method::FolderCreate(FolderCreateParams {
            name: "backend".into(),
            workspace_ids: Vec::new(),
            before: None,
            after: None,
        }),
    );
    public_request(
        &mut server,
        Method::FolderCreateWorkspace(FolderCreateWorkspaceParams {
            folder_id: "f1".into(),
            workspace: WorkspaceCreateParams {
                source_workspace_id: None,
                cwd: None,
                focus: true,
                label: Some("inside".into()),
                env: Default::default(),
            },
        }),
    );
    let created = server
        .app
        .session_snapshot()
        .workspaces
        .last()
        .expect("created workspace")
        .workspace_id
        .clone();
    server.render_and_stream();

    let snapshot = latest_snapshot(&control_rx).expect("replacement snapshot");
    assert_eq!(
        snapshot.focused_workspace_id.as_deref(),
        Some(created.as_str())
    );
    shutdown_test_runtimes(&mut server);
}
