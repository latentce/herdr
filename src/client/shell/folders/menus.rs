//! Folder menus: the footer's new menu, the folder header menu, and the
//! workspace menu's "Move to folder ▸" submenu. Also the folder name prompts.

use super::*;
use crate::api::schema::{
    FolderAddWorkspaceParams, FolderCreateParams, FolderCreateWorkspaceParams,
    FolderRemoveWorkspaceParams, FolderRenameParams, FolderTarget, Method, WorkspaceCreateParams,
};

/// Longest folder name shown in a menu before it is cut off.
const MENU_NAME_WIDTH: usize = 32;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::client::shell) enum FolderMenuKind {
    /// The footer's ` new` menu.
    New,
    /// Right-click menu of a folder header.
    Header { folder_id: String },
    /// "Move to folder ▸" submenu of a workspace menu.
    MoveTo { workspace_id: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::client::shell) enum FolderMenuAction {
    NewWorkspace,
    NewFolder,
    Rename,
    ToggleCollapse,
    NewWorkspaceHere,
    Delete,
    MoveTo(String),
    MoveToNewFolder,
    RemoveFromFolder,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::client::shell) struct FolderMenuItem {
    pub(in crate::client::shell) label: String,
    pub(in crate::client::shell) enabled: bool,
    pub(in crate::client::shell) action: FolderMenuAction,
}

#[derive(Debug)]
pub(in crate::client::shell) struct FolderMenuOverlay {
    pub(in crate::client::shell) kind: FolderMenuKind,
    pub(in crate::client::shell) x: u16,
    pub(in crate::client::shell) y: u16,
    pub(in crate::client::shell) highlighted: usize,
    /// The workspace menu a submenu was opened from, drawn beneath it.
    pub(in crate::client::shell) parent: Option<ClientContextMenuOverlay>,
}

/// Prompts that edit folders, shown with the shared rename prompt.
#[derive(Debug)]
pub(in crate::client::shell) enum FolderPrompt {
    /// A new folder, optionally containing a workspace.
    Create {
        workspace_id: Option<String>,
    },
    Rename {
        folder_id: String,
    },
    NewWorkspace {
        folder_id: String,
        source_workspace_id: Option<String>,
        cwd: Option<String>,
        suggested_name: String,
    },
}

/// Where the menu was drawn, for mouse handling.
#[derive(Default)]
pub(in crate::client::shell) struct FolderMenuRender {
    pub(in crate::client::shell) area: Rect,
    pub(in crate::client::shell) rows: Vec<(Rect, usize)>,
    pub(in crate::client::shell) parent: Option<render::overlays::OverlayRender>,
}

/// Items of a folder menu, from the active machine's current layout.
pub(in crate::client::shell) fn folder_menu_items(
    folders: &FolderUiState,
    endpoints: &[ClientShellEndpoint],
    active_endpoint_id: &ClientEndpointId,
    menu: &FolderMenuOverlay,
) -> Vec<FolderMenuItem> {
    let item = |label: &str, action| FolderMenuItem {
        label: label.to_owned(),
        enabled: true,
        action,
    };
    match &menu.kind {
        FolderMenuKind::New => vec![
            item("workspace", FolderMenuAction::NewWorkspace),
            item("folder", FolderMenuAction::NewFolder),
        ],
        FolderMenuKind::Header { folder_id } => vec![
            item("Rename", FolderMenuAction::Rename),
            item(
                if folders.is_folder_collapsed(active_endpoint_id, folder_id) {
                    "Expand"
                } else {
                    "Collapse"
                },
                FolderMenuAction::ToggleCollapse,
            ),
            item("New workspace here", FolderMenuAction::NewWorkspaceHere),
            item("Delete folder", FolderMenuAction::Delete),
        ],
        FolderMenuKind::MoveTo { workspace_id } => {
            let Some(view) = folders.view_for(endpoints, active_endpoint_id) else {
                return Vec::new();
            };
            let current = view.folder_id_of(workspace_id).map(str::to_owned);
            let mut items = view
                .layout
                .folders
                .iter()
                .map(|folder| FolderMenuItem {
                    label: crate::ui::truncate_end(&folder.name, MENU_NAME_WIDTH),
                    enabled: current.as_deref() != Some(folder.id.as_str()),
                    action: FolderMenuAction::MoveTo(folder.id.clone()),
                })
                .collect::<Vec<_>>();
            items.push(item("New folder…", FolderMenuAction::MoveToNewFolder));
            if current.is_some() {
                items.push(item(
                    "Remove from folder",
                    FolderMenuAction::RemoveFromFolder,
                ));
            }
            items
        }
    }
}

/// Draws a folder menu. A submenu opens to the right of its parent menu and
/// flips left when there is no room; long lists scroll.
pub(in crate::client::shell) fn render_folder_menu(
    buffer: &mut Buffer,
    menu: &FolderMenuOverlay,
    items: &[FolderMenuItem],
    palette: &Palette,
) -> Option<FolderMenuRender> {
    let screen = buffer.area;
    let parent = match menu.parent.as_ref() {
        Some(parent) => Some(render::render_context_menu(buffer, parent, palette)?),
        None => None,
    };
    let label_width = items
        .iter()
        .map(|item| display_width(&item.label))
        .max()
        .unwrap_or(0);
    let width = label_width
        .saturating_add(4)
        .max(14)
        .min(screen.width.max(1));
    let height = (items.len() as u16)
        .saturating_add(2)
        .min(screen.height.max(1));
    let (x, y) = match parent.as_ref() {
        Some(parent) => {
            let anchor_row = parent
                .menu_rows
                .iter()
                .find(|(_, index)| Some(*index) == menu.parent.as_ref().map(|p| p.highlighted))
                .map_or(parent.area.y, |(row, _)| row.y);
            let x = if parent.area.right().saturating_add(width) <= screen.right() {
                parent.area.right()
            } else {
                parent.area.x.saturating_sub(width)
            };
            (x, anchor_row.saturating_sub(1))
        }
        None => (menu.x, menu.y),
    };
    let x = x.min(screen.right().saturating_sub(width));
    let y = y.min(screen.bottom().saturating_sub(height));
    let area = Rect::new(x, y, width, height);
    let inner = render::overlays::panel(buffer, area, palette.accent, palette.panel_bg)?;
    let visible = usize::from(inner.height).max(1);
    let scroll = (menu.highlighted + 1).saturating_sub(visible);
    let mut rows = Vec::new();
    for (offset, (index, item)) in items.iter().enumerate().skip(scroll).enumerate() {
        let row_y = inner.y.saturating_add(offset as u16);
        if row_y >= inner.bottom() {
            break;
        }
        let row = Rect::new(inner.x, row_y, inner.width, 1);
        let style = if !item.enabled {
            Style::default().fg(palette.overlay0).bg(palette.panel_bg)
        } else if index == menu.highlighted {
            Style::default()
                .fg(panel_contrast_fg(palette))
                .bg(palette.accent)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(palette.text).bg(palette.panel_bg)
        };
        buffer.set_style(row, style);
        put_text(buffer, row.x, row.y, row.width, &item.label, style);
        rows.push((row, index));
    }
    Some(FolderMenuRender { area, rows, parent })
}

impl ClientShellState {
    fn current_folder_menu_items(&self) -> Vec<FolderMenuItem> {
        match self.overlay.as_ref() {
            Some(ClientShellOverlay::FolderMenu(menu)) => folder_menu_items(
                &self.folders,
                &self.endpoints,
                &self.active_endpoint_id,
                menu,
            ),
            _ => Vec::new(),
        }
    }

    /// The footer's ` new`: a workspace/folder menu on machines that
    /// support folders. Returns `false` to fall back to direct creation.
    pub(in crate::client::shell) fn open_folder_new_menu(&mut self) -> bool {
        if !self.active_endpoint_supports_folders() {
            return false;
        }
        let anchor = self.hits.new_workspace;
        let height = 4;
        self.overlay = Some(ClientShellOverlay::FolderMenu(FolderMenuOverlay {
            kind: FolderMenuKind::New,
            x: anchor.x,
            y: anchor.y.saturating_sub(height),
            highlighted: 0,
            parent: None,
        }));
        true
    }

    /// Right-click on a folder header of the active machine.
    pub(in crate::client::shell) fn open_folder_header_menu(&mut self, point: (u16, u16)) -> bool {
        let Some(hit) = self
            .hits
            .folder_headers
            .iter()
            .find(|hit| super::super::contains(hit.rect, point))
        else {
            return false;
        };
        if hit.endpoint_id != self.active_endpoint_id {
            return true;
        }
        self.overlay = Some(ClientShellOverlay::FolderMenu(FolderMenuOverlay {
            kind: FolderMenuKind::Header {
                folder_id: hit.folder_id.clone(),
            },
            x: point.0,
            y: point.1,
            highlighted: 0,
            parent: None,
        }));
        true
    }

    /// Opens the "Move to folder ▸" submenu when `index` is that item of the
    /// open workspace menu. Returns whether it did.
    pub(in crate::client::shell) fn open_folder_submenu(&mut self, index: usize) -> bool {
        let Some(ClientShellOverlay::ContextMenu(menu)) = self.overlay.as_ref() else {
            return false;
        };
        let is_move = menu
            .items()
            .get(index)
            .is_some_and(|item| item.action == ClientContextMenuAction::MoveToFolder);
        let ClientContextMenuTarget::Workspace { workspace_id, .. } = &menu.target else {
            return false;
        };
        if !is_move {
            return false;
        }
        let workspace_id = workspace_id.clone();
        let Some(ClientShellOverlay::ContextMenu(mut parent)) = self.overlay.take() else {
            return false;
        };
        parent.highlighted = index;
        let mut submenu = FolderMenuOverlay {
            kind: FolderMenuKind::MoveTo { workspace_id },
            x: 0,
            y: 0,
            highlighted: 0,
            parent: Some(parent),
        };
        let items = folder_menu_items(
            &self.folders,
            &self.endpoints,
            &self.active_endpoint_id,
            &submenu,
        );
        submenu.highlighted = first_enabled(&items, 0, 1).unwrap_or(0);
        self.overlay = Some(ClientShellOverlay::FolderMenu(submenu));
        true
    }

    /// Back from a submenu to its workspace menu, or closes the menu.
    fn close_folder_menu_level(&mut self) {
        self.overlay = match self.overlay.take() {
            Some(ClientShellOverlay::FolderMenu(FolderMenuOverlay {
                parent: Some(parent),
                ..
            })) => Some(ClientShellOverlay::ContextMenu(parent)),
            _ => None,
        };
    }

    fn move_folder_menu_selection(&mut self, delta: isize) {
        let items = self.current_folder_menu_items();
        let Some(ClientShellOverlay::FolderMenu(menu)) = self.overlay.as_mut() else {
            return;
        };
        let start = menu.highlighted as isize + delta;
        if let Some(next) = first_enabled(&items, start, delta) {
            menu.highlighted = next;
        }
    }

    pub(in crate::client::shell) fn handle_folder_menu_key(
        &mut self,
        key: &crate::input::TerminalKey,
        outcome: &mut ClientShellInput,
    ) {
        match key.code {
            KeyCode::Esc | KeyCode::Left => self.close_folder_menu_level(),
            KeyCode::Up | KeyCode::Char('k') => self.move_folder_menu_selection(-1),
            KeyCode::Down | KeyCode::Char('j') => self.move_folder_menu_selection(1),
            KeyCode::Enter => {
                let highlighted = match self.overlay.as_ref() {
                    Some(ClientShellOverlay::FolderMenu(menu)) => menu.highlighted,
                    _ => return,
                };
                self.activate_folder_menu_item(highlighted, outcome);
            }
            _ => {}
        }
        outcome.repaint = true;
    }

    pub(in crate::client::shell) fn handle_folder_menu_mouse(
        &mut self,
        mouse: &crossterm::event::MouseEvent,
        point: (u16, u16),
        outcome: &mut ClientShellInput,
    ) {
        let row_hit = self
            .hits
            .folder_menu_rows
            .iter()
            .find(|(rect, _)| super::super::contains(*rect, point))
            .map(|(_, index)| *index);
        let parent_hit = self
            .hits
            .context_menu_rows
            .iter()
            .find(|(rect, _)| super::super::contains(*rect, point))
            .map(|(_, index)| *index);
        let items = self.current_folder_menu_items();
        match mouse.kind {
            MouseEventKind::Moved => {
                if let Some(index) =
                    row_hit.filter(|index| items.get(*index).is_some_and(|item| item.enabled))
                {
                    if let Some(ClientShellOverlay::FolderMenu(menu)) = self.overlay.as_mut() {
                        menu.highlighted = index;
                    }
                } else if let Some(index) = parent_hit {
                    let on_move_item = matches!(
                        self.overlay.as_ref(),
                        Some(ClientShellOverlay::FolderMenu(FolderMenuOverlay {
                            parent: Some(parent),
                            ..
                        })) if parent.highlighted == index
                    );
                    if !on_move_item {
                        self.close_folder_menu_level();
                        if let Some(ClientShellOverlay::ContextMenu(menu)) = self.overlay.as_mut() {
                            menu.highlighted = index;
                        }
                    }
                }
            }
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some(index) = row_hit {
                    if items.get(index).is_some_and(|item| item.enabled) {
                        self.activate_folder_menu_item(index, outcome);
                    }
                } else if let Some(index) = parent_hit {
                    self.close_folder_menu_level();
                    self.activate_context_menu_item(index, outcome);
                } else if self.hits.folder_menu_area.is_empty()
                    || !super::super::contains(self.hits.folder_menu_area, point)
                {
                    self.overlay = None;
                }
            }
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                let delta = if mouse.kind == MouseEventKind::ScrollUp {
                    -1
                } else {
                    1
                };
                self.move_folder_menu_selection(delta);
            }
            _ => {}
        }
        outcome.repaint = true;
    }

    fn activate_folder_menu_item(&mut self, index: usize, outcome: &mut ClientShellInput) {
        let items = self.current_folder_menu_items();
        let Some(ClientShellOverlay::FolderMenu(menu)) = self.overlay.take() else {
            return;
        };
        let Some(item) = items.get(index).filter(|item| item.enabled) else {
            self.overlay = Some(ClientShellOverlay::FolderMenu(menu));
            return;
        };
        let folder_name = |id: &str| {
            self.active_folder_view().and_then(|view| {
                view.layout
                    .folders
                    .iter()
                    .find(|folder| folder.id == id)
                    .map(|folder| folder.name.clone())
            })
        };
        match (&menu.kind, item.action.clone()) {
            (_, FolderMenuAction::NewWorkspace) => {
                self.record_binding(
                    crate::input::KeybindMatch::Action(crate::input::KeybindAction::NewWorkspace),
                    outcome,
                );
            }
            (_, FolderMenuAction::NewFolder) => self.open_folder_prompt(
                "new folder",
                "",
                FolderPrompt::Create { workspace_id: None },
            ),
            (FolderMenuKind::Header { folder_id }, FolderMenuAction::Rename) => {
                let name = folder_name(folder_id).unwrap_or_default();
                self.open_folder_prompt(
                    "rename folder",
                    &name,
                    FolderPrompt::Rename {
                        folder_id: folder_id.clone(),
                    },
                );
            }
            (FolderMenuKind::Header { folder_id }, FolderMenuAction::ToggleCollapse) => {
                let endpoint_id = self.active_endpoint_id.clone();
                self.folders.toggle_folder(&endpoint_id, folder_id);
                self.persist_chrome_preferences(outcome);
            }
            (FolderMenuKind::Header { folder_id }, FolderMenuAction::NewWorkspaceHere) => {
                self.create_workspace_in_folder(folder_id.clone(), outcome);
            }
            (FolderMenuKind::Header { folder_id }, FolderMenuAction::Delete) => {
                self.push_endpoint_method(
                    Method::FolderDelete(FolderTarget {
                        folder_id: folder_id.clone(),
                    }),
                    outcome,
                );
            }
            (FolderMenuKind::MoveTo { workspace_id }, FolderMenuAction::MoveTo(folder_id)) => {
                self.push_endpoint_method(
                    Method::FolderAddWorkspace(FolderAddWorkspaceParams {
                        folder_id,
                        workspace_id: workspace_id.clone(),
                        before: None,
                        after: None,
                    }),
                    outcome,
                );
            }
            (FolderMenuKind::MoveTo { workspace_id }, FolderMenuAction::MoveToNewFolder) => self
                .open_folder_prompt(
                    "new folder",
                    "",
                    FolderPrompt::Create {
                        workspace_id: Some(workspace_id.clone()),
                    },
                ),
            (FolderMenuKind::MoveTo { workspace_id }, FolderMenuAction::RemoveFromFolder) => {
                self.push_endpoint_method(
                    Method::FolderRemoveWorkspace(FolderRemoveWorkspaceParams {
                        workspace_id: workspace_id.clone(),
                        before: None,
                        after: None,
                    }),
                    outcome,
                );
            }
            _ => {}
        }
        outcome.repaint = true;
    }

    fn open_folder_prompt(&mut self, title: &'static str, text: &str, prompt: FolderPrompt) {
        self.overlay = Some(ClientShellOverlay::Rename(ClientRenameOverlay {
            title,
            input: TextEditor::new(text, false),
            target: ClientRenameTarget::Folder(prompt),
        }));
    }

    /// "New workspace here": the usual creation flow, filed at the folder's end.
    fn create_workspace_in_folder(&mut self, folder_id: String, outcome: &mut ClientShellInput) {
        if self.config.prompt_new_workspace_name {
            self.open_new_workspace_overlay();
            if let Some(ClientShellOverlay::Rename(rename)) = self.overlay.as_mut() {
                if let ClientRenameTarget::NewWorkspace {
                    source_workspace_id,
                    cwd,
                    suggested_name,
                } = std::mem::replace(
                    &mut rename.target,
                    ClientRenameTarget::Folder(FolderPrompt::Create { workspace_id: None }),
                ) {
                    rename.target = ClientRenameTarget::Folder(FolderPrompt::NewWorkspace {
                        folder_id,
                        source_workspace_id,
                        cwd,
                        suggested_name,
                    });
                }
            }
            return;
        }
        self.push_endpoint_method(
            Method::FolderCreateWorkspace(FolderCreateWorkspaceParams {
                folder_id,
                workspace: WorkspaceCreateParams {
                    source_workspace_id: self.workspace_action_id(),
                    cwd: None,
                    focus: true,
                    label: None,
                    env: Default::default(),
                },
            }),
            outcome,
        );
    }

    /// Enter in a folder prompt. A new folder needs a name, so an empty one
    /// keeps the prompt open; an empty rename changes nothing.
    pub(in crate::client::shell) fn save_folder_prompt(&mut self, outcome: &mut ClientShellInput) {
        outcome.repaint = true;
        let empty_create = matches!(
            self.overlay.as_ref(),
            Some(ClientShellOverlay::Rename(ClientRenameOverlay {
                target: ClientRenameTarget::Folder(FolderPrompt::Create { .. }),
                input,
                ..
            })) if input.trim().is_empty()
        );
        if empty_create {
            return;
        }
        let Some(ClientShellOverlay::Rename(rename)) = self.overlay.take() else {
            return;
        };
        let name = rename.input.trim().to_owned();
        let ClientRenameTarget::Folder(prompt) = rename.target else {
            return;
        };
        let method = match prompt {
            FolderPrompt::Create { workspace_id } => {
                Some(Method::FolderCreate(FolderCreateParams {
                    name,
                    workspace_ids: workspace_id.into_iter().collect(),
                    before: None,
                    after: None,
                }))
            }
            FolderPrompt::Rename { folder_id } => (!name.is_empty())
                .then_some(Method::FolderRename(FolderRenameParams { folder_id, name })),
            FolderPrompt::NewWorkspace {
                folder_id,
                source_workspace_id,
                cwd,
                suggested_name,
            } => Some(Method::FolderCreateWorkspace(FolderCreateWorkspaceParams {
                folder_id,
                workspace: WorkspaceCreateParams {
                    source_workspace_id,
                    cwd,
                    focus: true,
                    label: (!name.is_empty() && name != suggested_name).then_some(name),
                    env: Default::default(),
                },
            })),
        };
        if let Some(method) = method {
            self.push_endpoint_method(method, outcome);
        }
    }

    /// Whether a workspace's menu offers "Move to folder ▸": machines with
    /// folders, for every workspace except worktree-group children.
    pub(in crate::client::shell) fn workspace_can_move_to_folder(
        &self,
        snapshot: &ClientShellSnapshot,
        workspace: &ClientShellWorkspace,
    ) -> bool {
        if !self.active_endpoint_supports_folders() {
            return false;
        }
        let group_child = workspace.worktree.as_ref().is_some_and(|worktree| {
            worktree.is_linked_worktree
                && snapshot.workspaces.iter().any(|candidate| {
                    candidate.worktree.as_ref().is_some_and(|candidate| {
                        candidate.key == worktree.key && !candidate.is_linked_worktree
                    })
                })
        });
        !group_child
    }
}

/// The first enabled item at or after `start` moving by `step`, if any.
fn first_enabled(items: &[FolderMenuItem], start: isize, step: isize) -> Option<usize> {
    let step = if step < 0 { -1 } else { 1 };
    let mut index = start;
    while index >= 0 && (index as usize) < items.len() {
        if items[index as usize].enabled {
            return Some(index as usize);
        }
        index += step;
    }
    None
}
