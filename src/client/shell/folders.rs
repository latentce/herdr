//! The TUI side of workspace folders.
//!
//! Folder layouts arrive per machine as an endpoint companion. This module
//! keeps them, the client-local collapse state, and the agents panel's
//! `folders` mode, and renders folder rows through small hooks in the
//! upstream sidebar, mouse, and navigation code.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

use super::render::{display_width, put_text};
use super::*;
use crate::folders::protocol::{EndpointFolderLayout, FolderLayout, FOLDER_SUPPORT_METHOD};
use crossterm::event::{MouseButton, MouseEventKind};

mod agents;
pub(super) mod drag;
mod entries;
pub(super) mod menus;
mod navigation;
pub(super) mod prefs;
mod rows;

pub(super) use agents::{render_folder_agent_panel, FolderAgentHeader};
pub(super) use drag::FolderDrag;
pub(super) use entries::{
    entries_with_hidden, entry_reveals_focus, folder_sidebar_entries, member_rect, FolderRow,
    FOLDER_INDENT,
};
pub(super) use menus::{folder_menu_items, render_folder_menu, FolderMenuOverlay, FolderPrompt};
pub(super) use navigation::folder_navigate_step;
pub(super) use rows::{
    render_folder_drop_indicator, render_folder_header, FolderHeaderHit, HeaderLook,
};

/// How many recent layouts are kept per machine, so a layout that arrives
/// ahead of its snapshot can still be paired with it.
const RECENT_LAYOUTS: usize = 4;

#[derive(Clone, Debug)]
struct ReceivedLayout {
    generation: Option<u64>,
    boot_id: String,
    revision: u64,
    layout: Arc<FolderLayout>,
    member_of: Arc<HashMap<String, usize>>,
}

/// An empty layout for machines that support folders but have none yet.
fn empty_layout() -> (Arc<FolderLayout>, Arc<HashMap<String, usize>>) {
    (Arc::new(FolderLayout::default()), Arc::new(HashMap::new()))
}

/// A folder header press that becomes a collapse toggle on release, or a
/// folder drag once the pointer moves.
#[derive(Clone, Debug)]
pub(super) struct FolderPress {
    pub(super) endpoint_id: ClientEndpointId,
    pub(super) folder_id: String,
    pub(super) start_column: u16,
    pub(super) start_row: u16,
}

/// Agents panel mode. `Grouped` and `Priority` are upstream's modes; the
/// fork adds `Folders`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum AgentPanelMode {
    #[default]
    Grouped,
    Priority,
    Folders,
}

impl AgentPanelMode {
    pub(super) fn from_config(mode: crate::config::AgentPanelModeConfig) -> Self {
        match mode {
            crate::config::AgentPanelModeConfig::Grouped => Self::Grouped,
            crate::config::AgentPanelModeConfig::Priority => Self::Priority,
            crate::config::AgentPanelModeConfig::Folders => Self::Folders,
        }
    }

    pub(super) fn from_sort(sort: crate::config::AgentPanelSortConfig) -> Self {
        match sort {
            crate::config::AgentPanelSortConfig::Spaces => Self::Grouped,
            crate::config::AgentPanelSortConfig::Priority => Self::Priority,
        }
    }

    /// The upstream sort that drives flat agent lists in this mode.
    pub(super) fn upstream_sort(self) -> crate::config::AgentPanelSortConfig {
        match self {
            Self::Grouped | Self::Folders => crate::config::AgentPanelSortConfig::Spaces,
            Self::Priority => crate::config::AgentPanelSortConfig::Priority,
        }
    }

    pub(super) fn next(self) -> Self {
        match self {
            Self::Grouped => Self::Priority,
            Self::Priority => Self::Folders,
            Self::Folders => Self::Grouped,
        }
    }

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Grouped => "grouped",
            Self::Priority => "priority",
            Self::Folders => "folders",
        }
    }
}

#[derive(Default)]
pub(crate) struct FolderUiState {
    layouts: HashMap<ClientEndpointId, VecDeque<ReceivedLayout>>,
    /// Collapsed folder ids per machine; also drives the agents panel tree.
    pub(super) collapsed: HashMap<ClientEndpointId, HashSet<String>>,
    /// Collapsed workspace headers in the agents panel, per machine.
    pub(super) collapsed_agent_workspaces: HashMap<ClientEndpointId, HashSet<String>>,
    pub(super) agent_mode: AgentPanelMode,
    pub(super) agent_mode_manual: bool,
    /// The sort choice read from upstream's preferences; written back as is.
    pub(super) upstream_saved_sort: Option<crate::config::AgentPanelSortConfig>,
    pub(super) press: Option<FolderPress>,
    pub(super) prefs_path: Option<std::path::PathBuf>,
}

/// What the sidebar renderers need from the folder UI state.
pub(super) struct FolderRenderState<'a> {
    pub(super) ui: &'a FolderUiState,
    pub(super) drag: Option<&'a FolderDrag>,
}

impl FolderRenderState<'_> {
    pub(super) fn view(&self, endpoint: &ClientShellEndpoint) -> Option<FolderView<'_>> {
        self.ui.view(endpoint)
    }

    pub(super) fn look(&self, folder_id: &str, focus_visible: bool, dim: bool) -> HeaderLook {
        HeaderLook {
            focus_visible,
            drop_target: self
                .drag
                .is_some_and(|drag| drag.target_header() == Some(folder_id)),
            dragged: self
                .drag
                .is_some_and(|drag| drag.dragged_folder_id() == Some(folder_id)),
            dim,
        }
    }

    pub(super) fn drop_indicator(&self) -> Option<(u16, u16)> {
        self.drag.and_then(FolderDrag::indicator)
    }
}

/// One machine's folder layout as seen by the sidebar.
#[derive(Clone)]
pub(super) struct FolderView<'a> {
    pub(super) layout: Arc<FolderLayout>,
    member_of: Arc<HashMap<String, usize>>,
    collapsed: Option<&'a HashSet<String>>,
}

impl FolderView<'_> {
    pub(super) fn folder_index_of(&self, workspace_id: &str) -> Option<usize> {
        self.member_of.get(workspace_id).copied()
    }

    pub(super) fn folder_id_of(&self, workspace_id: &str) -> Option<&str> {
        self.folder_index_of(workspace_id)
            .and_then(|index| self.layout.folders.get(index))
            .map(|folder| folder.id.as_str())
    }

    pub(super) fn is_collapsed(&self, folder: usize) -> bool {
        self.layout
            .folders
            .get(folder)
            .zip(self.collapsed)
            .is_some_and(|(folder, collapsed)| collapsed.contains(&folder.id))
    }

    /// Whether the workspace sits inside a collapsed folder.
    pub(super) fn is_hidden(&self, workspace_id: &str) -> bool {
        self.folder_index_of(workspace_id)
            .is_some_and(|folder| self.is_collapsed(folder))
    }

    pub(super) fn has_folders(&self) -> bool {
        !self.layout.folders.is_empty()
    }
}

/// Whether a machine runs a server that supports folders.
pub(super) fn endpoint_supports_folders(endpoint: &ClientShellEndpoint) -> bool {
    endpoint
        .methods
        .as_ref()
        .is_some_and(|methods| methods.contains(FOLDER_SUPPORT_METHOD))
}

impl FolderUiState {
    pub(super) fn receive_layout(
        &mut self,
        endpoint_id: &ClientEndpointId,
        generation: Option<u64>,
        layout: EndpointFolderLayout,
    ) {
        let member_of = layout
            .layout
            .folders
            .iter()
            .enumerate()
            .flat_map(|(index, folder)| {
                folder
                    .workspace_ids
                    .iter()
                    .map(move |workspace| (workspace.clone(), index))
            })
            .collect();
        if let Some(collapsed) = self.collapsed.get_mut(endpoint_id) {
            collapsed.retain(|id| layout.layout.folders.iter().any(|folder| folder.id == *id));
        }
        let recent = self.layouts.entry(endpoint_id.clone()).or_default();
        recent.retain(|received| {
            received.generation == generation && received.boot_id == layout.boot_id
        });
        recent.push_back(ReceivedLayout {
            generation,
            boot_id: layout.boot_id,
            revision: layout.revision,
            layout: Arc::new(layout.layout),
            member_of: Arc::new(member_of),
        });
        while recent.len() > RECENT_LAYOUTS {
            recent.pop_front();
        }
    }

    /// The folder layout that belongs to the machine's current snapshot, or
    /// `None` when the machine does not support folders.
    pub(super) fn view<'a>(&'a self, endpoint: &ClientShellEndpoint) -> Option<FolderView<'a>> {
        if !endpoint_supports_folders(endpoint) {
            return None;
        }
        let snapshot = endpoint.snapshot.as_deref()?;
        let (layout, member_of) = self
            .layouts
            .get(&endpoint.endpoint_id)
            .and_then(|recent| {
                recent.iter().rev().find(|received| {
                    received.generation == endpoint.snapshot_generation
                        && received.boot_id == snapshot.boot_id
                        && received.revision <= snapshot.revision
                })
            })
            .map(|received| (received.layout.clone(), received.member_of.clone()))
            .unwrap_or_else(empty_layout);
        Some(FolderView {
            layout,
            member_of,
            collapsed: self.collapsed.get(&endpoint.endpoint_id),
        })
    }

    pub(super) fn view_for<'a>(
        &'a self,
        endpoints: &'a [ClientShellEndpoint],
        endpoint_id: &ClientEndpointId,
    ) -> Option<FolderView<'a>> {
        endpoints
            .iter()
            .find(|endpoint| &endpoint.endpoint_id == endpoint_id)
            .and_then(|endpoint| self.view(endpoint))
    }

    pub(super) fn is_folder_collapsed(&self, endpoint_id: &ClientEndpointId, folder: &str) -> bool {
        self.collapsed
            .get(endpoint_id)
            .is_some_and(|collapsed| collapsed.contains(folder))
    }

    pub(super) fn toggle_folder(&mut self, endpoint_id: &ClientEndpointId, folder: &str) {
        let collapsed = self.collapsed.entry(endpoint_id.clone()).or_default();
        if !collapsed.remove(folder) {
            collapsed.insert(folder.to_owned());
        }
    }

    pub(super) fn toggle_agent_workspace(
        &mut self,
        endpoint_id: &ClientEndpointId,
        workspace_id: &str,
    ) {
        let collapsed = self
            .collapsed_agent_workspaces
            .entry(endpoint_id.clone())
            .or_default();
        if !collapsed.remove(workspace_id) {
            collapsed.insert(workspace_id.to_owned());
        }
    }

    /// Forgets agents-panel collapse for workspaces that closed. Returns
    /// whether anything was dropped.
    pub(super) fn prune_closed_workspaces(
        &mut self,
        endpoint_id: &ClientEndpointId,
        snapshot: &ClientShellSnapshot,
    ) -> bool {
        let Some(collapsed) = self.collapsed_agent_workspaces.get_mut(endpoint_id) else {
            return false;
        };
        let before = collapsed.len();
        collapsed.retain(|id| {
            snapshot
                .workspaces
                .iter()
                .any(|workspace| workspace.workspace_id == *id)
        });
        collapsed.len() != before
    }

    pub(super) fn retire_endpoint(&mut self, endpoint_id: &ClientEndpointId) {
        self.layouts.remove(endpoint_id);
    }
}

impl ClientShellState {
    /// Stores a machine's folder layout.
    pub(crate) fn set_endpoint_folder_layout(
        &mut self,
        endpoint_id: &ClientEndpointId,
        generation: u64,
        layout: EndpointFolderLayout,
    ) {
        self.folders
            .receive_layout(endpoint_id, Some(generation), layout);
    }

    pub(super) fn active_folder_view(&self) -> Option<FolderView<'_>> {
        self.folders
            .view_for(&self.endpoints, &self.active_endpoint_id)
    }

    /// Whether the active machine supports folders.
    pub(super) fn active_endpoint_supports_folders(&self) -> bool {
        self.endpoints
            .iter()
            .find(|endpoint| endpoint.endpoint_id == self.active_endpoint_id)
            .is_some_and(endpoint_supports_folders)
    }
}
