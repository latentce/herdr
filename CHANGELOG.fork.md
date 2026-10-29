# Fork changelog

What this fork adds on top of each upstream herdr release. Every `vX.Y.Z-fork.N`
release is upstream `vX.Y.Z` plus everything listed here; the fork revision `N`
restarts at 1 for each upstream base and increases when fork features land
between upstream releases. Upstream's own changes live in
[CHANGELOG.md](CHANGELOG.md) and the upstream release notes.

## Workspace folders

- Group a machine's workspaces into named, collapsible folders in the sidebar. Folders sit anywhere among loose workspaces, hold workspaces one level deep, and can stay empty until you delete them. Deleting a folder only ungroups it; its workspaces stay open where they were.
- Create folders from the sidebar's ` new` menu or a workspace's **Move to folder ▸** submenu, manage them from the folder header's right-click menu, and click a header to collapse or expand it. A collapsed folder shows its most urgent agent status, and highlights its header when it hides the focused workspace.
- Drag workspaces into, within, and out of folders, and drag folder headers to reorder them. The pointer's column decides the level: right of a folder's member column files into the folder, left of it stays at top level. The machines sidebar now shows the drop line and highlights for every drag.
- A worktree group always stays together in one folder; new worktree workspaces join their parent's folder.
- With the sidebar expanded, workspaces in collapsed folders are skipped by digit switching, next/previous workspace and Navigate mode. The collapsed sidebar lists every workspace, so there the keys follow the numbers on screen.
- With several machines in the sidebar, each machine's arrow sits at the left edge and its name lines up with the panel titles, so everything under a machine moves two columns left. The agents panel's `folders` mode uses the same layout.
- The agents panel has a third mode, `folders`, that shows agents under machine, folder and workspace headers. Click the sort label to cycle grouped → priority → folders, or set `[ui] agent_panel_mode = "folders"`; don't use `agent_panel_sort = "folders"`, which stock Herdr rejects.
- Folders belong to the server, so every attached client sees the same ones, and they survive restarts and live handoff. They're saved in `folders.json` next to the session, and reconcile cleanly if stock Herdr ran the same session in between. Folder and machine collapse and the agents panel mode are saved per client in a separate `.fork.json` file, leaving stock Herdr's own preferences readable.
- Scripts and agents can manage folders with `herdr folder list|get|create|rename|delete|move|add|remove`, `herdr workspace create --folder`, the `folder.*` socket API methods and `folder.*` / `workspace.folder_changed` events. Workspace objects include `folder_id`. Machines running stock Herdr report `folders_unsupported` and otherwise keep working as before.
- Full documentation: [Workspace folders](https://github.com/latentce/herdr/blob/fork/fork-docs/workspace-folders.md).

## Animated status indicators

- A third agent status style, `animated`, next to `dots` and `symbols`: working agents show a braille spinner (⠒⠲⠰⠴⠤⠦⠆⠖, 100 ms per frame) and blocked and done agents blink ● in their usual colors, fading halfway into the background every other half second so they never disappear. Idle (○) and unknown (·) look as they do with `dots`, and every animated icon moves in step.
- Pick **animated dots** in settings, on desktop or in the mobile view, or set `[ui] status_indicator_style = "animated"`; don't use `status_indicators = "animated"`, which stock Herdr rejects. Picking color dots or distinct symbols turns the animation off.
- Herdr only redraws while an animated icon is on screen, so `dots`, `symbols` and screens with only idle agents cost nothing extra.
- Full documentation: [Animated status indicators](https://github.com/latentce/herdr/blob/fork/fork-docs/animated-status-indicators.md).

## Fork infrastructure

- Fork builds are published as `vX.Y.Z-fork.N` GitHub releases by `.github/workflows/fork-release.yml`, which syncs each upstream stable release, reapplies the fork patches, and uploads binaries for all five upstream platforms (Linux x86_64/aarch64, macOS x86_64/aarch64, Windows x86_64) with a `latest.json` update manifest. Fork builds report their identity as `vX.Y.Z-fork.N` in `herdr --version` and self-update when a newer fork revision of the same upstream base is published.
- Installed binaries self-update from this fork's release manifest instead of upstream's.
- The in-app release notes for a fork update list this fork's additions, followed by upstream's notes for the base release.
- An install script for Linux and macOS is published to GitHub Pages (`install.sh`) pointing at the fork releases. Windows builds are downloaded from the releases page.
