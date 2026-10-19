//! Workspace folders: named, collapsible groups of workspaces owned by each server.
//!
//! The server's flat workspace order stays the source of truth for ordering.
//! Folder state adds membership, folder names, and the combined top-level
//! order of folders and loose workspaces, and keeps every folder's members
//! together in the flat order so clients that know nothing about folders still
//! see them side by side.

pub(crate) mod model;
pub(crate) mod protocol;
pub(crate) mod store;

pub(crate) use model::{
    parse_folder_id, public_folder_id, FolderError, FolderId, FolderOutcome, FolderRef,
    FolderState, MembershipChange, Placement, WorkspaceFact, WorktreeFact,
};
