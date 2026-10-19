# Workspace folders

Fork-only feature. Stock Herdr's documentation does not describe it.

- [Concepts](#concepts)
- [Sidebar](#sidebar)
- [Keyboard](#keyboard)
- [Several machines](#several-machines)
- [Agents panel](#agents-panel)
- [Configuration](#configuration)
- [CLI](#cli)
- [Socket API](#socket-api)
- [Events](#events)
- [Saved state](#saved-state)
- [Compatibility with stock Herdr](#compatibility-with-stock-herdr)

## Concepts

A folder is a named, collapsible group of workspaces on one machine. Folders hold workspaces only, one level deep, and share one sidebar order with loose workspaces. A folder stays until you delete it, even when empty. Deleting a folder only ungroups it: its workspaces stay open, loose in place. Folders belong to the server, so every attached client sees the same folders and changes appear live.

Folder IDs such as `f1` stay stable across restarts and are never reused on that server.

A worktree group always lives in one place, in a folder or loose, and moving any member moves the whole group. When a worktree action forms or extends a group, the group joins its parent's folder, otherwise the folder of a member already in one. Members that leave a group in a folder stay in that folder, right after the rest of the group. `worktree create` and `worktree open` put the new workspace in its group's folder.

New workspaces from the footer, the new-workspace keybinding, or the API without a folder are loose at the end of the list. **New workspace here** creates one at the end of that folder. Adding a workspace to a folder puts it at the folder's end, and removing it puts it right after the folder.

## Sidebar

A folder header shows ▾ or ▸ and the folder's name, with its members indented beneath it. Click anywhere on a header to collapse or expand it; this works for any machine's folders. A collapsed folder hides its members and shows their most urgent agent status: blocked, done, working, then idle. When it hides the focused workspace, the header takes the active-row highlight. Focusing a hidden workspace never expands its folder.

On the active machine, right-click a folder header to rename it, collapse or expand it, create a workspace in it with **New workspace here**, or delete it. A workspace's menu has **Move to folder ▸**, which lists the machine's folders, **New folder…**, and **Remove from folder** when the workspace is in one; worktree group children don't offer it. The sidebar footer's **new** button offers **workspace**, the usual creation flow, and **folder**, which asks for a name and adds an empty folder at the end. On a machine without folder support, it creates a workspace directly. An empty name keeps the new-folder prompt open; an empty rename changes nothing.

### Drag and drop

Drag workspaces and folder headers on the active machine to reorder them; drags never cross machines. The pointer picks the nearest gap: the upper line of a two-line row drops before it, the lower line after it.

The column picks the level, like an outliner, divided where a folder's member rows start:

- Over a folder header, right of that column files into the folder at its end and highlights the header; left places the item before the folder at top level.
- Between a folder's header and its last member, a drop lands inside the folder regardless of column.
- At an expanded folder's bottom edge (the lower line of its last member, the upper line of the next row, or empty space below a last folder), right means the end of the folder and left means top level after it.
- After a collapsed folder, a drop is always top level.

A dragged folder header moves only among top-level positions. Collapsed folders don't expand during a drag, and linked worktrees still can't be dragged; file them through **Move to folder** or the CLI. With one-line workspace rows, dragging can't reach the slot before a folder's first member; use `herdr folder add <folder> <workspace> --before <first-member>`.

## Keyboard

Folders add no keybindings, and folder headers are not keyboard-selectable.

In the expanded sidebar, digit switching, next/previous workspace and Navigate mode skip workspaces in collapsed folders. When the focused workspace is hidden, next/previous treat it as sitting at its folder's position.

The compact sidebar and the mobile layout show no folders and list every workspace, so there digits and navigation follow the rows on screen and skip nothing. The Goto picker is unchanged.

## Several machines

With more than one machine, each machine row puts its collapse arrow at the left edge, and its name lines up with the panel titles. Click the arrow to collapse or expand a machine without switching away from your current workspace; click the name to switch to that machine, as before. Machine collapse is saved with the client's other sidebar state.

Folders appear under their machine row together with its loose workspaces. You can collapse or expand any machine's folders, but folder menus and drag and drop work only on the active machine, and drags never cross machines. Dragging shows the drop line and highlights here too. With the sidebar expanded, next/previous workspace skips workspaces in collapsed folders.

Machines running stock Herdr don't support folders: their workspaces are loose, no folder controls appear for them, and nothing reports an error. Other machines are unaffected.

## Agents panel

The Agent panel has three modes: `grouped`, `priority`, and `folders`. Click the sort label in its header to cycle them.

`folders` shows a tree of machine headers (only with more than one machine), folder headers, workspace headers, and agents in tab and pane order. Loose workspaces, and every workspace on a machine without folder support, sit directly under their machine; worktree groups are ordinary workspace headers, parent first. Machine headers show the machine row's label and connection status. Headers with no agents beneath them are hidden.

Clicking a header collapses or expands it without activating the machine or focusing the workspace. A collapsed header shows the most urgent status it hides, and takes the active-row highlight when it hides the focused workspace. Machine and folder collapse is shared with the Space panel in this mode; workspace collapse is the Agent panel's own. `grouped` and `priority` ignore collapse and list every agent.

Agent rows in `folders` mode omit the `machine` and `workspace` tokens; when that leaves only the status icon on a row's first line, the icon moves to the start of the next line. The tree's indentation matches the Space panel.

In the expanded sidebar, next/previous and indexed agent focus follow the `folders` tree and skip agents under collapsed headers. The compact sidebar and mobile layout keep flat lists; in `folders` mode, they use grouped order. An active `agent.view.set` view overrides the mode until cleared.

## Configuration

To choose the Agent panel mode in config, use the fork-only key:

```toml
[ui]
agent_panel_mode = "folders"
```

`agent_panel_mode` accepts `"grouped"` (aliases `"spaces"` and `"workspaces"`), `"priority"`, or `"folders"`. When set, it replaces `agent_panel_sort`; it is unset by default.

Do not set `agent_panel_sort = "folders"`. Stock Herdr rejects unknown values there and falls back to default settings for the entire config, while it only warns about the unknown `agent_panel_mode` key and keeps everything else.

A mode chosen from the panel header beats config and survives reloads. Until you first choose one in this build, a choice saved by stock Herdr applies.

## CLI

```bash
herdr folder list
herdr folder get <folder_id>
herdr folder create <name> [--workspace ID]... [--before ID | --after ID]
herdr folder rename <folder_id> <name>
herdr folder delete <folder_id>
herdr folder move <folder_id> (--before ID | --after ID)
herdr folder add <folder_id> <workspace_id> [--before ID | --after ID]
herdr folder remove <workspace_id> [--before ID | --after ID]
herdr workspace create [--cwd PATH] [--label TEXT] [--env KEY=VALUE] [--focus] [--no-focus] [--folder FOLDER_ID]
```

`folder list` prints each folder with its `position` and member `workspace_ids`, plus `order`, the combined sidebar order of folder IDs and loose workspace IDs.

`--before` and `--after` name a neighbor: a folder or loose workspace for `create`, `move`, and `remove`, or a member of the target folder for `add`. Without one, `create` places the folder at the end, or where its first `--workspace` was; `add` files the workspace at the end of the folder; and `remove` places it right after the folder it leaves. Naming any member of a worktree group acts on the whole group. `delete` only ungroups: the workspaces stay open, loose in place. Unknown IDs, empty names, and invalid positions return an error and change nothing. Folder collapse is per client and has no command.

`workspace create --folder` files the new workspace at the end of that folder; an unknown folder fails before anything is created. Without `--folder`, a new workspace is loose at the end of the sidebar.

`folder` commands work with `--machine` like other forwarded commands. Against a machine whose server does not support folders, folder commands and `workspace create --folder` fail with `folders_unsupported` and create nothing.

## Socket API

Methods: `folder.list`, `folder.get`, `folder.create`, `folder.rename`, `folder.delete`, `folder.move`, `folder.add_workspace`, `folder.remove_workspace`, `folder.create_workspace`. Collapse is client state; there is no collapse API.

```json
{"id":"req_folders","method":"folder.list","params":{}}
{"id":"req_folder_create","method":"folder.create","params":{"name":"api","workspace_ids":["w2","w3"],"after":"w1"}}
{"id":"req_folder_add","method":"folder.add_workspace","params":{"folder_id":"f1","workspace_id":"w5","before":"w3"}}
{"id":"req_folder_remove","method":"folder.remove_workspace","params":{"workspace_id":"w5","after":"f1"}}
{"id":"req_folder_workspace","method":"folder.create_workspace","params":{"folder_id":"f1","cwd":"/repo","focus":false}}
```

`folder.list` returns `type: "folder_list"` with `folders` and `order`, the combined order of folder IDs and loose workspace IDs. `folder.get`, `folder.create`, `folder.rename`, `folder.move`, and `folder.add_workspace` return `type: "folder_info"` with one `folder`. `folder.delete` returns `folder_list`, and `folder.remove_workspace` returns `workspace_info`. A folder record has `folder_id`, `name`, `position` (its zero-based index in the combined order), and member `workspace_ids` in sidebar order. With `position` and the order of loose workspaces, a client can reproduce the sidebar order.

Workspace records include `folder_id` when the workspace is in a folder, and `session.snapshot` includes `folders` when any exist.

Positions name a neighbor with `before` or `after`, never both. For `folder.create`, `folder.move`, and `folder.remove_workspace`, the neighbor is a folder ID or a loose workspace ID. For `folder.add_workspace`, it is a member of the target folder. `folder.move` requires a position. Without one, a new folder goes to the end, or where its first listed workspace was (right after that workspace's folder if it was in one). An added workspace goes to the end of the folder, and a removed workspace goes right after the folder it leaves. Starting `workspace_ids` keep their listed order and leave any other folder; duplicates are ignored. Adding a workspace to its own folder repositions it. Removing a loose workspace without a position succeeds without changes; with a position, it moves there. Operations on any member of a worktree group act on the whole group. Names are trimmed and need not be unique. Deleting a folder makes its workspaces loose in place and closes nothing.

`folder.create_workspace` accepts `folder_id` plus the `workspace.create` fields (`source_workspace_id`, `cwd`, `focus`, `label`, `env`), returns `workspace_created`, and files the new workspace at the end of the folder. An unknown folder fails before anything is created. `workspace.create` keeps its shape and creates loose workspaces at the end.

`workspace.move` and `workspace.move_block` never change folder membership. A position that would take a workspace out of its folder, or move a loose workspace into one, is clamped to the nearest folder edge that keeps membership.

Folder errors change nothing. Codes are `folder_not_found`, `workspace_not_found`, `folder_name_invalid` for a name that is empty after trimming, and `folder_position_invalid` for a neighbor outside the target level or folder, or for both `before` and `after`. `folder_ids_exhausted` only occurs when saved folder data carries ids near the integer limit; delete that folder to create new ones.

## Events

Folder event subscriptions include `folder.created`, `folder.renamed`, `folder.deleted`, `folder.moved`, and `workspace.folder_changed`; plugin event hooks use the same names. Event envelopes use `folder_created`, `folder_renamed`, `folder_deleted`, `folder_moved`, and `workspace_folder_changed` for `event` and `data.type`.

- `folder.created` includes the new `folder`.
- `folder.renamed` includes `folder_id` and `name`.
- `folder.deleted` includes `folder_id` and the former members' `workspace_ids`.
- `folder.moved` includes `folder_id`.
- `workspace.folder_changed` includes `workspace_id`, optional `folder_id`, and optional `previous_folder_id`; it is also emitted when worktree-group placement moves a workspace.

Every folder event carries `folders`, the full folder list after the change. Folder operations that reorder workspaces also emit `workspace.reordered` with the full workspace list. Operations that change nothing emit nothing.

A new workspace whose worktree group is already in a folder joins that folder right after it opens, so its `workspace.create`, `worktree.create`, or `worktree.open` result and `workspace.created` event have no `folder_id`; a `workspace.folder_changed` event follows.

`events.wait` also matches these events and returns `wait_matched` with the event envelope. `folder_created`, `folder_renamed`, `folder_deleted`, and `folder_moved` matches accept an optional `folder_id`; `workspace_folder_changed` accepts optional `workspace_id` and `folder_id`, the new folder. IDs take the same forms as the folder methods, such as `f01` for `f1`; an unknown `workspace_id` fails with `workspace_not_found`:

```json
{"id":"req_wait_folder","method":"events.wait","params":{"match_event":{"event":"workspace_folder_changed","workspace_id":"w5"}}}
```

## Saved state

Herdr saves each session's folders in `folders.json` next to `session.json`. Keeping them separate lets stock Herdr rewrite `session.json` without dropping folders. Folders survive server restarts and live handoff.

If stock Herdr ran the same session, from disk or through live handoff into this build, Herdr reconciles the saved folders:

- Members that no longer exist are dropped. Before dropping them at startup, Herdr copies the loaded file to `folders-recovery.json` beside it, replacing any older copy, and logs the path. If you recover a session from `session-backups/` or `session-snapshots/`, stop the server and copy `folders-recovery.json` over `folders.json` as well.
- Each folder's surviving members are gathered at the position of its first surviving member, keeping the flat order stock Herdr left.
- Each worktree group then joins its parent's folder, otherwise the folder of a member already in one.
- A folder left empty returns to its saved place, or to the end when that place cannot be determined.
- If a workspace created under stock Herdr received the ID of a closed folder member, Herdr treats it as that member and it lands in the old folder; move it out. The same ID reuse can apply a saved Agent panel workspace collapse to the wrong workspace.

Collapse state belongs to each client and is saved on the client's host: per machine, which folders and Agent panel workspaces are collapsed, and which machines are collapsed. Like worktree-group collapse, it is stored per home connection: the local session, or each `--remote` target and session. Two running clients can differ; after a restart, the latest save wins. A workspace's Agent panel collapse is forgotten when it closes, and a folder's collapse when the folder is deleted.

These values, and the Agent panel mode chosen in this build, live in a separate file next to the client preferences file, with the same name and a `.fork.json` extension, for example `client-shell/local-<hash>.fork.json` under the Herdr state directory. Stock Herdr can still read and rewrite its own preferences file, and the Agent panel choice it saved there is left untouched.

## Compatibility with stock Herdr

- A stock Herdr client attached to a server with folders works normally and shows a flat list, with each folder's members side by side.
- Machines running stock Herdr show no folder controls, and folder commands against them fail with `folders_unsupported`.
- The `[ui] agent_panel_mode` key only produces an "unknown key" warning in stock Herdr.
