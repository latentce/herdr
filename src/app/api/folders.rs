use crate::api::schema::{
    EventData, EventEnvelope, EventKind, FolderAssignParams, FolderCreateParams, FolderInfo,
    FolderMoveParams, FolderRenameParams, FolderTarget, ResponseResult, SpaceOrderEntryInfo,
    SpaceOrderEntryKind,
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

    pub(super) fn handle_folder_rename(
        &mut self,
        id: String,
        params: FolderRenameParams,
    ) -> String {
        match self.state.rename_folder(&params.folder_id, &params.name) {
            Ok(()) => {
                self.schedule_session_save();
                let Some(folder) = self.state.folder(&params.folder_id) else {
                    // Unreachable in practice (rename validated existence),
                    // but degrade gracefully.
                    return folder_mutation_error(id, &FolderMutationError::FolderNotFound);
                };
                let folder = folder_info(folder);
                self.emit_event(EventEnvelope {
                    event: EventKind::FolderUpdated,
                    data: EventData::FolderUpdated {
                        folder: folder.clone(),
                    },
                });
                encode_success(id, ResponseResult::FolderUpdated { folder })
            }
            Err(err) => folder_mutation_error(id, &err),
        }
    }

    pub(super) fn handle_folder_delete(&mut self, id: String, target: FolderTarget) -> String {
        match self.state.delete_folder(&target.folder_id) {
            Ok(workspace_ids) => {
                self.schedule_session_save();
                self.emit_event(EventEnvelope {
                    event: EventKind::FolderDeleted,
                    data: EventData::FolderDeleted {
                        folder_id: target.folder_id.clone(),
                        workspace_ids: workspace_ids.clone(),
                    },
                });
                encode_success(
                    id,
                    ResponseResult::FolderDeleted {
                        folder_id: target.folder_id,
                        workspace_ids,
                    },
                )
            }
            Err(err) => folder_mutation_error(id, &err),
        }
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
        let previous_folder_id = self
            .state
            .workspace_folder_id(&workspace_id)
            .map(str::to_string);

        match self.state.assign_workspace_to_folder(
            &workspace_id,
            params.folder_id.as_deref(),
            params.position,
        ) {
            Ok(workspace_ids) => {
                self.schedule_session_save();
                // Assigning a space to the folder it is already in reorders
                // the folder's members rather than changing membership, so
                // it is a member-order change: `folder.updated`, not
                // `folder.assigned`.
                let same_folder =
                    params.folder_id.is_some() && params.folder_id == previous_folder_id;
                if same_folder {
                    if let Some(folder) = params
                        .folder_id
                        .as_deref()
                        .and_then(|folder_id| self.state.folder(folder_id))
                    {
                        let folder = folder_info(folder);
                        self.emit_event(EventEnvelope {
                            event: EventKind::FolderUpdated,
                            data: EventData::FolderUpdated { folder },
                        });
                    }
                } else {
                    self.emit_event(EventEnvelope {
                        event: EventKind::FolderAssigned,
                        data: EventData::FolderAssigned {
                            folder_id: params.folder_id.clone(),
                            workspace_ids: workspace_ids.clone(),
                        },
                    });
                }
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

    pub(super) fn handle_folder_move(&mut self, id: String, params: FolderMoveParams) -> String {
        match self.state.move_folder(&params.folder_id, params.position) {
            Ok((position, moved)) => {
                if moved {
                    self.schedule_session_save();
                    self.emit_event(EventEnvelope {
                        event: EventKind::FolderMoved,
                        data: EventData::FolderMoved {
                            folder_id: params.folder_id.clone(),
                            position,
                        },
                    });
                }
                encode_success(
                    id,
                    ResponseResult::FolderMoved {
                        folder_id: params.folder_id,
                        position,
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
    fn collapse_state_is_absent_from_api_payloads() {
        // Per the folder-organization ADR, collapse is per-client visual
        // state: persisted in the session snapshot, but never API-exposed.
        // Do not "fix" this asymmetry.
        let mut app = test_app(&["one", "two"]);
        let member = app.state.workspaces[1].id.clone();
        let response = app.handle_folder_create(
            "create".into(),
            FolderCreateParams {
                name: "work".into(),
            },
        );
        let folder_id = created_folder_id(&response);
        app.handle_folder_assign(
            "assign".into(),
            FolderAssignParams {
                workspace_id: member,
                folder_id: Some(folder_id.clone()),
                position: None,
            },
        );
        app.state.collapsed_folder_ids.insert(folder_id);
        app.state.collapsed_space_keys.insert("repo-key".into());
        let ws_id = app.state.workspaces[0].id.clone();
        app.state.collapsed_agent_space_ids.insert(ws_id);

        let folder_list = app.handle_folder_list("list".into());
        let session_snapshot = app.handle_session_snapshot("snapshot".into());

        for (payload, name) in [
            (&folder_list, "folder.list"),
            (&session_snapshot, "session.snapshot"),
        ] {
            serde_json::from_str::<SuccessResponse>(payload).expect("success response");
            assert!(
                !payload.contains("collaps"),
                "{name} response must not expose collapse state: {payload}"
            );
        }
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
                position: None,
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
    fn folder_rename_updates_label_and_emits_folder_updated() {
        let mut app = test_app(&["one"]);
        let response = app.handle_folder_create(
            "create".into(),
            FolderCreateParams {
                name: "work".into(),
            },
        );
        let folder_id = created_folder_id(&response);

        let response = app.handle_folder_rename(
            "rename".into(),
            FolderRenameParams {
                folder_id: folder_id.clone(),
                name: "personal".into(),
            },
        );

        let success: SuccessResponse = serde_json::from_str(&response).expect("success response");
        assert_eq!(
            success.result,
            ResponseResult::FolderUpdated {
                folder: FolderInfo {
                    folder_id: folder_id.clone(),
                    name: "personal".into(),
                    members: Vec::new(),
                }
            }
        );
        assert_eq!(
            app.state.folder(&folder_id).expect("folder").name,
            "personal"
        );
        let events: Vec<(EventKind, EventData)> = app
            .event_hub
            .events_after(0)
            .into_iter()
            .map(|(_, envelope)| (envelope.event, envelope.data))
            .collect();
        assert!(events.iter().any(|(kind, data)| {
            *kind == EventKind::FolderUpdated
                && matches!(
                    data,
                    EventData::FolderUpdated { folder }
                        if folder.folder_id == folder_id && folder.name == "personal"
                )
        }));
        app.state.assert_invariants_for_test();
    }

    #[test]
    fn folder_rename_never_fails_on_name_collision() {
        let mut app = test_app(&["one"]);
        let first = created_folder_id(&app.handle_folder_create(
            "create-1".into(),
            FolderCreateParams {
                name: "work".into(),
            },
        ));
        let second = created_folder_id(&app.handle_folder_create(
            "create-2".into(),
            FolderCreateParams {
                name: "other".into(),
            },
        ));

        let response = app.handle_folder_rename(
            "rename".into(),
            FolderRenameParams {
                folder_id: second.clone(),
                name: "work".into(),
            },
        );

        let success: SuccessResponse = serde_json::from_str(&response).expect("success response");
        assert!(matches!(
            success.result,
            ResponseResult::FolderUpdated { folder } if folder.name == "work"
        ));
        assert_eq!(app.state.folder(&first).expect("folder").name, "work");
        assert_eq!(app.state.folder(&second).expect("folder").name, "work");
        app.state.assert_invariants_for_test();
    }

    #[test]
    fn folder_rename_rejects_empty_name_and_unknown_folder() {
        let mut app = test_app(&["one"]);
        let response = app.handle_folder_create(
            "create".into(),
            FolderCreateParams {
                name: "work".into(),
            },
        );
        let folder_id = created_folder_id(&response);

        let response = app.handle_folder_rename(
            "rename".into(),
            FolderRenameParams {
                folder_id: folder_id.clone(),
                name: "   ".into(),
            },
        );
        let error: ErrorResponse = serde_json::from_str(&response).expect("error response");
        assert_eq!(error.error.code, "invalid_folder_name");

        let response = app.handle_folder_rename(
            "rename".into(),
            FolderRenameParams {
                folder_id: "f-missing".into(),
                name: "personal".into(),
            },
        );
        let error: ErrorResponse = serde_json::from_str(&response).expect("error response");
        assert_eq!(error.error.code, "folder_not_found");
        assert_eq!(app.state.folder(&folder_id).expect("folder").name, "work");
    }

    #[test]
    fn folder_delete_releases_members_and_emits_folder_deleted() {
        let mut app = test_app(&["one", "two"]);
        let w1 = app.state.workspaces[0].id.clone();
        let w2 = app.state.workspaces[1].id.clone();
        let response = app.handle_folder_create(
            "create".into(),
            FolderCreateParams {
                name: "work".into(),
            },
        );
        let folder_id = created_folder_id(&response);
        app.handle_folder_assign(
            "assign".into(),
            FolderAssignParams {
                workspace_id: w2.clone(),
                folder_id: Some(folder_id.clone()),
                position: None,
            },
        );

        let response = app.handle_folder_delete(
            "delete".into(),
            FolderTarget {
                folder_id: folder_id.clone(),
            },
        );

        let success: SuccessResponse = serde_json::from_str(&response).expect("success response");
        assert_eq!(
            success.result,
            ResponseResult::FolderDeleted {
                folder_id: folder_id.clone(),
                workspace_ids: vec![w2.clone()],
            }
        );
        assert!(app.state.folder(&folder_id).is_none());
        assert_eq!(app.state.workspace_folder_id(&w2), None);
        // No space was closed by the delete.
        assert_eq!(
            app.state
                .workspaces
                .iter()
                .map(|ws| ws.id.clone())
                .collect::<Vec<_>>(),
            vec![w1, w2.clone()]
        );
        let events: Vec<(EventKind, EventData)> = app
            .event_hub
            .events_after(0)
            .into_iter()
            .map(|(_, envelope)| (envelope.event, envelope.data))
            .collect();
        assert!(events.iter().any(|(kind, data)| {
            *kind == EventKind::FolderDeleted
                && matches!(
                    data,
                    EventData::FolderDeleted { folder_id: deleted, workspace_ids }
                        if *deleted == folder_id && *workspace_ids == vec![w2.clone()]
                )
        }));
        app.state.assert_invariants_for_test();
    }

    #[test]
    fn folder_delete_rejects_unknown_folder() {
        let mut app = test_app(&["one"]);

        let response = app.handle_folder_delete(
            "delete".into(),
            FolderTarget {
                folder_id: "f-missing".into(),
            },
        );

        let error: ErrorResponse = serde_json::from_str(&response).expect("error response");
        assert_eq!(error.error.code, "folder_not_found");
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
                position: None,
            },
        );
        let error: ErrorResponse = serde_json::from_str(&response).expect("error response");
        assert_eq!(error.error.code, "workspace_not_found");

        let response = app.handle_folder_assign(
            "assign".into(),
            FolderAssignParams {
                workspace_id,
                folder_id: Some("f-missing".into()),
                position: None,
            },
        );
        let error: ErrorResponse = serde_json::from_str(&response).expect("error response");
        assert_eq!(error.error.code, "folder_not_found");
    }

    fn listed_order(app: &mut App) -> (Vec<FolderInfo>, Vec<SpaceOrderEntryInfo>) {
        let response = app.handle_folder_list("list".into());
        let success: SuccessResponse = serde_json::from_str(&response).expect("success response");
        match success.result {
            ResponseResult::FolderList { folders, order } => (folders, order),
            other => panic!("expected folder_list, got {other:?}"),
        }
    }

    #[test]
    fn folder_assign_with_position_inserts_member_at_position() {
        let mut app = test_app(&["one", "two", "three"]);
        let w1 = app.state.workspaces[0].id.clone();
        let w2 = app.state.workspaces[1].id.clone();
        let w3 = app.state.workspaces[2].id.clone();
        let folder_id = created_folder_id(&app.handle_folder_create(
            "create".into(),
            FolderCreateParams {
                name: "work".into(),
            },
        ));
        for (req, ws) in [("assign-1", &w1), ("assign-2", &w2)] {
            app.handle_folder_assign(
                req.into(),
                FolderAssignParams {
                    workspace_id: ws.clone(),
                    folder_id: Some(folder_id.clone()),
                    position: None,
                },
            );
        }

        let response = app.handle_folder_assign(
            "assign-positional".into(),
            FolderAssignParams {
                workspace_id: w3.clone(),
                folder_id: Some(folder_id.clone()),
                position: Some(1),
            },
        );

        let success: SuccessResponse = serde_json::from_str(&response).expect("success response");
        assert_eq!(
            success.result,
            ResponseResult::FolderAssigned {
                folder_id: Some(folder_id.clone()),
                workspace_ids: vec![w3.clone()],
            }
        );
        // folder.list reflects the canonical order after the mutation.
        let (folders, _) = listed_order(&mut app);
        assert_eq!(folders[0].members, vec![w1, w3, w2]);
        app.state.assert_invariants_for_test();
    }

    #[test]
    fn folder_assign_null_with_position_reorders_top_level() {
        let mut app = test_app(&["one", "two", "three"]);
        let w1 = app.state.workspaces[0].id.clone();
        let w2 = app.state.workspaces[1].id.clone();
        let w3 = app.state.workspaces[2].id.clone();

        let response = app.handle_folder_assign(
            "reorder".into(),
            FolderAssignParams {
                workspace_id: w3.clone(),
                folder_id: None,
                position: Some(0),
            },
        );

        let success: SuccessResponse = serde_json::from_str(&response).expect("success response");
        assert_eq!(
            success.result,
            ResponseResult::FolderAssigned {
                folder_id: None,
                workspace_ids: vec![w3.clone()],
            }
        );
        let (_, order) = listed_order(&mut app);
        let ids: Vec<&str> = order.iter().map(|entry| entry.id.as_str()).collect();
        assert_eq!(ids, vec![w3.as_str(), w1.as_str(), w2.as_str()]);
        // A top-level reorder is still an assignment to the top level.
        let events: Vec<EventKind> = app
            .event_hub
            .events_after(0)
            .into_iter()
            .map(|(_, envelope)| envelope.event)
            .collect();
        assert!(events.contains(&EventKind::FolderAssigned));
        app.state.assert_invariants_for_test();
    }

    #[test]
    fn folder_assign_within_folder_reorder_emits_folder_updated() {
        let mut app = test_app(&["one", "two"]);
        let w1 = app.state.workspaces[0].id.clone();
        let w2 = app.state.workspaces[1].id.clone();
        let folder_id = created_folder_id(&app.handle_folder_create(
            "create".into(),
            FolderCreateParams {
                name: "work".into(),
            },
        ));
        for (req, ws) in [("assign-1", &w1), ("assign-2", &w2)] {
            app.handle_folder_assign(
                req.into(),
                FolderAssignParams {
                    workspace_id: ws.clone(),
                    folder_id: Some(folder_id.clone()),
                    position: None,
                },
            );
        }
        let sequence_before = app
            .event_hub
            .events_after(0)
            .last()
            .map(|(sequence, _)| *sequence)
            .unwrap_or(0);

        let response = app.handle_folder_assign(
            "reorder".into(),
            FolderAssignParams {
                workspace_id: w2.clone(),
                folder_id: Some(folder_id.clone()),
                position: Some(0),
            },
        );

        // The method contract stays folder_assigned; the event reports a
        // member-order change.
        let success: SuccessResponse = serde_json::from_str(&response).expect("success response");
        assert_eq!(
            success.result,
            ResponseResult::FolderAssigned {
                folder_id: Some(folder_id.clone()),
                workspace_ids: vec![w2.clone()],
            }
        );
        let events: Vec<(EventKind, EventData)> = app
            .event_hub
            .events_after(sequence_before)
            .into_iter()
            .map(|(_, envelope)| (envelope.event, envelope.data))
            .collect();
        assert!(
            events.iter().any(|(kind, data)| {
                *kind == EventKind::FolderUpdated
                    && matches!(
                        data,
                        EventData::FolderUpdated { folder }
                            if folder.folder_id == folder_id
                                && folder.members == vec![w2.clone(), w1.clone()]
                    )
            }),
            "within-folder reorder must emit folder.updated: {events:?}"
        );
        assert!(
            !events
                .iter()
                .any(|(kind, _)| *kind == EventKind::FolderAssigned),
            "within-folder reorder must not emit folder.assigned: {events:?}"
        );
        assert_eq!(
            app.state.folder(&folder_id).expect("folder").members,
            vec![w2, w1]
        );
        app.state.assert_invariants_for_test();
    }

    #[test]
    fn folder_move_repositions_folder_and_emits_folder_moved() {
        let mut app = test_app(&["one", "two"]);
        let w1 = app.state.workspaces[0].id.clone();
        let w2 = app.state.workspaces[1].id.clone();
        let folder_id = created_folder_id(&app.handle_folder_create(
            "create".into(),
            FolderCreateParams {
                name: "work".into(),
            },
        ));
        app.handle_folder_assign(
            "assign".into(),
            FolderAssignParams {
                workspace_id: w2.clone(),
                folder_id: Some(folder_id.clone()),
                position: None,
            },
        );

        let response = app.handle_folder_move(
            "move".into(),
            FolderMoveParams {
                folder_id: folder_id.clone(),
                position: 0,
            },
        );

        let success: SuccessResponse = serde_json::from_str(&response).expect("success response");
        assert_eq!(
            success.result,
            ResponseResult::FolderMoved {
                folder_id: folder_id.clone(),
                position: 0,
            }
        );
        let (_, order) = listed_order(&mut app);
        assert_eq!(order[0].kind, SpaceOrderEntryKind::Folder);
        assert_eq!(order[0].id, folder_id);
        assert_eq!(order[1].id, w1);
        // Membership is untouched by the reposition.
        assert_eq!(app.state.workspace_folder_id(&w2), Some(folder_id.as_str()));
        // The session snapshot lists workspaces in the new canonical order,
        // each carrying its folder id.
        let response = app.handle_session_snapshot("snapshot".into());
        let success: SuccessResponse = serde_json::from_str(&response).expect("success response");
        let ResponseResult::SessionSnapshot { snapshot } = success.result else {
            panic!("expected session_snapshot");
        };
        let listed: Vec<(String, Option<String>)> = snapshot
            .workspaces
            .iter()
            .map(|ws| (ws.workspace_id.clone(), ws.folder_id.clone()))
            .collect();
        assert_eq!(
            listed,
            vec![(w2.clone(), Some(folder_id.clone())), (w1.clone(), None)]
        );
        let events: Vec<(EventKind, EventData)> = app
            .event_hub
            .events_after(0)
            .into_iter()
            .map(|(_, envelope)| (envelope.event, envelope.data))
            .collect();
        assert!(events.iter().any(|(kind, data)| {
            *kind == EventKind::FolderMoved
                && matches!(
                    data,
                    EventData::FolderMoved { folder_id: moved, position }
                        if *moved == folder_id && *position == 0
                )
        }));
        app.state.assert_invariants_for_test();
    }

    #[test]
    fn folder_move_clamps_position_and_reports_effective_index() {
        let mut app = test_app(&["one", "two"]);
        let folder_id = created_folder_id(&app.handle_folder_create(
            "create".into(),
            FolderCreateParams {
                name: "work".into(),
            },
        ));
        app.handle_folder_move(
            "move-front".into(),
            FolderMoveParams {
                folder_id: folder_id.clone(),
                position: 0,
            },
        );

        let response = app.handle_folder_move(
            "move-clamped".into(),
            FolderMoveParams {
                folder_id: folder_id.clone(),
                position: 99,
            },
        );

        let success: SuccessResponse = serde_json::from_str(&response).expect("success response");
        assert_eq!(
            success.result,
            ResponseResult::FolderMoved {
                folder_id,
                position: 2,
            }
        );
        app.state.assert_invariants_for_test();
    }

    #[test]
    fn folder_move_noop_does_not_emit_event() {
        let mut app = test_app(&["one"]);
        let folder_id = created_folder_id(&app.handle_folder_create(
            "create".into(),
            FolderCreateParams {
                name: "work".into(),
            },
        ));
        let sequence_before = app
            .event_hub
            .events_after(0)
            .last()
            .map(|(sequence, _)| *sequence)
            .unwrap_or(0);

        let response = app.handle_folder_move(
            "move-noop".into(),
            FolderMoveParams {
                folder_id: folder_id.clone(),
                position: 1,
            },
        );

        let success: SuccessResponse = serde_json::from_str(&response).expect("success response");
        assert_eq!(
            success.result,
            ResponseResult::FolderMoved {
                folder_id,
                position: 1,
            }
        );
        assert!(
            app.event_hub.events_after(sequence_before).is_empty(),
            "a no-op move must not emit events"
        );
        app.state.assert_invariants_for_test();
    }

    #[test]
    fn folder_move_rejects_unknown_folder() {
        let mut app = test_app(&["one"]);

        let response = app.handle_folder_move(
            "move".into(),
            FolderMoveParams {
                folder_id: "f-missing".into(),
                position: 0,
            },
        );

        let error: ErrorResponse = serde_json::from_str(&response).expect("error response");
        assert_eq!(error.error.code, "folder_not_found");
    }
}
