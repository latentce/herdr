//! Folder data sent to TUI clients as an optional endpoint companion.
//!
//! Stock clients ignore the unknown control kind; the frozen snapshot codec is
//! untouched. A client treats a machine as supporting folders when the server
//! advertises [`FOLDER_SUPPORT_METHOD`].

use serde::{Deserialize, Serialize};

use super::model::{public_folder_id, FolderState, Item};
use crate::protocol::ServerMessage;

pub(crate) const FOLDER_LAYOUT_KIND: &str = "endpoint.workspace-folders.v1";
/// Advertised endpoint method that marks a server as supporting folders.
pub(crate) const FOLDER_SUPPORT_METHOD: &str = "folder.create";

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct FolderLayoutFolder {
    pub(crate) id: String,
    pub(crate) name: String,
    /// Members in display order.
    #[serde(default)]
    pub(crate) workspace_ids: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct FolderLayout {
    /// Folders in combined order.
    #[serde(default)]
    pub(crate) folders: Vec<FolderLayoutFolder>,
    /// Combined order of folder ids and loose workspace ids.
    #[serde(default)]
    pub(crate) order: Vec<String>,
}

impl FolderLayout {
    pub(crate) fn from_state(state: &FolderState) -> Self {
        Self {
            folders: state
                .folders_in_order()
                .map(|(_, folder)| FolderLayoutFolder {
                    id: public_folder_id(folder.id),
                    name: folder.name.clone(),
                    workspace_ids: folder.members.clone(),
                })
                .collect(),
            order: state
                .order()
                .iter()
                .map(|item| match item {
                    Item::Workspace(id) => id.clone(),
                    Item::Folder(id) => public_folder_id(*id),
                })
                .collect(),
        }
    }
}

/// A folder layout bound to the snapshot revision it accompanies.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct EndpointFolderLayout {
    pub(crate) boot_id: String,
    pub(crate) revision: u64,
    #[serde(default)]
    pub(crate) layout: FolderLayout,
}

/// What one client connection was last sent.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct SentFolders {
    revision: Option<u64>,
    nonempty: bool,
}

impl SentFolders {
    pub(crate) fn current(state: &FolderState) -> Self {
        Self {
            revision: Some(state.revision()),
            nonempty: !state.is_empty(),
        }
    }

    /// Whether the client needs a new layout. Servers without folders never
    /// send one, so their control stream stays exactly as before.
    pub(crate) fn is_stale(self, state: &FolderState) -> bool {
        self.revision != Some(state.revision()) && (self.nonempty || !state.is_empty())
    }
}

/// The layout message for a client, when it needs one.
pub(crate) fn pending_message(
    state: &FolderState,
    sent: SentFolders,
    boot_id: &str,
    revision: u64,
) -> serde_json::Result<Option<ServerMessage>> {
    if !sent.is_stale(state) {
        return Ok(None);
    }
    let payload = EndpointFolderLayout {
        boot_id: boot_id.to_owned(),
        revision,
        layout: FolderLayout::from_state(state),
    };
    Ok(Some(ServerMessage::EndpointControl {
        kind: FOLDER_LAYOUT_KIND.into(),
        data: serde_json::to_string(&payload)?,
    }))
}
