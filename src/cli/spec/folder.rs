use clap::{Arg, Command};

use super::{option, repeatable_option, required};

/// Help and completion spec for `herdr folder`.
pub(super) fn command() -> Command {
    Command::new("folder")
        .about("Organize workspaces into folders over the socket API")
        .subcommand(Command::new("list").about("List folders in sidebar order"))
        .subcommand(
            Command::new("get")
                .about("Show a folder")
                .arg(required("folder_id", "FOLDER_ID")),
        )
        .subcommand(
            Command::new("create")
                .about("Create a folder")
                .arg(required("name", "NAME").num_args(1..))
                .arg(repeatable_option("workspace", "ID"))
                .arg(option("before", "ID"))
                .arg(option("after", "ID")),
        )
        .subcommand(
            Command::new("rename")
                .about("Rename a folder")
                .arg(required("folder_id", "FOLDER_ID"))
                .arg(required("name", "NAME").num_args(1..)),
        )
        .subcommand(
            Command::new("delete")
                .about("Delete a folder; its workspaces stay open, loose in place")
                .arg(required("folder_id", "FOLDER_ID")),
        )
        .subcommand(
            Command::new("move")
                .about("Move a folder before or after a folder or loose workspace")
                .arg(required("folder_id", "FOLDER_ID"))
                .arg(option("before", "ID"))
                .arg(option("after", "ID")),
        )
        .subcommand(
            Command::new("add")
                .about("Add a workspace, with its worktree group, to a folder")
                .arg(required("folder_id", "FOLDER_ID"))
                .arg(required("workspace_id", "WORKSPACE_ID"))
                .arg(option("before", "ID"))
                .arg(option("after", "ID")),
        )
        .subcommand(
            Command::new("remove")
                .about("Take a workspace, with its worktree group, out of its folder")
                .arg(
                    Arg::new("workspace_id")
                        .value_name("WORKSPACE_ID")
                        .required(true),
                )
                .arg(option("before", "ID"))
                .arg(option("after", "ID")),
        )
}
