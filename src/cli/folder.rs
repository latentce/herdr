//! `herdr folder` commands.

use crate::api::schema::{
    EmptyParams, FolderAddWorkspaceParams, FolderCreateParams, FolderCreateWorkspaceParams,
    FolderMoveParams, FolderRemoveWorkspaceParams, FolderRenameParams, FolderTarget, Method,
    Request, WorkspaceCreateParams,
};

pub(super) fn run_folder_command(args: &[String]) -> std::io::Result<i32> {
    let Some(subcommand) = args.first().map(String::as_str) else {
        print_folder_help();
        return Ok(2);
    };
    let rest = &args[1..];
    match subcommand {
        "list" => {
            if !rest.is_empty() {
                eprintln!("usage: herdr folder list");
                return Ok(2);
            }
            send(
                "cli:folder:list",
                Method::FolderList(EmptyParams::default()),
            )
        }
        "get" => match rest {
            [folder_id] => send(
                "cli:folder:get",
                Method::FolderGet(FolderTarget {
                    folder_id: folder_id.clone(),
                }),
            ),
            _ => usage("herdr folder get <folder_id>"),
        },
        "create" => folder_create(rest),
        "rename" => {
            if rest.len() < 2 {
                return usage("herdr folder rename <folder_id> <name>");
            }
            send(
                "cli:folder:rename",
                Method::FolderRename(FolderRenameParams {
                    folder_id: rest[0].clone(),
                    name: rest[1..].join(" "),
                }),
            )
        }
        "delete" => match rest {
            [folder_id] => send(
                "cli:folder:delete",
                Method::FolderDelete(FolderTarget {
                    folder_id: folder_id.clone(),
                }),
            ),
            _ => usage("herdr folder delete <folder_id>"),
        },
        "move" => {
            let Some(parsed) = parse_positioned(rest, 1)? else {
                return Ok(2);
            };
            if parsed.before.is_none() && parsed.after.is_none() {
                return usage("herdr folder move <folder_id> (--before ID | --after ID)");
            }
            send(
                "cli:folder:move",
                Method::FolderMove(FolderMoveParams {
                    folder_id: parsed.positional[0].clone(),
                    before: parsed.before,
                    after: parsed.after,
                }),
            )
        }
        "add" => {
            let Some(parsed) = parse_positioned(rest, 2)? else {
                return Ok(2);
            };
            send(
                "cli:folder:add",
                Method::FolderAddWorkspace(FolderAddWorkspaceParams {
                    folder_id: parsed.positional[0].clone(),
                    workspace_id: super::normalize_workspace_id(&parsed.positional[1]),
                    before: parsed.before,
                    after: parsed.after,
                }),
            )
        }
        "remove" => {
            let Some(parsed) = parse_positioned(rest, 1)? else {
                return Ok(2);
            };
            send(
                "cli:folder:remove",
                Method::FolderRemoveWorkspace(FolderRemoveWorkspaceParams {
                    workspace_id: super::normalize_workspace_id(&parsed.positional[0]),
                    before: parsed.before,
                    after: parsed.after,
                }),
            )
        }
        "help" | "--help" | "-h" => {
            print_folder_help();
            Ok(0)
        }
        _ => {
            print_folder_help();
            Ok(2)
        }
    }
}

/// `herdr workspace create --folder <id>`: creates the workspace inside the
/// folder in one request, so an unknown folder creates nothing.
pub(super) fn workspace_create_in_folder(
    folder_id: String,
    workspace: WorkspaceCreateParams,
) -> std::io::Result<i32> {
    send(
        "cli:workspace:create",
        Method::FolderCreateWorkspace(FolderCreateWorkspaceParams {
            folder_id,
            workspace,
        }),
    )
}

fn folder_create(args: &[String]) -> std::io::Result<i32> {
    let mut name = Vec::new();
    let mut workspace_ids = Vec::new();
    let mut before = None;
    let mut after = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--workspace" => {
                let Some(value) = args.get(index + 1) else {
                    eprintln!("missing value for --workspace");
                    return Ok(2);
                };
                workspace_ids.push(super::normalize_workspace_id(value));
                index += 2;
            }
            "--before" | "--after" => {
                let flag = args[index].as_str();
                let Some(value) = args.get(index + 1) else {
                    eprintln!("missing value for {flag}");
                    return Ok(2);
                };
                if flag == "--before" {
                    before = Some(value.clone());
                } else {
                    after = Some(value.clone());
                }
                index += 2;
            }
            other if other.starts_with("--") => {
                eprintln!("unknown option: {other}");
                return Ok(2);
            }
            word => {
                name.push(word.to_owned());
                index += 1;
            }
        }
    }
    if name.is_empty() {
        return usage("herdr folder create <name> [--workspace ID]... [--before ID | --after ID]");
    }
    send(
        "cli:folder:create",
        Method::FolderCreate(FolderCreateParams {
            name: name.join(" "),
            workspace_ids,
            before,
            after,
        }),
    )
}

struct Positioned {
    positional: Vec<String>,
    before: Option<String>,
    after: Option<String>,
}

/// Parses exactly `count` positional arguments plus optional `--before`/`--after`.
fn parse_positioned(args: &[String], count: usize) -> std::io::Result<Option<Positioned>> {
    let mut parsed = Positioned {
        positional: Vec::new(),
        before: None,
        after: None,
    };
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            flag @ ("--before" | "--after") => {
                let Some(value) = args.get(index + 1) else {
                    eprintln!("missing value for {flag}");
                    return Ok(None);
                };
                if flag == "--before" {
                    parsed.before = Some(value.clone());
                } else {
                    parsed.after = Some(value.clone());
                }
                index += 2;
            }
            other if other.starts_with("--") => {
                eprintln!("unknown option: {other}");
                return Ok(None);
            }
            word => {
                parsed.positional.push(word.to_owned());
                index += 1;
            }
        }
    }
    if parsed.positional.len() != count {
        print_folder_help();
        return Ok(None);
    }
    Ok(Some(parsed))
}

fn usage(text: &str) -> std::io::Result<i32> {
    eprintln!("usage: {text}");
    Ok(2)
}

/// Sends a folder request, reporting a server that predates folders clearly.
fn send(id: &'static str, method: Method) -> std::io::Result<i32> {
    let response = super::send_request(&Request {
        id: id.into(),
        method,
    })?;
    super::print_response(&unsupported_folder_error(response))
}

/// Servers without folders reject the method name itself; say so plainly.
fn unsupported_folder_error(response: serde_json::Value) -> serde_json::Value {
    let rejected_method = response["error"]["code"] == "invalid_request"
        && response["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("unknown variant `folder."));
    if !rejected_method {
        return response;
    }
    serde_json::json!({
        "id": response["id"],
        "error": {
            "code": "folders_unsupported",
            "message": "this machine's herdr server does not support folders",
        },
    })
}

fn print_folder_help() {
    eprintln!("herdr folder commands:");
    eprintln!("  herdr folder list");
    eprintln!("  herdr folder get <folder_id>");
    eprintln!("  herdr folder create <name> [--workspace ID]... [--before ID | --after ID]");
    eprintln!("  herdr folder rename <folder_id> <name>");
    eprintln!("  herdr folder delete <folder_id>");
    eprintln!("  herdr folder move <folder_id> (--before ID | --after ID)");
    eprintln!("  herdr folder add <folder_id> <workspace_id> [--before ID | --after ID]");
    eprintln!("  herdr folder remove <workspace_id> [--before ID | --after ID]");
    eprintln!(
        "Positions name a folder (fN) or a loose workspace; for `add`, a member of the folder."
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejected_folder_methods_report_missing_support() {
        let rejected = serde_json::json!({
            "id": "cli:folder:list",
            "error": {
                "code": "invalid_request",
                "message": "invalid request: unknown variant `folder.list`, expected one of `ping`",
            },
        });
        let translated = unsupported_folder_error(rejected);
        assert_eq!(translated["error"]["code"], "folders_unsupported");
        assert_eq!(translated["id"], "cli:folder:list");

        let other = serde_json::json!({
            "id": "x",
            "error": {"code": "folder_not_found", "message": "folder f9 not found"},
        });
        assert_eq!(unsupported_folder_error(other.clone()), other);
    }
}
