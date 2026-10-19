//! Folder-aware drag and drop in the spaces panel.
//!
//! Vertical position picks the gap nearest the pointer, as upstream does.
//! On machines with folders the column then decides the level, like an
//! outliner: right of a folder's member column means inside the folder.

use super::*;
use crate::api::schema::{
    FolderAddWorkspaceParams, FolderMoveParams, FolderRemoveWorkspaceParams, Method,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::client::shell) enum DragSource {
    Workspace(String),
    Folder(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::client::shell) enum DropPlacement {
    /// Inside a folder, before a member or at the end.
    Into {
        folder_id: String,
        before: Option<String>,
    },
    /// At top level, before or after a folder or loose workspace.
    Top {
        before: Option<String>,
        after: Option<String>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::client::shell) struct DropTarget {
    pub(in crate::client::shell) placement: DropPlacement,
    /// Drop line row and start column.
    pub(in crate::client::shell) indicator: Option<(u16, u16)>,
    /// Folder header highlighted as the drop target.
    pub(in crate::client::shell) header: Option<String>,
}

#[derive(Clone, Debug)]
pub(in crate::client::shell) struct FolderDrag {
    pub(in crate::client::shell) source: DragSource,
    pub(in crate::client::shell) target: Option<DropTarget>,
}

impl FolderDrag {
    pub(in crate::client::shell) fn dragged_workspace_id(&self) -> Option<&str> {
        match &self.source {
            DragSource::Workspace(id) => Some(id),
            DragSource::Folder(_) => None,
        }
    }

    pub(in crate::client::shell) fn dragged_folder_id(&self) -> Option<&str> {
        match &self.source {
            DragSource::Folder(id) => Some(id),
            DragSource::Workspace(_) => None,
        }
    }

    pub(in crate::client::shell) fn indicator(&self) -> Option<(u16, u16)> {
        self.target.as_ref().and_then(|target| target.indicator)
    }

    pub(in crate::client::shell) fn target_header(&self) -> Option<&str> {
        self.target
            .as_ref()
            .and_then(|target| target.header.as_deref())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum UnitKind {
    Header,
    Member { folder_id: String },
    Loose,
}

/// A visible top-level item or folder member, with its rows.
#[derive(Clone, Debug)]
struct Unit {
    kind: UnitKind,
    /// Folder id for headers, the leading workspace id otherwise.
    id: String,
    top: u16,
    bottom: u16,
}

impl Unit {
    fn is_top_level(&self) -> bool {
        !matches!(self.kind, UnitKind::Member { .. })
    }
}

struct DropGeometry {
    units: Vec<Unit>,
    top_x: u16,
    divider_x: u16,
}

impl ClientShellState {
    /// The active machine's visible rows, grouped into drop units.
    fn folder_drop_geometry(&self) -> Option<DropGeometry> {
        let snapshot = self.snapshot.as_deref()?;
        let view = self.active_folder_view()?;
        let top_x = self.hits.workspace_body.x;
        let divider_x = top_x.saturating_add(FOLDER_INDENT).saturating_add(1);
        enum Row<'a> {
            Header(&'a FolderHeaderHit),
            Workspace(&'a WorkspaceHit),
        }
        let mut rows = self
            .hits
            .folder_headers
            .iter()
            .filter(|hit| hit.endpoint_id == self.active_endpoint_id)
            .map(|hit| (hit.rect.y, Row::Header(hit)))
            .chain(
                self.hits
                    .workspaces
                    .iter()
                    .filter(|hit| hit.endpoint_id == self.active_endpoint_id)
                    .map(|hit| (hit.rect.y, Row::Workspace(hit))),
            )
            .collect::<Vec<_>>();
        rows.sort_by_key(|(y, _)| *y);
        let mut units = Vec::<Unit>::new();
        for (_, row) in rows {
            match row {
                Row::Header(hit) => units.push(Unit {
                    kind: UnitKind::Header,
                    id: hit.folder_id.clone(),
                    top: hit.rect.y,
                    bottom: hit.rect.bottom(),
                }),
                Row::Workspace(hit) => {
                    if starts_unit(snapshot, hit) || units.is_empty() {
                        units.push(Unit {
                            kind: match view.folder_id_of(&hit.workspace_id) {
                                Some(folder_id) => UnitKind::Member {
                                    folder_id: folder_id.to_owned(),
                                },
                                None => UnitKind::Loose,
                            },
                            id: hit.workspace_id.clone(),
                            top: hit.rect.y,
                            bottom: hit.rect.bottom(),
                        });
                    } else if let Some(unit) = units.last_mut() {
                        unit.bottom = hit.rect.bottom();
                    }
                }
            }
        }
        Some(DropGeometry {
            units,
            top_x,
            divider_x,
        })
    }

    fn pointer_in_drop_area(&self, point: (u16, u16)) -> bool {
        self.hits.workspace_body.height > 0
            && point.1 >= self.hits.workspace_body.y.saturating_sub(1)
            && point.1 < self.hits.new_workspace.y
            && !self.hits.workspaces.iter().any(|hit| {
                hit.endpoint_id != self.active_endpoint_id
                    && super::super::contains(hit.rect, point)
            })
            && !self.hits.folder_headers.iter().any(|hit| {
                hit.endpoint_id != self.active_endpoint_id
                    && super::super::contains(hit.rect, point)
            })
            && !self.hits.machines.iter().any(|hit| {
                hit.endpoint_id != self.active_endpoint_id
                    && super::super::contains(hit.rect, point)
            })
    }

    pub(in crate::client::shell) fn folder_drop_target_at(
        &self,
        source: &DragSource,
        point: (u16, u16),
    ) -> Option<DropTarget> {
        if !self.pointer_in_drop_area(point) {
            return None;
        }
        let geometry = self.folder_drop_geometry()?;
        match source {
            DragSource::Workspace(_) => workspace_drop_target(&geometry, point),
            DragSource::Folder(_) => folder_drop_target(&geometry, point),
        }
    }

    /// Starts or ignores a drag from a folder header press or, on machines
    /// with folders, a workspace press. Returns whether it handled the motion.
    pub(in crate::client::shell) fn folder_drag_motion(
        &mut self,
        column: u16,
        row: u16,
        outcome: &mut ClientShellInput,
    ) -> bool {
        let point = (column, row);
        if let Some(press) = self.folders.press.as_ref() {
            let moved = column
                .abs_diff(press.start_column)
                .max(row.abs_diff(press.start_row));
            if moved >= 1 && press.endpoint_id == self.active_endpoint_id {
                let source = DragSource::Folder(press.folder_id.clone());
                if let Some(target) = self.folder_drop_target_at(&source, point) {
                    self.folders.press = None;
                    self.chrome_drag = Some(ClientChromeDrag::Folders(FolderDrag {
                        source,
                        target: Some(target),
                    }));
                    outcome.repaint = true;
                }
            }
            return true;
        }
        let Some(press) = self.workspace_press.as_ref() else {
            return false;
        };
        if !self.active_endpoint_supports_folders() {
            return false;
        }
        let moved = column
            .abs_diff(press.start_column)
            .max(row.abs_diff(press.start_row));
        if moved >= 1 && self.endpoint_workspace_is_draggable(press) {
            let source = DragSource::Workspace(press.workspace_id.clone());
            if let Some(target) = self.folder_drop_target_at(&source, point) {
                self.chrome_drag = Some(ClientChromeDrag::Folders(FolderDrag {
                    source,
                    target: Some(target),
                }));
                outcome.repaint = true;
            }
        }
        true
    }

    pub(in crate::client::shell) fn update_folder_drag(&mut self, point: (u16, u16)) {
        let source = match self.chrome_drag.as_ref() {
            Some(ClientChromeDrag::Folders(drag)) => drag.source.clone(),
            _ => return,
        };
        let target = self.folder_drop_target_at(&source, point);
        if let Some(ClientChromeDrag::Folders(drag)) = self.chrome_drag.as_mut() {
            drag.target = target;
        }
    }

    pub(in crate::client::shell) fn finish_folder_drag(
        &mut self,
        drag: FolderDrag,
        outcome: &mut ClientShellInput,
    ) {
        outcome.repaint = true;
        let Some(target) = drag.target else {
            return;
        };
        let refers_to = |id: &str| match &target.placement {
            DropPlacement::Top { before, after } => {
                before.as_deref() == Some(id) || after.as_deref() == Some(id)
            }
            DropPlacement::Into { before, .. } => before.as_deref() == Some(id),
        };
        let method = match (drag.source, target.placement.clone()) {
            (DragSource::Workspace(workspace_id), DropPlacement::Into { folder_id, before }) => {
                if refers_to(&workspace_id) {
                    return;
                }
                Method::FolderAddWorkspace(FolderAddWorkspaceParams {
                    folder_id,
                    workspace_id,
                    before,
                    after: None,
                })
            }
            (DragSource::Workspace(workspace_id), DropPlacement::Top { before, after }) => {
                if refers_to(&workspace_id) {
                    return;
                }
                Method::FolderRemoveWorkspace(FolderRemoveWorkspaceParams {
                    workspace_id,
                    before,
                    after,
                })
            }
            (DragSource::Folder(folder_id), DropPlacement::Top { before, after }) => {
                if refers_to(&folder_id) {
                    return;
                }
                Method::FolderMove(FolderMoveParams {
                    folder_id,
                    before,
                    after,
                })
            }
            (DragSource::Folder(_), DropPlacement::Into { .. }) => return,
        };
        self.push_endpoint_method(method, outcome);
    }

    /// Left press on a folder header: toggles on release unless it turns
    /// into a drag. Works on every machine's headers.
    pub(in crate::client::shell) fn press_folder_header(&mut self, column: u16, row: u16) -> bool {
        let Some(hit) = self
            .hits
            .folder_headers
            .iter()
            .find(|hit| super::super::contains(hit.rect, (column, row)))
        else {
            return false;
        };
        self.folders.press = Some(FolderPress {
            endpoint_id: hit.endpoint_id.clone(),
            folder_id: hit.folder_id.clone(),
            start_column: column,
            start_row: row,
        });
        true
    }

    /// Release of a header press that did not become a drag.
    pub(in crate::client::shell) fn release_folder_press(
        &mut self,
        outcome: &mut ClientShellInput,
    ) -> bool {
        let Some(press) = self.folders.press.take() else {
            return false;
        };
        self.folders
            .toggle_folder(&press.endpoint_id, &press.folder_id);
        self.persist_chrome_preferences(outcome);
        outcome.repaint = true;
        true
    }
}

/// Whether a workspace row begins a new unit (not a worktree child, and not
/// a second parent of a group drawn under the first).
fn starts_unit(snapshot: &ClientShellSnapshot, hit: &WorkspaceHit) -> bool {
    if hit.indented {
        return false;
    }
    hit.group_toggle.as_ref().is_none_or(|(_, key)| {
        snapshot
            .workspaces
            .iter()
            .find(|workspace| {
                workspace
                    .worktree
                    .as_ref()
                    .is_some_and(|worktree| worktree.key == *key && !worktree.is_linked_worktree)
            })
            .is_some_and(|workspace| workspace.workspace_id == hit.workspace_id)
    })
}

fn header_at(units: &[Unit], point: (u16, u16)) -> Option<&Unit> {
    units.iter().find(|unit| {
        matches!(unit.kind, UnitKind::Header) && point.1 >= unit.top && point.1 < unit.bottom
    })
}

fn nearest_gap(gaps: &[u16], row: u16) -> Option<usize> {
    gaps.iter()
        .enumerate()
        .min_by_key(|(index, gap)| (row.abs_diff(**gap), *index))
        .map(|(index, _)| index)
}

fn workspace_drop_target(geometry: &DropGeometry, point: (u16, u16)) -> Option<DropTarget> {
    let units = &geometry.units;
    let right_of_divider = point.0 >= geometry.divider_x;
    if let Some(header) = header_at(units, point) {
        return Some(if right_of_divider {
            DropTarget {
                placement: DropPlacement::Into {
                    folder_id: header.id.clone(),
                    before: None,
                },
                indicator: None,
                header: Some(header.id.clone()),
            }
        } else {
            DropTarget {
                placement: DropPlacement::Top {
                    before: Some(header.id.clone()),
                    after: None,
                },
                indicator: Some((header.top.saturating_sub(1), geometry.top_x)),
                header: None,
            }
        });
    }
    let last = units.last()?;
    let gaps = units
        .iter()
        .map(|unit| unit.top.saturating_sub(1))
        .chain(std::iter::once(last.bottom))
        .collect::<Vec<_>>();
    let index = nearest_gap(&gaps, point.1)?;
    let row = gaps[index];
    let previous = index.checked_sub(1).and_then(|index| units.get(index));
    let current = units.get(index);
    let top = |placement| DropTarget {
        placement,
        indicator: Some((row, geometry.top_x)),
        header: None,
    };
    let into = |folder_id: &str, before: Option<String>| DropTarget {
        placement: DropPlacement::Into {
            folder_id: folder_id.to_owned(),
            before,
        },
        indicator: Some((row, geometry.divider_x)),
        header: None,
    };
    Some(match (previous, current) {
        (
            _,
            Some(Unit {
                kind: UnitKind::Member { folder_id },
                id,
                ..
            }),
        ) => into(folder_id, Some(id.clone())),
        (
            Some(Unit {
                kind: UnitKind::Member { folder_id },
                ..
            }),
            current,
        ) => {
            if right_of_divider {
                into(folder_id, None)
            } else {
                match current {
                    Some(current) => top(DropPlacement::Top {
                        before: Some(current.id.clone()),
                        after: None,
                    }),
                    None => top(DropPlacement::Top {
                        before: None,
                        after: Some(folder_id.clone()),
                    }),
                }
            }
        }
        (_, Some(current)) => top(DropPlacement::Top {
            before: Some(current.id.clone()),
            after: None,
        }),
        (Some(previous), None) => top(DropPlacement::Top {
            before: None,
            after: Some(previous.id.clone()),
        }),
        (None, None) => return None,
    })
}

fn folder_drop_target(geometry: &DropGeometry, point: (u16, u16)) -> Option<DropTarget> {
    let units = &geometry.units;
    let top_target = |row: u16, placement| DropTarget {
        placement,
        indicator: Some((row, geometry.top_x)),
        header: None,
    };
    if let Some(header) = header_at(units, point) {
        return Some(top_target(
            header.top.saturating_sub(1),
            DropPlacement::Top {
                before: Some(header.id.clone()),
                after: None,
            },
        ));
    }
    let items = units
        .iter()
        .filter(|unit| unit.is_top_level())
        .collect::<Vec<_>>();
    let last_item = items.last()?;
    let last_bottom = units.last().map_or(last_item.bottom, |unit| unit.bottom);
    let gaps = items
        .iter()
        .map(|unit| unit.top.saturating_sub(1))
        .chain(std::iter::once(last_bottom))
        .collect::<Vec<_>>();
    let index = nearest_gap(&gaps, point.1)?;
    let placement = match items.get(index) {
        Some(item) => DropPlacement::Top {
            before: Some(item.id.clone()),
            after: None,
        },
        None => DropPlacement::Top {
            before: None,
            after: Some(last_item.id.clone()),
        },
    };
    Some(top_target(gaps[index], placement))
}
