//! Folder methods and payloads. Folders are named groups of workspaces on one
//! machine; positions name a neighbour in the combined order of folders and
//! loose workspaces, or a member of the target folder.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct FolderInfo {
    /// Stable folder ID such as `f1`. IDs are never reused on a machine.
    pub folder_id: String,
    pub name: String,
    /// Zero-based index in the combined order of folders and loose workspaces.
    pub position: usize,
    /// Member workspaces in sidebar order.
    pub workspace_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct FolderTarget {
    pub folder_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct FolderCreateParams {
    /// Required; surrounding whitespace is trimmed.
    pub name: String,
    /// Starting members, kept in the order listed. Each one brings its whole
    /// worktree group, and workspaces already in a folder are moved.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub workspace_ids: Vec<String>,
    /// Place the folder before this folder or loose workspace.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<String>,
    /// Place the folder after this folder or loose workspace.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct FolderRenameParams {
    pub folder_id: String,
    /// Required; surrounding whitespace is trimmed.
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct FolderMoveParams {
    pub folder_id: String,
    /// Move before this folder or loose workspace.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<String>,
    /// Move after this folder or loose workspace.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct FolderAddWorkspaceParams {
    pub folder_id: String,
    /// Acts on the workspace's whole worktree group.
    pub workspace_id: String,
    /// Place before this member of the folder; defaults to the end.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<String>,
    /// Place after this member of the folder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<String>,
}

/// Creates a workspace directly at the end of a folder. Takes the same
/// fields as `workspace.create`, plus the folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct FolderCreateWorkspaceParams {
    pub folder_id: String,
    #[serde(flatten)]
    pub workspace: super::workspaces::WorkspaceCreateParams,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct FolderRemoveWorkspaceParams {
    /// Acts on the workspace's whole worktree group.
    pub workspace_id: String,
    /// Place before this folder or loose workspace; defaults to right after
    /// the folder the workspace leaves.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<String>,
    /// Place after this folder or loose workspace.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<String>,
}
