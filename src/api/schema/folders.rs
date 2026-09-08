use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct FolderCreateParams {
    /// Display name; duplicates allowed, empty/whitespace-only rejected.
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct FolderRenameParams {
    pub folder_id: String,
    /// New display name; duplicates allowed, empty/whitespace-only rejected.
    pub name: String,
}

/// Parameters addressing a single folder by id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct FolderTarget {
    pub folder_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct FolderAssignParams {
    pub workspace_id: String,
    /// Target folder, or `null` to return the workspace to the top level.
    /// Assigning any worktree family member moves the whole family.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folder_id: Option<String>,
    /// Final index of the assigned block inside the target container — the
    /// folder's member list, or the top-level order (where a folder counts
    /// as one entry) when `folder_id` is `null`. Omitted appends;
    /// out-of-range positions clamp to the end. Assigning to the current
    /// container with a position performs a plain reorder inside it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct FolderMoveParams {
    pub folder_id: String,
    /// The folder's final index among top-level entries (folders and loose
    /// spaces); out-of-range positions clamp to the end.
    pub position: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct FolderInfo {
    pub folder_id: String,
    pub name: String,
    /// Ordered member workspace ids.
    pub members: Vec<String>,
}

/// One entry of the top-level space order: a folder or a loose workspace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SpaceOrderEntryInfo {
    pub kind: SpaceOrderEntryKind,
    /// Folder id or workspace id, matching `kind`.
    pub id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SpaceOrderEntryKind {
    Folder,
    Workspace,
}
