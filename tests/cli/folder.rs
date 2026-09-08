use super::harness::*;

#[test]
fn folder_management_commands_work() {
    let base = unique_test_dir();
    let config_home = base.join("config");
    let runtime_dir = base.join("runtime");
    let socket_path = runtime_dir.join("herdr.sock");

    let herdr = spawn_herdr(&config_home, &runtime_dir, &socket_path);
    wait_for_socket(&socket_path, Duration::from_secs(5));

    let workspace = run_cli_json(
        &socket_path,
        &["workspace", "create", "--cwd", base.to_str().unwrap()],
    );
    let workspace_id = workspace["result"]["workspace"]["workspace_id"]
        .as_str()
        .unwrap()
        .to_string();

    let created = run_cli_json(&socket_path, &["folder", "create", "--name", "projects"]);
    assert_eq!(created["result"]["type"], "folder_created");
    let folder_id = created["result"]["folder_id"].as_str().unwrap().to_string();

    let listed = run_cli_json(&socket_path, &["folder", "list"]);
    assert_eq!(listed["result"]["type"], "folder_list");
    let folders = listed["result"]["folders"].as_array().unwrap();
    assert!(folders
        .iter()
        .any(|folder| folder["folder_id"].as_str() == Some(folder_id.as_str())));
    assert!(listed["result"]["order"].is_array());

    // Positional name segments join with spaces, matching workspace/tab rename.
    let renamed = run_cli_json(
        &socket_path,
        &["folder", "rename", &folder_id, "in", "flight"],
    );
    assert_eq!(renamed["result"]["type"], "folder_updated");
    assert_eq!(renamed["result"]["folder"]["name"], "in flight");

    let assigned = run_cli_json(
        &socket_path,
        &[
            "folder",
            "assign",
            &workspace_id,
            "--folder",
            &folder_id,
            "--position",
            "0",
        ],
    );
    assert_eq!(assigned["result"]["type"], "folder_assigned");
    assert_eq!(assigned["result"]["folder_id"], folder_id);
    assert!(assigned["result"]["workspace_ids"]
        .as_array()
        .unwrap()
        .iter()
        .any(|id| id.as_str() == Some(workspace_id.as_str())));

    // Omitting --folder moves the workspace back to the top level.
    let released = run_cli_json(&socket_path, &["folder", "assign", &workspace_id]);
    assert_eq!(released["result"]["type"], "folder_assigned");
    assert!(released["result"]["folder_id"].is_null());

    let moved = run_cli_json(
        &socket_path,
        &["folder", "move", &folder_id, "--position", "0"],
    );
    assert_eq!(moved["result"]["type"], "folder_moved");
    assert_eq!(moved["result"]["folder_id"], folder_id);
    assert_eq!(moved["result"]["position"], 0);

    let deleted = run_cli_json(&socket_path, &["folder", "delete", &folder_id]);
    assert_eq!(deleted["result"]["type"], "folder_deleted");
    assert_eq!(deleted["result"]["folder_id"], folder_id);

    let after_delete = run_cli_json(&socket_path, &["folder", "list"]);
    assert!(after_delete["result"]["folders"]
        .as_array()
        .unwrap()
        .is_empty());

    cleanup_spawned_herdr(herdr, base);
}

#[test]
fn folder_cli_rejects_local_argument_errors_before_socket_use() {
    let base = unique_test_dir();
    fs::create_dir_all(&base).unwrap();
    let socket_path = base.join("missing.sock");
    let cases: &[&[&str]] = &[
        // group and unknown subcommand
        &["folder"],
        &["folder", "bogus"],
        // list takes no arguments
        &["folder", "list", "extra"],
        // create requires --name with a value
        &["folder", "create"],
        &["folder", "create", "--name"],
        &["folder", "create", "--bogus", "x"],
        // rename requires a folder id and at least one name segment
        &["folder", "rename"],
        &["folder", "rename", "f1"],
        // assign requires a workspace id; flags need values; position must parse
        &["folder", "assign"],
        &["folder", "assign", "w1", "--folder"],
        &["folder", "assign", "w1", "--position"],
        &["folder", "assign", "w1", "--position", "abc"],
        &["folder", "assign", "w1", "--bogus"],
        // move requires a folder id and a parseable --position
        &["folder", "move"],
        &["folder", "move", "f1"],
        &["folder", "move", "f1", "--position"],
        &["folder", "move", "f1", "--position", "abc"],
        &["folder", "move", "f1", "--bogus"],
        // delete requires exactly one folder id
        &["folder", "delete"],
        &["folder", "delete", "f1", "extra"],
    ];

    for args in cases {
        let output = run_cli(&socket_path, args);
        assert_eq!(
            output.status.code(),
            Some(2),
            "herdr {} should fail as local parse error; stdout={} stderr={}",
            args.join(" "),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            output.stdout.is_empty(),
            "herdr {} should not print to stdout; stdout={}",
            args.join(" "),
            String::from_utf8_lossy(&output.stdout)
        );
    }

    cleanup_test_base(&base);
}

#[test]
fn folder_help_states_top_level_assign_and_family_move_semantics() {
    let base = unique_test_dir();
    fs::create_dir_all(&base).unwrap();
    let socket_path = base.join("missing.sock");

    let output = run_cli(&socket_path, &["folder", "help"]);
    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    for subcommand in ["list", "create", "rename", "assign", "move", "delete"] {
        assert!(
            stderr.contains(&format!("herdr folder {subcommand}")),
            "folder help is missing {subcommand}: {stderr}"
        );
    }
    assert!(
        stderr.contains("top level"),
        "folder help must state that omitting --folder moves to the top level: {stderr}"
    );
    assert!(
        stderr.contains("whole family"),
        "folder help must note worktree families move together: {stderr}"
    );

    cleanup_test_base(&base);
}
