use crate::api::schema::{
    EventData, EventEnvelope, EventKind, FolderAssignParams, FolderCreateParams, FolderInfo,
    ResponseResult, SpaceOrderEntryInfo, SpaceOrderEntryKind,
};
use crate::app::folders::FolderMutationError;
use crate::app::App;
use crate::folder::SpaceOrderEntry;

use super::responses::{encode_error, encode_success};

impl App {
    pub(super) fn handle_folder_create(
        &mut self,
        id: String,
        params: FolderCreateParams,
    ) -> String {
        match self.state.create_folder(&params.name) {
            Ok(folder_id) => {
                self.schedule_session_save();
                if let Some(folder) = self.state.folder(&folder_id) {
                    let folder = folder_info(folder);
                    self.emit_event(EventEnvelope {
                        event: EventKind::FolderCreated,
                        data: EventData::FolderCreated { folder },
                    });
                }
                encode_success(id, ResponseResult::FolderCreated { folder_id })
            }
            Err(FolderMutationError::EmptyName) => encode_error(
                id,
                "invalid_folder_name",
                "folder name must contain at least one non-whitespace character",
            ),
            Err(other) => folder_mutation_error(id, &other),
        }
    }

    pub(super) fn handle_folder_list(&mut self, id: String) -> String {
        // Normalizing makes implicitly loose workspaces explicit and drops
        // stale references, so clients see the full organizational picture.
        let workspace_ids: Vec<&str> = self
            .state
            .workspaces
            .iter()
            .map(|ws| ws.id.as_str())
            .collect();
        let entries =
            crate::folder::normalized_space_order(&self.state.space_order, &workspace_ids);
        let mut folders = Vec::new();
        let mut order = Vec::new();
        for entry in &entries {
            match entry {
                SpaceOrderEntry::Folder(folder) => {
                    order.push(SpaceOrderEntryInfo {
                        kind: SpaceOrderEntryKind::Folder,
                        id: folder.id.clone(),
                    });
                    folders.push(folder_info(folder));
                }
                SpaceOrderEntry::Workspace(workspace_id) => {
                    order.push(SpaceOrderEntryInfo {
                        kind: SpaceOrderEntryKind::Workspace,
                        id: workspace_id.clone(),
                    });
                }
            }
        }

        encode_success(id, ResponseResult::FolderList { folders, order })
    }

    pub(super) fn handle_folder_assign(
        &mut self,
        id: String,
        params: FolderAssignParams,
    ) -> String {
        // Accept the same workspace-id forms as other workspace methods.
        let Some(index) = self.parse_workspace_id(&params.workspace_id) else {
            return folder_mutation_error(id, &FolderMutationError::WorkspaceNotFound);
        };
        let Some(workspace_id) = self.state.workspaces.get(index).map(|ws| ws.id.clone()) else {
            return folder_mutation_error(id, &FolderMutationError::WorkspaceNotFound);
        };

        match self
            .state
            .assign_workspace_to_folder(&workspace_id, params.folder_id.as_deref())
        {
            Ok(workspace_ids) => {
                self.schedule_session_save();
                self.emit_event(EventEnvelope {
                    event: EventKind::FolderAssigned,
                    data: EventData::FolderAssigned {
                        folder_id: params.folder_id.clone(),
                        workspace_ids: workspace_ids.clone(),
                    },
                });
                encode_success(
                    id,
                    ResponseResult::FolderAssigned {
                        folder_id: params.folder_id,
                        workspace_ids,
                    },
                )
            }
            Err(err) => folder_mutation_error(id, &err),
        }
    }
}

fn folder_info(folder: &crate::folder::Folder) -> FolderInfo {
    FolderInfo {
        folder_id: folder.id.clone(),
        name: folder.name.clone(),
        members: folder.members.clone(),
    }
}

fn folder_mutation_error(id: String, err: &FolderMutationError) -> String {
    match err {
        FolderMutationError::EmptyName => encode_error(
            id,
            "invalid_folder_name",
            "folder name must contain at least one non-whitespace character",
        ),
        FolderMutationError::WorkspaceNotFound => {
            encode_error(id, "workspace_not_found", "workspace not found")
        }
        FolderMutationError::FolderNotFound => {
            encode_error(id, "folder_not_found", "folder not found")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::schema::{ErrorResponse, SuccessResponse};
    use crate::config::Config;
    use crate::workspace::Workspace;

    fn test_app(names: &[&str]) -> App {
        let (_api_tx, api_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut app = App::new(
            &Config::default(),
            true,
            None,
            api_rx,
            crate::api::EventHub::default(),
        );
        app.state.workspaces = names.iter().map(|name| Workspace::test_new(name)).collect();
        app.state.ensure_test_terminals();
        if !app.state.workspaces.is_empty() {
            app.state.active = Some(0);
            app.state.selected = 0;
        }
        app
    }

    fn created_folder_id(response: &str) -> String {
        let success: SuccessResponse = serde_json::from_str(response).expect("success response");
        match success.result {
            ResponseResult::FolderCreated { folder_id } => folder_id,
            other => panic!("expected folder_created, got {other:?}"),
        }
    }

    #[test]
    fn folder_create_rejects_whitespace_only_name() {
        let mut app = test_app(&["one"]);

        let response =
            app.handle_folder_create("req".into(), FolderCreateParams { name: "   ".into() });

        let error: ErrorResponse = serde_json::from_str(&response).expect("error response");
        assert_eq!(error.error.code, "invalid_folder_name");
    }

    #[test]
    fn folder_assign_moves_family_and_reports_all_affected_spaces() {
        let mut app = test_app(&["parent", "child", "other"]);
        for (ws_idx, is_linked) in [(0usize, false), (1usize, true)] {
            app.state.workspaces[ws_idx].worktree_space =
                Some(crate::workspace::WorktreeSpaceMembership {
                    key: "repo-key".into(),
                    label: "herdr".into(),
                    repo_root: "/repo/herdr".into(),
                    checkout_path: if is_linked {
                        "/repo/herdr-issue".into()
                    } else {
                        "/repo/herdr".into()
                    },
                    is_linked_worktree: is_linked,
                });
        }
        let parent = app.state.workspaces[0].id.clone();
        let child = app.state.workspaces[1].id.clone();

        let response = app.handle_folder_create(
            "create".into(),
            FolderCreateParams {
                name: "work".into(),
            },
        );
        let folder_id = created_folder_id(&response);

        let response = app.handle_folder_assign(
            "assign".into(),
            FolderAssignParams {
                workspace_id: child.clone(),
                folder_id: Some(folder_id.clone()),
            },
        );

        let success: SuccessResponse = serde_json::from_str(&response).expect("success response");
        assert_eq!(
            success.result,
            ResponseResult::FolderAssigned {
                folder_id: Some(folder_id.clone()),
                workspace_ids: vec![parent.clone(), child.clone()],
            }
        );
        // Both the created and assigned events reach subscribers.
        let events: Vec<EventKind> = app
            .event_hub
            .events_after(0)
            .into_iter()
            .map(|(_, envelope)| envelope.event)
            .collect();
        assert!(events.contains(&EventKind::FolderCreated));
        assert!(events.contains(&EventKind::FolderAssigned));
        // Workspace info now carries the folder id.
        let parent_idx = app
            .state
            .workspaces
            .iter()
            .position(|ws| ws.id == parent)
            .expect("parent present");
        assert_eq!(
            app.workspace_info(parent_idx).folder_id,
            Some(folder_id.clone())
        );
        // folder.list reproduces the organization: loose space then folder.
        let response = app.handle_folder_list("list".into());
        let success: SuccessResponse = serde_json::from_str(&response).expect("success response");
        let ResponseResult::FolderList { folders, order } = success.result else {
            panic!("expected folder_list");
        };
        assert_eq!(folders.len(), 1);
        assert_eq!(folders[0].folder_id, folder_id);
        assert_eq!(folders[0].members, vec![parent, child]);
        assert_eq!(order.len(), 2);
        assert_eq!(order[0].kind, SpaceOrderEntryKind::Workspace);
        assert_eq!(order[1].kind, SpaceOrderEntryKind::Folder);
        assert_eq!(order[1].id, folder_id);
        app.state.assert_invariants_for_test();
    }

    #[test]
    fn folder_assign_rejects_unknown_targets() {
        let mut app = test_app(&["one"]);
        let workspace_id = app.state.workspaces[0].id.clone();

        let response = app.handle_folder_assign(
            "assign".into(),
            FolderAssignParams {
                workspace_id: "w-missing".into(),
                folder_id: None,
            },
        );
        let error: ErrorResponse = serde_json::from_str(&response).expect("error response");
        assert_eq!(error.error.code, "workspace_not_found");

        let response = app.handle_folder_assign(
            "assign".into(),
            FolderAssignParams {
                workspace_id,
                folder_id: Some("f-missing".into()),
            },
        );
        let error: ErrorResponse = serde_json::from_str(&response).expect("error response");
        assert_eq!(error.error.code, "folder_not_found");
    }
}
