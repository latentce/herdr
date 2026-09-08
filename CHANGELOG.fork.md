# Fork changelog

What this fork adds on top of each upstream herdr release. Every `vX.Y.Z-fork.N`
release is upstream `vX.Y.Z` plus everything listed here; the fork revision `N`
restarts at 1 for each upstream base and increases when fork features land
between upstream releases. Upstream's
own changes live in [CHANGELOG.md](CHANGELOG.md) and the upstream release notes.

## Folders

- Spaces can be organized into named folders in the sidebar: create, rename, and delete folders from right-click menus, move spaces by menu or drag-and-drop to exact positions, and drop a space onto a folder header to file it. Deleting a folder releases its members in place and never closes spaces.
- Worktree families move as one unit when foldered: assigning any member or dragging the family's parent moves the whole family, and new worktree children automatically join their family's folder. Linked worktree children keep upstream's no-drag rule; the parent is the drag handle.
- Folders collapse and expand with a header chevron in both the spaces panel and the agents panel, with one collapse state shared by both panels and remembered per machine across restarts (alongside the other client sidebar preferences). A collapsed folder hides all of its spaces, and its header lights up when the active space is inside it. The multi-machine sidebar shows every machine's folders in its own section, collapsible in place.
- The agents panel gained a third `folders` ordering that mirrors the spaces panel's organization: agents nest under their space and folder, worktree families keep their nesting, and each space's agent list can be collapsed independently. The choice persists like the existing orderings and can be set as the startup default.
- The socket API gained `folder.create`, `folder.list`, `folder.rename`, `folder.assign`, `folder.move`, and `folder.delete` methods with matching `folder.*` events; workspace records now carry a `folder_id` when foldered, and the sidebar space order is a server-owned session fact that survives restarts.
- A new `herdr folder` CLI group wraps the folder socket methods 1:1 (`list`, `create`, `rename`, `assign`, `move`, `delete`), so shells and agents can organize folders without speaking raw socket JSON.

## Fork infrastructure

- Fork builds are published as `vX.Y.Z-fork.N` GitHub releases by `.github/workflows/fork-release.yml`, which syncs each upstream stable release, reapplies the fork patches, and uploads binaries for all five upstream platforms (Linux x86_64/aarch64, macOS x86_64/aarch64, Windows x86_64) with a `latest.json` update manifest. Fork builds report their identity as `vX.Y.Z-fork.N` in `herdr --version` and self-update when a newer fork revision of the same upstream base is published.
- Installed binaries self-update from this fork's release manifest instead of upstream's.
- An install script for Linux and macOS is published to GitHub Pages (`install.sh`) pointing at the fork releases. Windows builds are downloaded from the releases page.
