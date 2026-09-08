use crate::api::schema::{
    FolderAssignParams, FolderCreateParams, FolderMoveParams, FolderRenameParams,
};

const FOLDER_ASSIGN_USAGE: &str = "usage: herdr folder assign <workspace_id> [--folder FOLDER_ID] [--position N]; omitting --folder moves the workspace to the top level";
const FOLDER_MOVE_USAGE: &str = "usage: herdr folder move <folder_id> --position N";

pub(super) fn run_folder_command(args: &[String]) -> std::io::Result<i32> {
    let Some(subcommand) = args.first().map(|arg| arg.as_str()) else {
        print_folder_help();
        return Ok(2);
    };

    match subcommand {
        "list" => folder_list(&args[1..]),
        "create" => folder_create(&args[1..]),
        "rename" => folder_rename(&args[1..]),
        "assign" => folder_assign(&args[1..]),
        "move" => folder_move(&args[1..]),
        "delete" => folder_delete(&args[1..]),
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

fn folder_list(args: &[String]) -> std::io::Result<i32> {
    if !args.is_empty() {
        eprintln!("usage: herdr folder list");
        return Ok(2);
    }

    super::runtime::folder_list()
}

fn folder_create(args: &[String]) -> std::io::Result<i32> {
    let mut name = None;

    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--name" => {
                let Some(value) = args.get(index + 1) else {
                    eprintln!("missing value for --name");
                    return Ok(2);
                };
                name = Some(value.clone());
                index += 2;
            }
            other => {
                eprintln!("unknown option: {other}");
                return Ok(2);
            }
        }
    }

    let Some(name) = name else {
        eprintln!("usage: herdr folder create --name TEXT");
        return Ok(2);
    };

    super::runtime::folder_create(FolderCreateParams { name })
}

fn folder_rename(args: &[String]) -> std::io::Result<i32> {
    if args.len() < 2 {
        eprintln!("usage: herdr folder rename <folder_id> <name>");
        return Ok(2);
    }

    super::runtime::folder_rename(FolderRenameParams {
        folder_id: args[0].clone(),
        name: args[1..].join(" "),
    })
}

fn folder_assign(args: &[String]) -> std::io::Result<i32> {
    let Some(raw_workspace_id) = args.first() else {
        eprintln!("{FOLDER_ASSIGN_USAGE}");
        return Ok(2);
    };
    let mut folder_id = None;
    let mut position = None;

    let mut index = 1;
    while index < args.len() {
        match args[index].as_str() {
            "--folder" => {
                let Some(value) = args.get(index + 1) else {
                    eprintln!("missing value for --folder");
                    return Ok(2);
                };
                folder_id = Some(value.clone());
                index += 2;
            }
            "--position" => {
                let Some(value) = args.get(index + 1) else {
                    eprintln!("missing value for --position");
                    return Ok(2);
                };
                position = Some(match parse_position(value) {
                    Ok(position) => position,
                    Err(code) => return Ok(code),
                });
                index += 2;
            }
            other => {
                eprintln!("unknown option: {other}");
                eprintln!("{FOLDER_ASSIGN_USAGE}");
                return Ok(2);
            }
        }
    }

    super::runtime::folder_assign(FolderAssignParams {
        workspace_id: super::normalize_workspace_id(raw_workspace_id),
        folder_id,
        position,
    })
}

fn folder_move(args: &[String]) -> std::io::Result<i32> {
    let Some(folder_id) = args.first() else {
        eprintln!("{FOLDER_MOVE_USAGE}");
        return Ok(2);
    };
    let mut position = None;

    let mut index = 1;
    while index < args.len() {
        match args[index].as_str() {
            "--position" => {
                let Some(value) = args.get(index + 1) else {
                    eprintln!("missing value for --position");
                    return Ok(2);
                };
                position = Some(match parse_position(value) {
                    Ok(position) => position,
                    Err(code) => return Ok(code),
                });
                index += 2;
            }
            other => {
                eprintln!("unknown option: {other}");
                eprintln!("{FOLDER_MOVE_USAGE}");
                return Ok(2);
            }
        }
    }

    let Some(position) = position else {
        eprintln!("{FOLDER_MOVE_USAGE}");
        return Ok(2);
    };

    super::runtime::folder_move(FolderMoveParams {
        folder_id: folder_id.clone(),
        position,
    })
}

fn folder_delete(args: &[String]) -> std::io::Result<i32> {
    let Some(folder_id) = args.first() else {
        eprintln!("usage: herdr folder delete <folder_id>");
        return Ok(2);
    };
    if args.len() != 1 {
        eprintln!("usage: herdr folder delete <folder_id>");
        return Ok(2);
    }

    super::runtime::folder_delete(folder_id.clone())
}

/// Positions are parsed client-side; a value that is not a non-negative
/// integer is a usage error (exit 2), not a server round trip.
fn parse_position(value: &str) -> Result<usize, i32> {
    value.parse::<usize>().map_err(|_| {
        eprintln!("invalid value for --position: {value}");
        2
    })
}

fn print_folder_help() {
    eprintln!("herdr folder commands:");
    eprintln!("  herdr folder list");
    eprintln!("  herdr folder create --name TEXT");
    eprintln!("  herdr folder rename <folder_id> <name>");
    eprintln!("  herdr folder assign <workspace_id> [--folder FOLDER_ID] [--position N]");
    eprintln!("      omitting --folder moves the workspace to the top level");
    eprintln!("  herdr folder move <folder_id> --position N");
    eprintln!("  herdr folder delete <folder_id>");
    eprintln!("  assigning any worktree family member moves the whole family");
}
