use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct FolderCreateParams {
    /// Display name; duplicates allowed, empty/whitespace-only rejected.
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct FolderAssignParams {
    pub workspace_id: String,
    /// Target folder, or `null` to return the workspace to the top level.
    /// Append semantics; assigning any worktree family member moves the
    /// whole family.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folder_id: Option<String>,
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
