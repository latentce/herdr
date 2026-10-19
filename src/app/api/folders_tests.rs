use serde_json::{json, Value};

use crate::app::App;
use crate::workspace::{Workspace, WorktreeSpaceMembership};

fn test_app(names: &[&str]) -> (App, Vec<String>) {
    let (_api_tx, api_rx) = tokio::sync::mpsc::unbounded_channel();
    let mut app = App::new(
        &crate::config::Config::default(),
        crate::app::AppPolicy::TEST,
        None,
        api_rx,
        crate::api::EventHub::default(),
    );
    app.state.workspaces = names.iter().map(|name| Workspace::test_new(name)).collect();
    app.state.active = Some(0);
    app.state.selected = 0;
    app.state.ensure_test_terminals();
    let ids = app
        .state
        .workspaces
        .iter()
        .map(|workspace| workspace.id.clone())
        .collect();
    (app, ids)
}

fn call(app: &mut App, method: &str, params: Value) -> Value {
    let request = serde_json::from_value(json!({
        "id": "test",
        "method": method,
        "params": params,
    }))
    .expect("request");
    serde_json::from_str(&app.handle_api_request(request)).expect("response")
}

fn ok(app: &mut App, method: &str, params: Value) -> Value {
    let response = call(app, method, params);
    assert!(
        response.get("error").is_none(),
        "{method} failed: {response}"
    );
    response["result"].clone()
}

fn error_code(app: &mut App, method: &str, params: Value) -> String {
    let response = call(app, method, params);
    response["error"]["code"]
        .as_str()
        .unwrap_or_else(|| panic!("{method} should fail: {response}"))
        .to_owned()
}

fn flat(app: &App) -> Vec<String> {
    app.state
        .workspaces
        .iter()
        .map(|workspace| workspace.id.clone())
        .collect()
}

fn assert_flat(app: &App, expected: &[&String]) {
    assert_eq!(flat(app).iter().collect::<Vec<_>>(), expected);
}

fn order(app: &mut App) -> Vec<String> {
    serde_json::from_value(ok(app, "folder.list", json!({}))["order"].clone()).expect("order")
}

fn event_names(app: &App) -> Vec<String> {
    app.event_hub
        .events_after(0)
        .into_iter()
        .map(|(_, event)| event.event.dot_name().to_owned())
        .collect()
}

fn membership(key: &str, linked: bool) -> WorktreeSpaceMembership {
    WorktreeSpaceMembership {
        key: key.into(),
        label: key.into(),
        repo_root: "/repo".into(),
        checkout_path: "/repo".into(),
        is_linked_worktree: linked,
    }
}

#[test]
fn folder_lifecycle_through_the_api() {
    let (mut app, ids) = test_app(&["a", "b", "c"]);
    let created = ok(
        &mut app,
        "folder.create",
        json!({"name": "  backend  ", "workspace_ids": [ids[1]]}),
    );
    assert_eq!(created["type"], "folder_info");
    assert_eq!(created["folder"]["folder_id"], "f1");
    assert_eq!(created["folder"]["name"], "backend");
    assert_eq!(created["folder"]["position"], 1);
    assert_eq!(created["folder"]["workspace_ids"], json!([ids[1]]));

    let workspace = ok(&mut app, "workspace.get", json!({"workspace_id": ids[1]}));
    assert_eq!(workspace["workspace"]["folder_id"], "f1");
    let loose = ok(&mut app, "workspace.get", json!({"workspace_id": ids[0]}));
    assert!(loose["workspace"].get("folder_id").is_none());

    let snapshot = ok(&mut app, "session.snapshot", json!({}));
    assert_eq!(snapshot["snapshot"]["folders"][0]["folder_id"], "f1");

    let renamed = ok(
        &mut app,
        "folder.rename",
        json!({"folder_id": "f1", "name": "api"}),
    );
    assert_eq!(renamed["folder"]["name"], "api");
    assert_eq!(
        ok(&mut app, "folder.get", json!({"folder_id": "f1"}))["folder"]["name"],
        "api"
    );

    ok(&mut app, "folder.delete", json!({"folder_id": "f1"}));
    assert_eq!(order(&mut app), ids);
    let created = ok(&mut app, "folder.create", json!({"name": "again"}));
    assert_eq!(created["folder"]["folder_id"], "f2", "ids are never reused");
    assert_eq!(created["folder"]["position"], 3);

    let events = event_names(&app);
    for expected in [
        "folder.created",
        "workspace.folder_changed",
        "folder.renamed",
        "folder.deleted",
    ] {
        assert!(
            events.iter().any(|event| event == expected),
            "{expected} in {events:?}"
        );
    }
}

#[test]
fn no_op_folder_operations_emit_and_bump_nothing() {
    let (mut app, ids) = test_app(&["a", "b", "c"]);
    ok(
        &mut app,
        "folder.create",
        json!({"name": "backend", "workspace_ids": [ids[1], ids[2]]}),
    );
    let revision = app.state.folders.revision();
    let events = event_names(&app).len();
    ok(
        &mut app,
        "folder.rename",
        json!({"folder_id": "f1", "name": " backend "}),
    );
    ok(
        &mut app,
        "folder.add_workspace",
        json!({"folder_id": "f1", "workspace_id": ids[2]}),
    );
    ok(
        &mut app,
        "folder.move",
        json!({"folder_id": "f1", "after": ids[0]}),
    );
    ok(
        &mut app,
        "folder.remove_workspace",
        json!({"workspace_id": ids[0]}),
    );
    assert_eq!(app.state.folders.revision(), revision);
    assert_eq!(event_names(&app).len(), events, "{:?}", event_names(&app));
}

#[test]
fn folder_operations_reject_bad_input_and_change_nothing() {
    let (mut app, ids) = test_app(&["a", "b", "c", "d"]);
    ok(
        &mut app,
        "folder.create",
        json!({"name": "x", "workspace_ids": [ids[0]]}),
    );
    ok(
        &mut app,
        "folder.create",
        json!({"name": "y", "workspace_ids": [ids[3]]}),
    );
    let before = (flat(&app), order(&mut app));

    assert_eq!(
        error_code(&mut app, "folder.create", json!({"name": "   "})),
        "folder_name_invalid"
    );
    assert_eq!(
        error_code(
            &mut app,
            "folder.rename",
            json!({"folder_id": "f1", "name": ""})
        ),
        "folder_name_invalid"
    );
    assert_eq!(
        error_code(
            &mut app,
            "folder.create",
            json!({"name": "z", "workspace_ids": [ids[1], "w999"]})
        ),
        "workspace_not_found"
    );
    assert_eq!(
        error_code(&mut app, "folder.get", json!({"folder_id": "f9"})),
        "folder_not_found"
    );
    assert_eq!(
        error_code(
            &mut app,
            "folder.add_workspace",
            json!({"folder_id": "f1", "workspace_id": ids[1], "before": ids[3]})
        ),
        "folder_position_invalid"
    );
    assert_eq!(
        error_code(
            &mut app,
            "folder.move",
            json!({"folder_id": "f1", "before": ids[3]})
        ),
        "folder_position_invalid"
    );
    assert_eq!(
        error_code(
            &mut app,
            "folder.remove_workspace",
            json!({"workspace_id": ids[1], "before": "f1", "after": "f2"})
        ),
        "folder_position_invalid"
    );
    assert_eq!(before, (flat(&app), order(&mut app)));
    assert_eq!(
        ok(&mut app, "folder.list", json!({}))["folders"]
            .as_array()
            .map(Vec::len),
        Some(2)
    );
}

#[test]
fn placement_defaults_follow_the_folder_rules() {
    let (mut app, ids) = test_app(&["a", "b", "c", "d"]);
    ok(
        &mut app,
        "folder.create",
        json!({"name": "x", "workspace_ids": [ids[1]]}),
    );
    assert_eq!(order(&mut app), [&ids[0], "f1", &ids[2], &ids[3]]);
    ok(
        &mut app,
        "folder.add_workspace",
        json!({"folder_id": "f1", "workspace_id": ids[3]}),
    );
    assert_flat(&app, &[&ids[0], &ids[1], &ids[3], &ids[2]]);
    ok(
        &mut app,
        "folder.remove_workspace",
        json!({"workspace_id": ids[1]}),
    );
    assert_eq!(order(&mut app), [&ids[0], "f1", &ids[1], &ids[2]]);
    ok(
        &mut app,
        "folder.create",
        json!({"name": "y", "workspace_ids": [ids[3]]}),
    );
    assert_eq!(order(&mut app), [&ids[0], "f1", "f2", &ids[1], &ids[2]]);
    let before = flat(&app);
    ok(
        &mut app,
        "folder.remove_workspace",
        json!({"workspace_id": ids[0]}),
    );
    assert_eq!(flat(&app), before);
    ok(
        &mut app,
        "folder.remove_workspace",
        json!({"workspace_id": ids[0], "after": "f1"}),
    );
    assert_eq!(order(&mut app), ["f1", &ids[0], "f2", &ids[1], &ids[2]]);
    ok(
        &mut app,
        "folder.move",
        json!({"folder_id": "f2", "before": "f1"}),
    );
    assert_eq!(order(&mut app), ["f2", "f1", &ids[0], &ids[1], &ids[2]]);
    assert!(event_names(&app)
        .iter()
        .any(|event| event == "folder.moved"));
}

#[test]
fn legacy_moves_keep_membership_and_clamp_to_the_nearest_edge() {
    let (mut app, ids) = test_app(&["a", "b", "c", "d", "e"]);
    ok(
        &mut app,
        "folder.create",
        json!({"name": "x", "workspace_ids": [ids[1], ids[2], ids[3]]}),
    );
    ok(
        &mut app,
        "workspace.move",
        json!({"workspace_id": ids[2], "insert_index": 0}),
    );
    assert_flat(&app, &[&ids[0], &ids[2], &ids[1], &ids[3], &ids[4]]);
    ok(
        &mut app,
        "workspace.move",
        json!({"workspace_id": ids[4], "insert_index": 2}),
    );
    assert_flat(&app, &[&ids[0], &ids[4], &ids[2], &ids[1], &ids[3]]);
    ok(
        &mut app,
        "workspace.move_block",
        json!({"workspace_ids": [ids[1]], "before_workspace_id": ids[0]}),
    );
    assert_flat(&app, &[&ids[0], &ids[4], &ids[1], &ids[2], &ids[3]]);
    for id in [&ids[1], &ids[2], &ids[3]] {
        assert_eq!(app.state.folders.folder_of(id), Some(1));
    }
    assert_eq!(app.state.folders.folder_of(&ids[4]), None);
}

#[test]
fn worktree_children_join_their_parents_folder() {
    let (mut app, ids) = test_app(&["a", "parent", "c"]);
    app.state.workspaces[1].worktree_space = Some(membership("repo", false));
    ok(
        &mut app,
        "folder.create",
        json!({"name": "x", "workspace_ids": [ids[1]]}),
    );
    let mut child = Workspace::test_new("child");
    child.worktree_space = Some(membership("repo", true));
    let child_id = child.id.clone();
    app.state.workspaces.push(child);
    app.state.ensure_test_terminals();
    assert!(app.sync_folders());
    assert_eq!(app.state.folders.folder_of(&child_id), Some(1));
    assert_flat(&app, &[&ids[0], &ids[1], &child_id, &ids[2]]);
    assert!(event_names(&app)
        .iter()
        .any(|event| event == "workspace.folder_changed"));
    ok(
        &mut app,
        "folder.remove_workspace",
        json!({"workspace_id": child_id}),
    );
    assert_eq!(app.state.folders.folder_of(&ids[1]), None);
    assert!(!app.sync_folders());
}

#[test]
fn closed_members_leave_and_empty_folders_stay() {
    let (mut app, ids) = test_app(&["a", "b"]);
    ok(
        &mut app,
        "folder.create",
        json!({"name": "x", "workspace_ids": [ids[1]]}),
    );
    app.state.workspaces.remove(1);
    app.state.active = Some(0);
    app.sync_folders();
    let folders = ok(&mut app, "folder.list", json!({}));
    assert_eq!(folders["folders"][0]["workspace_ids"], json!([]));
    assert_eq!(order(&mut app), [&ids[0], "f1"]);
}

#[test]
fn folders_persist_across_apps() {
    let dir = std::env::temp_dir().join(format!(
        "herdr-app-folders-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos())
    ));
    let path = dir.join(crate::folders::store::FOLDER_FILE_NAME);
    let (mut app, ids) = test_app(&["a", "b", "c"]);
    app.state.folders.storage.path = Some(path.clone());
    ok(
        &mut app,
        "folder.create",
        json!({"name": "x", "workspace_ids": [ids[2]]}),
    );
    ok(
        &mut app,
        "folder.create",
        json!({"name": "empty", "before": ids[0]}),
    );
    app.flush_folder_save();
    assert!(path.exists());

    let (mut restored, _) = test_app(&[]);
    restored.state.workspaces = std::mem::take(&mut app.state.workspaces);
    restored.state.terminals = std::mem::take(&mut app.state.terminals);
    restored.state.active = Some(0);
    restored.state.folders.storage.path = Some(path.clone());
    assert_eq!(order(&mut restored), ["f2", &ids[0], &ids[1], "f1"]);
    assert_eq!(restored.state.folders.folder_of(&ids[2]), Some(1));
    let _ = std::fs::remove_dir_all(dir);
}
