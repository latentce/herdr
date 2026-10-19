//! API handlers and server upkeep for workspace folders.

use crate::api::schema::{EventData, EventEnvelope, EventKind, FolderInfo, Method, ResponseResult};
use crate::folders::model::normalize_folder_name;
use crate::folders::{
    parse_folder_id, public_folder_id, FolderError, FolderId, FolderOutcome, FolderRef,
    MembershipChange, Placement, WorkspaceFact, WorktreeFact,
};

use super::responses::{encode_error, encode_success};
use crate::app::App;

fn workspace_facts(workspaces: &[crate::workspace::Workspace]) -> Vec<WorkspaceFact> {
    workspaces
        .iter()
        .map(|workspace| WorkspaceFact {
            id: workspace.id.clone(),
            worktree: workspace.worktree_space.as_ref().map(|space| WorktreeFact {
                key: space.key.clone(),
                linked: space.is_linked_worktree,
            }),
        })
        .collect()
}

impl App {
    /// Folds workspace-list changes into folder state: closed workspaces
    /// leave their folders, worktree groups are placed, and every folder's
    /// members are kept together. Returns whether anything changed.
    pub(crate) fn sync_folders(&mut self) -> bool {
        self.ensure_folders_loaded();
        let synced = self
            .state
            .folders
            .is_synced(self.state.workspaces.iter().map(|workspace| {
                (
                    workspace.id.as_str(),
                    workspace
                        .worktree_space
                        .as_ref()
                        .map(|space| (space.key.as_str(), space.is_linked_worktree)),
                )
            }));
        if synced {
            return false;
        }
        let outcome = self
            .state
            .folders
            .reconcile(workspace_facts(&self.state.workspaces));
        let changed = outcome.changed;
        let (membership, reordered) = self.apply_folder_outcome(outcome);
        let any_membership = !membership.is_empty();
        self.emit_folder_followups(membership, reordered);
        changed || reordered || any_membership
    }

    fn ensure_folders_loaded(&mut self) {
        let storage = &mut self.state.folders.storage;
        if storage.loaded {
            return;
        }
        storage.loaded = true;
        if cfg!(not(test)) && self.policy.persist_session && storage.path.is_none() {
            storage.path = Some(crate::folders::store::session_folder_path());
        }
        let Some(path) = storage.path.clone() else {
            return;
        };
        match crate::folders::store::load(&mut self.state.folders, &path) {
            Ok(found) => {
                self.state.folders.storage.has_file = found;
                self.keep_folders_naming_missing_workspaces(&path);
            }
            Err(err) => {
                tracing::warn!(path = %path.display(), err = %err, "failed to read workspace folders");
            }
        }
    }

    /// The first reconcile drops members that did not come back with the
    /// session. Keeps the loaded file aside first, so the folders can be
    /// recovered along with a recovered session.
    fn keep_folders_naming_missing_workspaces(&self, path: &std::path::Path) {
        let present = self
            .state
            .workspaces
            .iter()
            .map(|workspace| workspace.id.as_str())
            .collect::<std::collections::HashSet<_>>();
        if self
            .state
            .folders
            .member_ids()
            .all(|member| present.contains(member))
        {
            return;
        }
        match crate::folders::store::keep_recovery_copy(path) {
            Ok(recovery) => tracing::info!(
                path = %path.display(),
                recovery = %recovery.display(),
                "saved folders name workspaces that did not come back; kept a recovery copy"
            ),
            Err(err) => tracing::warn!(
                path = %path.display(),
                err = %err,
                "failed to keep a recovery copy of workspace folders"
            ),
        }
    }

    fn save_folders(&mut self) {
        let storage = &self.state.folders.storage;
        if storage.path.is_none() || (self.state.folders.is_empty() && !storage.has_file) {
            return;
        }
        let file = crate::folders::store::FolderFile::from_state(&self.state.folders);
        self.state.folders.storage.queue(file);
    }

    /// Waits for folder data to reach disk, before the server exits or
    /// hands off.
    pub(crate) fn flush_folder_save(&mut self) {
        self.state.folders.storage.flush();
    }

    /// Applies a new flat order and saves folder data. Returns the membership
    /// changes and whether workspaces were reordered.
    fn apply_folder_outcome(&mut self, outcome: FolderOutcome) -> (Vec<MembershipChange>, bool) {
        let reordered = outcome
            .flat
            .as_ref()
            .is_some_and(|flat| self.state.move_workspace_block(flat, None));
        if outcome.changed || reordered {
            self.save_folders();
        }
        (outcome.membership, reordered)
    }

    fn emit_folder_followups(&mut self, membership: Vec<MembershipChange>, reordered: bool) {
        if !membership.is_empty() {
            let folders = self.folder_infos();
            for change in membership {
                self.emit_event(EventEnvelope {
                    event: EventKind::WorkspaceFolderChanged,
                    data: EventData::WorkspaceFolderChanged {
                        workspace_id: change.workspace_id,
                        folder_id: change.to.map(public_folder_id),
                        previous_folder_id: change.from.map(public_folder_id),
                        folders: folders.clone(),
                    },
                });
            }
        }
        if reordered {
            let workspaces = (0..self.state.workspaces.len())
                .map(|index| self.workspace_info(index))
                .collect::<Vec<_>>();
            self.emit_event(EventEnvelope {
                event: EventKind::WorkspaceReordered,
                data: EventData::WorkspaceReordered {
                    workspace_ids: workspaces
                        .iter()
                        .map(|workspace| workspace.workspace_id.clone())
                        .collect(),
                    before_workspace_id: None,
                    workspaces,
                },
            });
        }
    }

    pub(super) fn folder_infos(&self) -> Vec<FolderInfo> {
        self.state
            .folders
            .folders_in_order()
            .map(|(position, folder)| FolderInfo {
                folder_id: public_folder_id(folder.id),
                name: folder.name.clone(),
                position,
                workspace_ids: folder.members.clone(),
            })
            .collect()
    }

    fn folder_info(&self, id: FolderId) -> Option<FolderInfo> {
        self.folder_infos()
            .into_iter()
            .find(|folder| folder.folder_id == public_folder_id(id))
    }

    fn folder_order_ids(&self) -> Vec<String> {
        self.state
            .folders
            .order()
            .iter()
            .map(|item| match item {
                crate::folders::model::Item::Workspace(id) => id.clone(),
                crate::folders::model::Item::Folder(id) => public_folder_id(*id),
            })
            .collect()
    }

    fn resolve_folder(&self, raw: &str) -> Result<FolderId, FolderError> {
        parse_folder_id(raw)
            .filter(|id| self.state.folders.folder(*id).is_some())
            .ok_or_else(|| FolderError {
                code: "folder_not_found",
                message: format!("folder {raw} not found"),
            })
    }

    fn resolve_folder_workspace(&self, raw: &str) -> Result<String, FolderError> {
        self.parse_workspace_id(raw)
            .and_then(|index| self.state.workspaces.get(index))
            .map(|workspace| workspace.id.clone())
            .ok_or_else(|| FolderError::workspace_not_found(raw))
    }

    fn resolve_folder_ref(&self, raw: &str) -> Result<FolderRef, FolderError> {
        if raw.trim().starts_with('f') {
            return self.resolve_folder(raw).map(FolderRef::Folder);
        }
        self.resolve_folder_workspace(raw).map(FolderRef::Workspace)
    }

    fn resolve_placement(
        &self,
        before: Option<&str>,
        after: Option<&str>,
    ) -> Result<Option<Placement>, FolderError> {
        match (before, after) {
            (Some(_), Some(_)) => Err(FolderError::invalid_position(
                "give either before or after, not both",
            )),
            (Some(before), None) => self
                .resolve_folder_ref(before)
                .map(|reference| Some(Placement::Before(reference))),
            (None, Some(after)) => self
                .resolve_folder_ref(after)
                .map(|reference| Some(Placement::After(reference))),
            (None, None) => Ok(None),
        }
    }

    pub(super) fn handle_folder_method(&mut self, id: String, method: Method) -> String {
        if let Method::FolderCreateWorkspace(params) = method {
            return self.handle_folder_create_workspace(id, params);
        }
        match self.run_folder_method(method) {
            Ok(result) => encode_success(id, result),
            Err(error) => encode_error(id, error.code, error.message),
        }
    }

    fn run_folder_method(&mut self, method: Method) -> Result<ResponseResult, FolderError> {
        match method {
            Method::FolderList(_) => Ok(ResponseResult::FolderList {
                folders: self.folder_infos(),
                order: self.folder_order_ids(),
            }),
            Method::FolderGet(target) => {
                let folder = self.resolve_folder(&target.folder_id)?;
                self.folder_result(folder)
            }
            Method::FolderCreate(params) => {
                let name =
                    normalize_folder_name(&params.name).ok_or_else(FolderError::invalid_name)?;
                let workspaces = params
                    .workspace_ids
                    .iter()
                    .map(|workspace| self.resolve_folder_workspace(workspace))
                    .collect::<Result<Vec<_>, _>>()?;
                let placement =
                    self.resolve_placement(params.before.as_deref(), params.after.as_deref())?;
                let (folder, outcome) = self.state.folders.create(name, &workspaces, placement)?;
                let (membership, reordered) = self.apply_folder_outcome(outcome);
                if let Some(info) = self.folder_info(folder) {
                    let folders = self.folder_infos();
                    self.emit_event(EventEnvelope {
                        event: EventKind::FolderCreated,
                        data: EventData::FolderCreated {
                            folder: info,
                            folders,
                        },
                    });
                }
                self.emit_folder_followups(membership, reordered);
                self.folder_result(folder)
            }
            Method::FolderRename(params) => {
                let folder = self.resolve_folder(&params.folder_id)?;
                let name =
                    normalize_folder_name(&params.name).ok_or_else(FolderError::invalid_name)?;
                if self.state.folders.rename(folder, name.clone())? {
                    self.save_folders();
                    let folders = self.folder_infos();
                    self.emit_event(EventEnvelope {
                        event: EventKind::FolderRenamed,
                        data: EventData::FolderRenamed {
                            folder_id: public_folder_id(folder),
                            name,
                            folders,
                        },
                    });
                }
                self.folder_result(folder)
            }
            Method::FolderDelete(target) => {
                let folder = self.resolve_folder(&target.folder_id)?;
                let workspace_ids = self
                    .state
                    .folders
                    .folder(folder)
                    .map(|folder| folder.members.clone())
                    .unwrap_or_default();
                let outcome = self.state.folders.delete(folder)?;
                let (membership, reordered) = self.apply_folder_outcome(outcome);
                let folders = self.folder_infos();
                self.emit_event(EventEnvelope {
                    event: EventKind::FolderDeleted,
                    data: EventData::FolderDeleted {
                        folder_id: public_folder_id(folder),
                        workspace_ids,
                        folders,
                    },
                });
                self.emit_folder_followups(membership, reordered);
                Ok(ResponseResult::FolderList {
                    folders: self.folder_infos(),
                    order: self.folder_order_ids(),
                })
            }
            Method::FolderMove(params) => {
                let folder = self.resolve_folder(&params.folder_id)?;
                let placement = self
                    .resolve_placement(params.before.as_deref(), params.after.as_deref())?
                    .ok_or_else(|| {
                        FolderError::invalid_position("folder.move needs before or after")
                    })?;
                let outcome = self.state.folders.move_folder(folder, placement)?;
                let moved = outcome.changed;
                let (membership, reordered) = self.apply_folder_outcome(outcome);
                if moved {
                    let folders = self.folder_infos();
                    self.emit_event(EventEnvelope {
                        event: EventKind::FolderMoved,
                        data: EventData::FolderMoved {
                            folder_id: public_folder_id(folder),
                            folders,
                        },
                    });
                }
                self.emit_folder_followups(membership, reordered);
                self.folder_result(folder)
            }
            Method::FolderAddWorkspace(params) => {
                let folder = self.resolve_folder(&params.folder_id)?;
                let workspace = self.resolve_folder_workspace(&params.workspace_id)?;
                let placement =
                    self.resolve_placement(params.before.as_deref(), params.after.as_deref())?;
                let outcome = self
                    .state
                    .folders
                    .add_workspace(folder, &workspace, placement)?;
                let (membership, reordered) = self.apply_folder_outcome(outcome);
                self.emit_folder_followups(membership, reordered);
                self.folder_result(folder)
            }
            Method::FolderRemoveWorkspace(params) => {
                let workspace = self.resolve_folder_workspace(&params.workspace_id)?;
                let placement =
                    self.resolve_placement(params.before.as_deref(), params.after.as_deref())?;
                let outcome = self.state.folders.remove_workspace(&workspace, placement)?;
                let (membership, reordered) = self.apply_folder_outcome(outcome);
                self.emit_folder_followups(membership, reordered);
                let index = self
                    .state
                    .workspaces
                    .iter()
                    .position(|candidate| candidate.id == workspace)
                    .ok_or_else(|| FolderError::workspace_not_found(&workspace))?;
                Ok(ResponseResult::WorkspaceInfo {
                    workspace: self.workspace_info(index),
                })
            }
            _ => Err(FolderError {
                code: "not_implemented",
                message: "not a folder method".into(),
            }),
        }
    }

    /// Creates a workspace and files it at the end of a folder. An unknown
    /// folder fails before anything is created.
    fn handle_folder_create_workspace(
        &mut self,
        id: String,
        params: crate::api::schema::FolderCreateWorkspaceParams,
    ) -> String {
        let folder = match self.resolve_folder(&params.folder_id) {
            Ok(folder) => folder,
            Err(error) => return encode_error(id, error.code, error.message),
        };
        let count = self.state.workspaces.len();
        let response = self.handle_workspace_create(id.clone(), params.workspace);
        if self.state.workspaces.len() <= count {
            return response;
        }
        let Some(workspace) = self
            .state
            .workspaces
            .last()
            .map(|workspace| workspace.id.clone())
        else {
            return response;
        };
        self.sync_folders();
        match self.state.folders.add_workspace(folder, &workspace, None) {
            Ok(outcome) => {
                let (membership, reordered) = self.apply_folder_outcome(outcome);
                self.emit_folder_followups(membership, reordered);
            }
            Err(error) => {
                tracing::warn!(
                    workspace = %workspace,
                    code = error.code,
                    err = %error.message,
                    "created workspace could not join its folder"
                );
            }
        }
        match self
            .state
            .workspaces
            .iter()
            .position(|candidate| candidate.id == workspace)
            .and_then(|index| self.workspace_created_result(index))
        {
            Some(result) => encode_success(id, result),
            None => response,
        }
    }

    fn folder_result(&self, folder: FolderId) -> Result<ResponseResult, FolderError> {
        self.folder_info(folder)
            .map(|folder| ResponseResult::FolderInfo { folder })
            .ok_or_else(|| FolderError::folder_not_found(folder))
    }

    /// Clamps a legacy `workspace.move` so it never changes folder membership.
    pub(super) fn clamp_folder_workspace_move(&self, source: usize, insert_index: usize) -> usize {
        if self.state.folders.is_empty() || source >= self.state.workspaces.len() {
            return insert_index;
        }
        let moving = self.state.workspaces[source].id.as_str();
        let remaining = self
            .state
            .workspaces
            .iter()
            .filter(|workspace| workspace.id != moving)
            .map(|workspace| workspace.id.as_str())
            .collect::<Vec<_>>();
        let target = if source < insert_index {
            insert_index - 1
        } else {
            insert_index
        };
        let clamped = self
            .state
            .folders
            .clamp_flat_insert(&remaining, &[moving], target);
        if clamped >= source {
            clamped + 1
        } else {
            clamped
        }
    }

    /// Clamps a legacy `workspace.move_block` so it never changes folder membership.
    pub(super) fn clamp_folder_block_move(
        &self,
        workspace_ids: &[String],
        before_workspace_id: Option<String>,
    ) -> Option<String> {
        if self.state.folders.is_empty() {
            return before_workspace_id;
        }
        let moving = workspace_ids.iter().map(String::as_str).collect::<Vec<_>>();
        let remaining = self
            .state
            .workspaces
            .iter()
            .map(|workspace| workspace.id.as_str())
            .filter(|id| !moving.contains(id))
            .collect::<Vec<_>>();
        let target = before_workspace_id
            .as_deref()
            .and_then(|before| remaining.iter().position(|id| *id == before))
            .unwrap_or(remaining.len());
        let clamped = self
            .state
            .folders
            .clamp_flat_insert(&remaining, &moving, target);
        remaining.get(clamped).map(|id| (*id).to_owned())
    }
}

#[cfg(test)]
#[path = "folders_tests.rs"]
mod tests;
