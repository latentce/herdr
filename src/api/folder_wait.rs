//! `events.wait` for folder events, matched straight against
//! the server's event history.

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use crate::api::schema::{
    ErrorBody, ErrorResponse, EventData, EventEnvelope, EventMatch, Method, Request,
    ResponseResult, SuccessResponse, WorkspaceTarget,
};
use crate::api::server::{should_stop_connection, CONNECTION_POLL_INTERVAL};
use crate::api::{ApiRequestMessage, ApiRequestSender, EventHub};
use crate::ipc::LocalStream;

/// How long resolving a workspace id may take before the wait gives up.
const RESOLVE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// A folder event match. `None` fields match anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum FolderEventFilter {
    WorkspaceFolderChanged {
        workspace_id: Option<String>,
        folder_id: Option<String>,
    },
    FolderCreated(Option<String>),
    FolderRenamed(Option<String>),
    FolderDeleted(Option<String>),
    FolderMoved(Option<String>),
}

/// Folder ids in the form events use, so `f01` matches `f1`.
fn canonical_folder(folder_id: &Option<String>) -> Option<String> {
    folder_id.as_ref().map(|id| {
        crate::folders::parse_folder_id(id)
            .map_or_else(|| id.clone(), crate::folders::public_folder_id)
    })
}

impl FolderEventFilter {
    pub(super) fn from_match(event: &EventMatch) -> Option<Self> {
        Some(match event {
            EventMatch::WorkspaceFolderChanged {
                workspace_id,
                folder_id,
            } => Self::WorkspaceFolderChanged {
                workspace_id: workspace_id.clone(),
                folder_id: canonical_folder(folder_id),
            },
            EventMatch::FolderCreated { folder_id } => {
                Self::FolderCreated(canonical_folder(folder_id))
            }
            EventMatch::FolderRenamed { folder_id } => {
                Self::FolderRenamed(canonical_folder(folder_id))
            }
            EventMatch::FolderDeleted { folder_id } => {
                Self::FolderDeleted(canonical_folder(folder_id))
            }
            EventMatch::FolderMoved { folder_id } => Self::FolderMoved(canonical_folder(folder_id)),
            _ => return None,
        })
    }

    /// Resolves a workspace id the way folder methods do (`w_2`, `2`, ...)
    /// into the id events carry.
    fn resolve(self, api_tx: &ApiRequestSender) -> Result<Self, ErrorBody> {
        match self {
            Self::WorkspaceFolderChanged {
                workspace_id: Some(workspace_id),
                folder_id,
            } => Ok(Self::WorkspaceFolderChanged {
                workspace_id: Some(canonical_workspace_id(api_tx, &workspace_id)?),
                folder_id,
            }),
            other => Ok(other),
        }
    }

    fn matches(&self, event: &EventEnvelope) -> bool {
        let same = |wanted: &Option<String>, actual: &str| {
            wanted.as_deref().is_none_or(|wanted| wanted == actual)
        };
        match (self, &event.data) {
            (
                Self::WorkspaceFolderChanged {
                    workspace_id,
                    folder_id,
                },
                EventData::WorkspaceFolderChanged {
                    workspace_id: actual_workspace,
                    folder_id: actual_folder,
                    ..
                },
            ) => {
                same(workspace_id, actual_workspace)
                    && folder_id
                        .as_deref()
                        .is_none_or(|wanted| actual_folder.as_deref() == Some(wanted))
            }
            (Self::FolderCreated(wanted), EventData::FolderCreated { folder, .. }) => {
                same(wanted, &folder.folder_id)
            }
            (Self::FolderRenamed(wanted), EventData::FolderRenamed { folder_id, .. })
            | (Self::FolderDeleted(wanted), EventData::FolderDeleted { folder_id, .. })
            | (Self::FolderMoved(wanted), EventData::FolderMoved { folder_id, .. }) => {
                same(wanted, folder_id)
            }
            _ => false,
        }
    }
}

/// Asks the server for the workspace's id, as `workspace.get` resolves it.
fn canonical_workspace_id(
    api_tx: &ApiRequestSender,
    workspace_id: &str,
) -> Result<String, ErrorBody> {
    let failure = |code: &str, message: String| ErrorBody {
        code: code.into(),
        message,
    };
    let (respond_to, response_rx) = std::sync::mpsc::channel();
    api_tx
        .send(ApiRequestMessage {
            request: Request {
                id: "events.wait.resolve_workspace".into(),
                method: Method::WorkspaceGet(WorkspaceTarget {
                    workspace_id: workspace_id.into(),
                }),
            },
            respond_to,
            response_write_complete: None,
        })
        .map_err(|err| {
            failure(
                "server_unavailable",
                format!("failed to dispatch request: {err}"),
            )
        })?;
    let response = response_rx
        .recv_timeout(RESOLVE_TIMEOUT)
        .map_err(|_| failure("timeout", "timed out resolving the workspace".into()))?;
    let response = serde_json::from_str::<serde_json::Value>(&response)
        .map_err(|err| failure("internal_error", err.to_string()))?;
    if let Some(error) = response.get("error") {
        let text = |key: &str| error.get(key).and_then(serde_json::Value::as_str);
        return Err(failure(
            text("code").unwrap_or("workspace_not_found"),
            text("message").map_or_else(
                || format!("workspace {workspace_id} not found"),
                str::to_owned,
            ),
        ));
    }
    response
        .pointer("/result/workspace/workspace_id")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| failure("internal_error", "workspace.get returned no id".into()))
}

pub(super) fn wait_for_folder_event(
    request_id: String,
    filter: FolderEventFilter,
    timeout_ms: Option<u64>,
    stream: &mut LocalStream,
    api_tx: &ApiRequestSender,
    event_hub: &EventHub,
    running: &Arc<AtomicBool>,
) -> std::io::Result<Option<String>> {
    let filter = match filter.resolve(api_tx) {
        Ok(filter) => filter,
        Err(error) => {
            return serde_json::to_string(&ErrorResponse {
                id: request_id,
                error,
            })
            .map(Some)
            .map_err(std::io::Error::other);
        }
    };
    let deadline =
        timeout_ms.map(|ms| std::time::Instant::now() + std::time::Duration::from_millis(ms));
    let mut sequence = event_hub.current_sequence();
    loop {
        if should_stop_connection(stream, running)? {
            return Ok(None);
        }
        for (next, event) in event_hub.events_after(sequence) {
            sequence = next;
            if filter.matches(&event) {
                return serde_json::to_string(&SuccessResponse {
                    id: request_id,
                    result: ResponseResult::WaitMatched { event },
                })
                .map(Some)
                .map_err(std::io::Error::other);
            }
        }
        if deadline.is_some_and(|deadline| std::time::Instant::now() >= deadline) {
            return serde_json::to_string(&ErrorResponse {
                id: request_id,
                error: ErrorBody {
                    code: "timeout".into(),
                    message: "timed out waiting for event match".into(),
                },
            })
            .map(Some)
            .map_err(std::io::Error::other);
        }
        std::thread::sleep(CONNECTION_POLL_INTERVAL);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::schema::{EventKind, FolderInfo};

    fn folder(id: &str) -> FolderInfo {
        FolderInfo {
            folder_id: id.into(),
            name: "x".into(),
            position: 0,
            workspace_ids: Vec::new(),
        }
    }

    #[test]
    fn filters_match_on_optional_ids() {
        let created = EventEnvelope {
            event: EventKind::FolderCreated,
            data: EventData::FolderCreated {
                folder: folder("f2"),
                folders: Vec::new(),
            },
        };
        assert!(FolderEventFilter::FolderCreated(None).matches(&created));
        assert!(FolderEventFilter::FolderCreated(Some("f2".into())).matches(&created));
        assert!(!FolderEventFilter::FolderCreated(Some("f1".into())).matches(&created));
        assert!(!FolderEventFilter::FolderDeleted(None).matches(&created));

        let changed = EventEnvelope {
            event: EventKind::WorkspaceFolderChanged,
            data: EventData::WorkspaceFolderChanged {
                workspace_id: "w3".into(),
                folder_id: Some("f2".into()),
                previous_folder_id: None,
                folders: Vec::new(),
            },
        };
        let filter = FolderEventFilter::WorkspaceFolderChanged {
            workspace_id: Some("w3".into()),
            folder_id: Some("f2".into()),
        };
        assert!(filter.matches(&changed));
        let filter = FolderEventFilter::WorkspaceFolderChanged {
            workspace_id: None,
            folder_id: Some("f1".into()),
        };
        assert!(!filter.matches(&changed));
        assert!(
            FolderEventFilter::from_match(&EventMatch::WorkspaceFocused {
                workspace_id: "w1".into()
            })
            .is_none()
        );
    }

    #[test]
    fn folder_ids_match_in_any_accepted_spelling() {
        let filter = FolderEventFilter::from_match(&EventMatch::FolderCreated {
            folder_id: Some(" f02 ".into()),
        });
        assert_eq!(
            filter,
            Some(FolderEventFilter::FolderCreated(Some("f2".into())))
        );
    }

    #[test]
    fn workspace_ids_resolve_through_the_server() {
        let (api_tx, mut api_rx) = tokio::sync::mpsc::unbounded_channel::<ApiRequestMessage>();
        let server = std::thread::spawn(move || {
            let message = api_rx.blocking_recv().expect("request");
            let Method::WorkspaceGet(target) = &message.request.method else {
                panic!("expected workspace.get");
            };
            let response = if target.workspace_id == "2" {
                r#"{"id":"x","result":{"type":"workspace_info","workspace":{"workspace_id":"w2"}}}"#
            } else {
                r#"{"id":"x","error":{"code":"workspace_not_found","message":"nope"}}"#
            };
            message.respond_to.send(response.into()).expect("respond");
        });
        let filter = FolderEventFilter::WorkspaceFolderChanged {
            workspace_id: Some("2".into()),
            folder_id: None,
        };
        assert_eq!(
            filter.resolve(&api_tx),
            Ok(FolderEventFilter::WorkspaceFolderChanged {
                workspace_id: Some("w2".into()),
                folder_id: None,
            })
        );
        server.join().expect("server");
    }
}
