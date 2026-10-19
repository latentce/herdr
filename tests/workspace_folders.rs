#![cfg(all(unix, not(target_os = "macos")))]
//! Workspace folders against a real server: CLI, events,
//! persistence across restarts and session rewrites, and live handoff.

pub mod support;

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use serde_json::{json, Value};
use support::{
    cleanup_test_base, register_runtime_dir, register_spawned_herdr_pid,
    unregister_spawned_herdr_pid,
};

struct SpawnedHerdr {
    _master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send + Sync>,
}

impl Drop for SpawnedHerdr {
    fn drop(&mut self) {
        let pid = self.child.process_id();
        support::stop_spawned_herdr(&mut *self.child);
        unregister_spawned_herdr_pid(pid);
    }
}

fn test_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn unique_test_dir() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    PathBuf::from(format!("/tmp/hwf-{}-{n}", std::process::id()))
}

struct Env {
    base: PathBuf,
    config_home: PathBuf,
    runtime_dir: PathBuf,
    socket: PathBuf,
}

impl Env {
    fn new() -> Self {
        let base = unique_test_dir();
        let config_home = base.join("config");
        let runtime_dir = base.join("runtime");
        let socket = config_home.join("herdr-dev/herdr.sock");
        Self {
            base,
            config_home,
            runtime_dir,
            socket,
        }
    }

    fn data_dir(&self) -> PathBuf {
        self.config_home.join("herdr-dev")
    }

    fn spawn(&self) -> SpawnedHerdr {
        fs::create_dir_all(self.data_dir()).unwrap();
        fs::create_dir_all(&self.runtime_dir).unwrap();
        fs::write(self.data_dir().join("config.toml"), "onboarding = false\n").unwrap();
        let pair = native_pty_system()
            .openpty(PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
        let mut cmd = CommandBuilder::new(env!("CARGO_BIN_EXE_herdr"));
        support::isolate_herdr_test_process(&mut cmd);
        cmd.arg("server");
        cmd.env("XDG_CONFIG_HOME", &self.config_home);
        cmd.env("XDG_RUNTIME_DIR", &self.runtime_dir);
        cmd.env("XDG_STATE_HOME", self.runtime_dir.join("state"));
        cmd.env_remove("HERDR_SESSION");
        cmd.env_remove("HERDR_SOCKET_PATH");
        cmd.env_remove("HERDR_CLIENT_SOCKET_PATH");
        cmd.env("SHELL", "/bin/sh");
        let child = pair.slave.spawn_command(cmd).unwrap();
        register_spawned_herdr_pid(child.process_id());
        let spawned = SpawnedHerdr {
            _master: pair.master,
            child,
        };
        wait_for_api(&self.socket, Duration::from_secs(10));
        register_runtime_dir(&self.runtime_dir);
        spawned
    }

    fn request(&self, method: &str, params: Value) -> Value {
        request(&self.socket, method, params)
    }

    fn ok(&self, method: &str, params: Value) -> Value {
        let response = self.request(method, params);
        assert!(
            response.get("result").is_some(),
            "{method} failed: {response}"
        );
        response["result"].clone()
    }

    fn cli(&self, args: &[&str]) -> std::process::Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_herdr"));
        command.args(args);
        command.env("HERDR_SOCKET_PATH", &self.socket);
        command.env("XDG_CONFIG_HOME", &self.config_home);
        command.env("XDG_STATE_HOME", self.runtime_dir.join("state"));
        command.output().unwrap()
    }

    fn cli_json(&self, args: &[&str]) -> Value {
        let output = self.cli(args);
        assert!(
            output.status.success(),
            "herdr {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap_or_else(|err| {
            panic!(
                "herdr {args:?}: {err}: {}",
                String::from_utf8_lossy(&output.stdout)
            )
        })
    }

    fn cli_error(&self, args: &[&str]) -> Value {
        let output = self.cli(args);
        assert_eq!(output.status.code(), Some(1), "herdr {args:?} should fail");
        serde_json::from_slice(&output.stderr).unwrap_or_else(|err| {
            panic!(
                "herdr {args:?}: {err}: {}",
                String::from_utf8_lossy(&output.stderr)
            )
        })
    }

    fn create_workspaces(&self, count: usize) -> Vec<String> {
        (0..count)
            .map(|index| {
                let cwd = self.base.join(format!("ws{index}"));
                fs::create_dir_all(&cwd).unwrap();
                self.ok(
                    "workspace.create",
                    json!({"cwd": cwd, "label": format!("ws{index}")}),
                )["workspace"]["workspace_id"]
                    .as_str()
                    .unwrap()
                    .to_owned()
            })
            .collect()
    }

    fn folders(&self) -> Value {
        self.ok("folder.list", json!({}))
    }

    fn stop(&self) {
        let _ = try_request(&self.socket, "server.stop", json!({}));
        let deadline = Instant::now() + Duration::from_secs(10);
        while self.socket.exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(25));
        }
    }
}

fn try_request(socket: &Path, method: &str, params: Value) -> Result<Value, String> {
    let mut stream = UnixStream::connect(socket).map_err(|err| err.to_string())?;
    let request = json!({"id": "test", "method": method, "params": params});
    writeln!(stream, "{request}").map_err(|err| err.to_string())?;
    let mut line = String::new();
    BufReader::new(stream)
        .read_line(&mut line)
        .map_err(|err| err.to_string())?;
    serde_json::from_str(&line).map_err(|err| format!("{err}: {line:?}"))
}

fn request(socket: &Path, method: &str, params: Value) -> Value {
    try_request(socket, method, params).unwrap_or_else(|err| panic!("{method}: {err}"))
}

fn wait_for_api(socket: &Path, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if try_request(socket, "ping", json!({}))
            .is_ok_and(|response| response.get("result").is_some())
        {
            return;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    panic!("api did not become ready at {}", socket.display());
}

fn order(list: &Value) -> Vec<String> {
    serde_json::from_value(list["order"].clone()).unwrap()
}

fn members(list: &Value, folder_id: &str) -> Vec<String> {
    list["folders"]
        .as_array()
        .unwrap()
        .iter()
        .find(|folder| folder["folder_id"] == folder_id)
        .map(|folder| serde_json::from_value(folder["workspace_ids"].clone()).unwrap())
        .unwrap_or_else(|| panic!("folder {folder_id} in {list}"))
}

#[test]
fn folder_cli_manages_folders_and_reports_errors() {
    let _lock = test_lock();
    let env = Env::new();
    let _server = env.spawn();
    let ids = env.create_workspaces(4);

    let created = env.cli_json(&[
        "folder",
        "create",
        "back",
        "end",
        "--workspace",
        &ids[1],
        "--workspace",
        &ids[2],
    ]);
    assert_eq!(created["result"]["folder"]["folder_id"], "f1");
    assert_eq!(created["result"]["folder"]["name"], "back end");
    assert_eq!(
        created["result"]["folder"]["workspace_ids"],
        json!([ids[1], ids[2]])
    );
    let listed = env.cli_json(&["folder", "list"]);
    assert_eq!(order(&listed["result"]), [&ids[0], "f1", &ids[3]]);
    let workspace = env.cli_json(&["workspace", "get", &ids[1]]);
    assert_eq!(workspace["result"]["workspace"]["folder_id"], "f1");

    env.cli_json(&["folder", "rename", "f1", "  backend  "]);
    assert_eq!(
        env.cli_json(&["folder", "get", "f1"])["result"]["folder"]["name"],
        "backend"
    );
    env.cli_json(&["folder", "create", "empty", "--before", &ids[0]]);
    env.cli_json(&["folder", "move", "f1", "--after", &ids[3]]);
    assert_eq!(order(&env.folders()), ["f2", &ids[0], &ids[3], "f1"]);
    env.cli_json(&["folder", "add", "f2", &ids[3]]);
    env.cli_json(&["folder", "add", "f1", &ids[0], "--before", &ids[1]]);
    assert_eq!(
        members(&env.folders(), "f1"),
        [ids[0].as_str(), ids[1].as_str(), ids[2].as_str()]
    );
    env.cli_json(&["folder", "remove", &ids[0], "--after", "f2"]);
    assert_eq!(order(&env.folders()), ["f2", &ids[0], "f1"]);

    let count = env.ok("workspace.list", json!({}))["workspaces"]
        .as_array()
        .unwrap()
        .len();
    let made = env.cli_json(&[
        "workspace",
        "create",
        "--folder",
        "f2",
        "--cwd",
        env.base.to_str().unwrap(),
    ]);
    let made_id = made["result"]["workspace"]["workspace_id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(made["result"]["workspace"]["folder_id"], "f2");
    assert_eq!(
        members(&env.folders(), "f2"),
        [ids[3].as_str(), made_id.as_str()]
    );
    let error = env.cli_error(&["workspace", "create", "--folder", "f9"]);
    assert_eq!(error["error"]["code"], "folder_not_found");
    assert_eq!(
        env.ok("workspace.list", json!({}))["workspaces"]
            .as_array()
            .unwrap()
            .len(),
        count + 1
    );

    let before = env.folders();
    assert_eq!(
        env.cli_error(&["folder", "create", "   "])["error"]["code"],
        "folder_name_invalid"
    );
    assert_eq!(
        env.cli_error(&["folder", "rename", "f1", " "])["error"]["code"],
        "folder_name_invalid"
    );
    assert_eq!(
        env.cli_error(&["folder", "get", "f42"])["error"]["code"],
        "folder_not_found"
    );
    assert_eq!(
        env.cli_error(&["folder", "add", "f1", &ids[0], "--before", &ids[3]])["error"]["code"],
        "folder_position_invalid"
    );
    assert_eq!(
        env.cli_error(&["folder", "create", "x", "--workspace", "w999"])["error"]["code"],
        "workspace_not_found"
    );
    assert_eq!(env.folders(), before);

    env.cli_json(&["folder", "delete", "f1"]);
    let after = env.folders();
    assert!(after["folders"]
        .as_array()
        .unwrap()
        .iter()
        .all(|folder| folder["folder_id"] != "f1"));
    assert_eq!(order(&after), ["f2", &ids[0], &ids[1], &ids[2]]);
    let created = env.cli_json(&["folder", "create", "new"]);
    assert_eq!(
        created["result"]["folder"]["folder_id"], "f3",
        "folder ids are never reused"
    );

    let schema = env.cli_json(&["api", "schema", "--json"]);
    assert!(schema.to_string().contains("folder.add_workspace"));
    env.stop();
    cleanup_test_base(&env.base);
}

#[test]
fn folder_events_reach_subscribers_and_events_wait() {
    let _lock = test_lock();
    let env = Env::new();
    let _server = env.spawn();
    let ids = env.create_workspaces(2);

    let socket = env.socket.clone();
    let waiter = std::thread::spawn(move || {
        request(
            &socket,
            "events.wait",
            json!({"match_event": {"event": "folder_created"}, "timeout_ms": 10000}),
        )
    });
    let socket = env.socket.clone();
    let spelled_waiter = std::thread::spawn(move || {
        request(
            &socket,
            "events.wait",
            json!({"match_event": {
                "event": "workspace_folder_changed", "workspace_id": "2", "folder_id": "f01"
            }, "timeout_ms": 10000}),
        )
    });
    let missing = env.request(
        "events.wait",
        json!({"match_event": {"event": "workspace_folder_changed", "workspace_id": "w_99"}}),
    );
    assert_eq!(missing["error"]["code"], "workspace_not_found", "{missing}");
    let mut stream = UnixStream::connect(&env.socket).unwrap();
    writeln!(
        stream,
        "{}",
        json!({"id": "sub", "method": "events.subscribe", "params": {"subscriptions": [
            {"type": "folder.created"}, {"type": "folder.renamed"}, {"type": "folder.moved"},
            {"type": "folder.deleted"}, {"type": "workspace.folder_changed"}
        ]}})
    )
    .unwrap();
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut ack = String::new();
    reader.read_line(&mut ack).unwrap();
    std::thread::sleep(Duration::from_millis(300));

    env.ok(
        "folder.create",
        json!({"name": "x", "workspace_ids": [ids[1]]}),
    );
    env.ok("folder.rename", json!({"folder_id": "f1", "name": "y"}));
    env.ok("folder.move", json!({"folder_id": "f1", "before": ids[0]}));
    env.ok("folder.delete", json!({"folder_id": "f1"}));

    let matched = waiter.join().unwrap();
    assert_eq!(matched["result"]["type"], "wait_matched");
    assert_eq!(matched["result"]["event"]["data"]["type"], "folder_created");
    let matched = spelled_waiter.join().unwrap();
    assert_eq!(
        matched["result"]["event"]["data"]["workspace_id"], ids[1],
        "{matched}"
    );

    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut seen = Vec::new();
    while seen.len() < 6 {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) == 0 {
            break;
        }
        let event: Value = serde_json::from_str(&line).unwrap();
        seen.push(event["event"].as_str().unwrap_or_default().to_owned());
    }
    for expected in [
        "folder_created",
        "workspace_folder_changed",
        "folder_renamed",
        "folder_moved",
        "folder_deleted",
    ] {
        assert!(
            seen.iter().any(|event| event == expected),
            "{expected} in {seen:?}"
        );
    }
    env.stop();
    cleanup_test_base(&env.base);
}

#[test]
fn folders_survive_restarts_and_reconcile_a_rewritten_session() {
    let _lock = test_lock();
    let env = Env::new();
    let server = env.spawn();
    let ids = env.create_workspaces(5);
    env.ok(
        "folder.create",
        json!({"name": "pair", "workspace_ids": [ids[1], ids[2]]}),
    );
    env.ok(
        "folder.create",
        json!({"name": "solo", "workspace_ids": [ids[4]]}),
    );
    env.ok("folder.create", json!({"name": "empty", "after": ids[0]}));
    let before = env.folders();
    env.stop();
    drop(server);
    assert!(env.data_dir().join("folders.json").exists());

    let server = env.spawn();
    assert_eq!(env.folders(), before);
    env.stop();
    drop(server);
    let recovery = env.data_dir().join("folders-recovery.json");
    assert!(!recovery.exists(), "nothing was missing");
    let saved = fs::read_to_string(env.data_dir().join("folders.json")).unwrap();

    let session_path = env.data_dir().join("session.json");
    let mut session: Value =
        serde_json::from_str(&fs::read_to_string(&session_path).unwrap()).unwrap();
    let workspaces = session["workspaces"].as_array().unwrap().clone();
    let by_id = |id: &str| {
        workspaces
            .iter()
            .find(|workspace| workspace["id"] == id)
            .cloned()
            .unwrap()
    };
    session["workspaces"] = json!([by_id(&ids[1]), by_id(&ids[0]), by_id(&ids[3])]);
    session["active"] = json!(0);
    session["selected"] = json!(0);
    fs::write(&session_path, serde_json::to_string(&session).unwrap()).unwrap();

    let server = env.spawn();
    let reconciled = env.folders();
    assert_eq!(fs::read_to_string(&recovery).unwrap(), saved);
    assert_eq!(members(&reconciled, "f1"), [ids[1].as_str()]);
    assert_eq!(members(&reconciled, "f2"), Vec::<String>::new());
    assert_eq!(order(&reconciled), ["f1", &ids[0], "f3", &ids[3], "f2"]);
    let created = env.ok("folder.create", json!({"name": "later"}));
    assert_eq!(created["folder"]["folder_id"], "f4");
    env.stop();
    drop(server);
    cleanup_test_base(&env.base);
}

#[test]
fn folders_survive_live_handoff() {
    let _lock = test_lock();
    let env = Env::new();
    let server = env.spawn();
    let ids = env.create_workspaces(3);
    env.ok(
        "folder.create",
        json!({"name": "kept", "workspace_ids": [ids[2], ids[0]]}),
    );
    env.ok("folder.create", json!({"name": "empty"}));
    let before = env.folders();

    let handoff = env.request("server.live_handoff", json!({}));
    assert!(handoff.get("result").is_some(), "handoff failed: {handoff}");
    drop(server);
    wait_for_api(&env.socket, Duration::from_secs(10));
    assert_eq!(env.folders(), before);
    let snapshot = env.ok("session.snapshot", json!({}));
    assert_eq!(snapshot["snapshot"]["folders"][0]["folder_id"], "f1");
    env.stop();
    cleanup_test_base(&env.base);
}

/// Endpoint control messages (kind, data) read until `stop` matches or time runs out.
fn read_controls(
    stream: &mut UnixStream,
    timeout: Duration,
    stop: impl Fn(&str, &str) -> bool,
) -> Vec<(String, String)> {
    fn string(payload: &[u8], offset: &mut usize) -> Option<String> {
        let (len, used) = match *payload.get(*offset)? {
            byte @ 0..=250 => (usize::from(byte), 1),
            251 => (
                usize::from(u16::from_le_bytes(
                    payload.get(*offset + 1..*offset + 3)?.try_into().ok()?,
                )),
                3,
            ),
            252 => (
                u32::from_le_bytes(payload.get(*offset + 1..*offset + 5)?.try_into().ok()?)
                    as usize,
                5,
            ),
            _ => return None,
        };
        *offset += used;
        let value = String::from_utf8(payload.get(*offset..*offset + len)?.to_vec()).ok()?;
        *offset += len;
        Some(value)
    }
    stream
        .set_read_timeout(Some(Duration::from_millis(200)))
        .unwrap();
    let deadline = Instant::now() + timeout;
    let mut controls = Vec::new();
    while Instant::now() < deadline {
        let Ok((variant, payload)) = support::read_server_message(stream) else {
            continue;
        };
        if variant != support::SERVER_MESSAGE_ENDPOINT_CONTROL {
            continue;
        }
        let mut offset = 0;
        let (Some(kind), Some(data)) =
            (string(&payload, &mut offset), string(&payload, &mut offset))
        else {
            continue;
        };
        let done = stop(&kind, &data);
        controls.push((kind, data));
        if done {
            break;
        }
    }
    controls
}

const LAYOUT_KIND: &str = "endpoint.workspace-folders.v1";

#[test]
fn attached_clients_receive_folder_layouts_with_their_snapshots() {
    let _lock = test_lock();
    let env = Env::new();
    let _server = env.spawn();
    let ids = env.create_workspaces(2);
    let client_socket = env.data_dir().join("herdr-client.sock");

    let mut first = UnixStream::connect(&client_socket).unwrap();
    support::client_shell_handshake(
        &mut first,
        support::CURRENT_ENDPOINT_PROTOCOL_GENERATION,
        80,
        24,
    )
    .unwrap();
    let bootstrap = read_controls(&mut first, Duration::from_secs(3), |kind, _| {
        kind == "shell.snapshot.v1"
    });
    assert!(bootstrap
        .iter()
        .any(|(kind, _)| kind == "shell.snapshot.v1"));
    assert!(
        bootstrap.iter().all(|(kind, _)| kind != LAYOUT_KIND),
        "servers without folders send no layout"
    );

    env.ok(
        "folder.create",
        json!({"name": "x", "workspace_ids": [ids[1]]}),
    );
    let update = read_controls(&mut first, Duration::from_secs(5), |kind, _| {
        kind == LAYOUT_KIND
    });
    let (_, data) = update
        .iter()
        .find(|(kind, _)| kind == LAYOUT_KIND)
        .expect("live folder layout");
    let layout: Value = serde_json::from_str(data).unwrap();
    assert_eq!(layout["layout"]["folders"][0]["id"], "f1");
    assert_eq!(
        layout["layout"]["folders"][0]["workspace_ids"],
        json!([ids[1]])
    );
    assert_eq!(layout["layout"]["order"], json!([ids[0], "f1"]));

    let mut second = UnixStream::connect(&client_socket).unwrap();
    support::client_shell_handshake(
        &mut second,
        support::CURRENT_ENDPOINT_PROTOCOL_GENERATION,
        80,
        24,
    )
    .unwrap();
    let seed = read_controls(&mut second, Duration::from_secs(3), |kind, _| {
        kind == "shell.snapshot.v1"
    });
    let layout_at = seed.iter().position(|(kind, _)| kind == LAYOUT_KIND);
    let snapshot_at = seed
        .iter()
        .position(|(kind, _)| kind == "shell.snapshot.v1");
    assert!(
        layout_at.is_some() && layout_at < snapshot_at,
        "layout before snapshot: {:?}",
        seed.iter().map(|(kind, _)| kind).collect::<Vec<_>>()
    );
    env.stop();
    cleanup_test_base(&env.base);
}
