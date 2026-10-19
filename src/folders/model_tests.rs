use super::*;

fn ws(id: &str) -> WorkspaceFact {
    WorkspaceFact {
        id: id.into(),
        worktree: None,
    }
}

fn parent(id: &str, key: &str) -> WorkspaceFact {
    WorkspaceFact {
        id: id.into(),
        worktree: Some(WorktreeFact {
            key: key.into(),
            linked: false,
        }),
    }
}

fn child(id: &str, key: &str) -> WorkspaceFact {
    WorkspaceFact {
        id: id.into(),
        worktree: Some(WorktreeFact {
            key: key.into(),
            linked: true,
        }),
    }
}

fn synced(flat: Vec<WorkspaceFact>) -> FolderState {
    let mut state = FolderState::default();
    state.reconcile(flat);
    state
}

/// Applies an outcome's flat order to the facts, like the server does.
fn apply(flat: &mut Vec<WorkspaceFact>, outcome: &FolderOutcome) {
    if let Some(order) = &outcome.flat {
        *flat = reorder(flat, order);
    }
}

fn ids(flat: &[WorkspaceFact]) -> Vec<&str> {
    flat.iter().map(|fact| fact.id.as_str()).collect()
}

fn order(state: &FolderState) -> Vec<String> {
    state
        .order()
        .iter()
        .map(|item| match item {
            Item::Workspace(id) => id.clone(),
            Item::Folder(id) => public_folder_id(*id),
        })
        .collect()
}

fn members(state: &FolderState, id: FolderId) -> Vec<&str> {
    state
        .folder(id)
        .map(|folder| folder.members.iter().map(String::as_str).collect())
        .unwrap_or_default()
}

fn before_ws(id: &str) -> Placement {
    Placement::Before(FolderRef::Workspace(id.into()))
}

fn after_ws(id: &str) -> Placement {
    Placement::After(FolderRef::Workspace(id.into()))
}

#[test]
fn folder_ids_parse_and_format() {
    assert_eq!(public_folder_id(12), "f12");
    assert_eq!(parse_folder_id("f12"), Some(12));
    assert_eq!(parse_folder_id(" f3 "), Some(3));
    assert_eq!(parse_folder_id("f0"), None);
    assert_eq!(parse_folder_id("w1"), None);
    assert_eq!(parse_folder_id("f"), None);
    assert_eq!(parse_folder_id("12"), None);
}

#[test]
fn names_are_trimmed_and_empty_names_rejected() {
    assert_eq!(normalize_folder_name("  api  ").as_deref(), Some("api"));
    assert_eq!(normalize_folder_name(" \t "), None);
}

#[test]
fn empty_folder_goes_to_the_end_and_ids_are_never_reused() {
    let mut state = synced(vec![ws("w1"), ws("w2")]);
    let (first, _) = state.create("a".into(), &[], None).expect("create");
    assert_eq!(order(&state), ["w1", "w2", "f1"]);
    state.delete(first).expect("delete");
    let (second, _) = state.create("b".into(), &[], None).expect("create");
    assert_eq!(second, 2);
    assert_eq!(order(&state), ["w1", "w2", "f2"]);
}

#[test]
fn saved_ids_at_the_integer_limit_never_overflow_or_repeat() {
    let mut state = FolderState::default();
    state.restore(
        vec![Folder {
            id: FolderId::MAX - 1,
            name: "huge".into(),
            members: vec![],
        }],
        vec![Item::Folder(FolderId::MAX - 1)],
        FolderId::MAX,
    );
    state.reconcile(vec![ws("w1")]);
    let before = order(&state);
    let error = state.create("a".into(), &[], None).expect_err("exhausted");
    assert_eq!(error.code, "folder_ids_exhausted");
    assert_eq!(order(&state), before);

    state.restore(
        vec![Folder {
            id: FolderId::MAX,
            name: "max".into(),
            members: vec![],
        }],
        vec![Item::Folder(FolderId::MAX)],
        1,
    );
    assert_eq!(state.next_id(), FolderId::MAX);
    state.reconcile(vec![ws("w1")]);
    assert_eq!(
        state
            .create("b".into(), &[], None)
            .expect_err("exhausted")
            .code,
        "folder_ids_exhausted"
    );
}

#[test]
fn create_with_workspaces_goes_where_the_first_listed_workspace_was() {
    let mut flat = vec![ws("w1"), ws("w2"), ws("w3"), ws("w4")];
    let mut state = synced(flat.clone());
    let (id, outcome) = state
        .create("x".into(), &["w3".into(), "w1".into(), "w3".into()], None)
        .expect("create");
    apply(&mut flat, &outcome);
    assert_eq!(members(&state, id), ["w3", "w1"]);
    assert_eq!(order(&state), ["w2", "f1", "w4"]);
    assert_eq!(ids(&flat), ["w2", "w3", "w1", "w4"]);
    assert_eq!(outcome.membership.len(), 2);
}

#[test]
fn new_folder_from_a_member_goes_right_after_its_old_folder() {
    let mut flat = vec![ws("w1"), ws("w2"), ws("w3")];
    let mut state = synced(flat.clone());
    let (old, outcome) = state
        .create("old".into(), &["w1".into(), "w2".into()], None)
        .expect("create");
    apply(&mut flat, &outcome);
    let (new, outcome) = state
        .create("new".into(), &["w2".into()], None)
        .expect("create");
    apply(&mut flat, &outcome);
    assert_eq!(order(&state), ["f1", "f2", "w3"]);
    assert_eq!(members(&state, old), ["w1"]);
    assert_eq!(members(&state, new), ["w2"]);
    assert_eq!(ids(&flat), ["w1", "w2", "w3"]);
}

#[test]
fn create_with_unknown_workspace_changes_nothing() {
    let mut state = synced(vec![ws("w1")]);
    let error = state
        .create("x".into(), &["w1".into(), "w9".into()], None)
        .expect_err("unknown");
    assert_eq!(error.code, "workspace_not_found");
    assert!(state.is_empty());
    assert_eq!(state.next_id(), 1);
}

#[test]
fn delete_leaves_members_loose_in_place() {
    let mut flat = vec![ws("w1"), ws("w2"), ws("w3"), ws("w4")];
    let mut state = synced(flat.clone());
    let (id, outcome) = state
        .create("x".into(), &["w2".into(), "w3".into()], None)
        .expect("create");
    apply(&mut flat, &outcome);
    let outcome = state.delete(id).expect("delete");
    assert!(outcome.flat.is_none());
    assert_eq!(order(&state), ["w1", "w2", "w3", "w4"]);
    assert_eq!(state.folder_of("w2"), None);
    assert_eq!(outcome.membership.len(), 2);
}

#[test]
fn add_appends_to_folder_end_and_moves_between_folders() {
    let mut flat = vec![ws("w1"), ws("w2"), ws("w3"), ws("w4"), ws("w5")];
    let mut state = synced(flat.clone());
    let (a, outcome) = state.create("a".into(), &["w1".into()], None).expect("a");
    apply(&mut flat, &outcome);
    let (b, outcome) = state.create("b".into(), &["w4".into()], None).expect("b");
    apply(&mut flat, &outcome);
    let outcome = state.add_workspace(a, "w5", None).expect("add");
    apply(&mut flat, &outcome);
    assert_eq!(members(&state, a), ["w1", "w5"]);
    assert_eq!(ids(&flat), ["w1", "w5", "w2", "w3", "w4"]);
    let outcome = state
        .add_workspace(b, "w1", Some(before_ws("w4")))
        .expect("move");
    apply(&mut flat, &outcome);
    assert_eq!(members(&state, a), ["w5"]);
    assert_eq!(members(&state, b), ["w1", "w4"]);
    assert_eq!(order(&state), ["f1", "w2", "w3", "f2"]);
    assert_eq!(ids(&flat), ["w5", "w2", "w3", "w1", "w4"]);
}

#[test]
fn adding_to_own_folder_repositions_it() {
    let mut flat = vec![ws("w1"), ws("w2"), ws("w3")];
    let mut state = synced(flat.clone());
    let (id, outcome) = state
        .create("a".into(), &["w1".into(), "w2".into(), "w3".into()], None)
        .expect("create");
    apply(&mut flat, &outcome);
    let outcome = state.add_workspace(id, "w1", None).expect("end");
    apply(&mut flat, &outcome);
    assert_eq!(members(&state, id), ["w2", "w3", "w1"]);
    let outcome = state
        .add_workspace(id, "w1", Some(before_ws("w2")))
        .expect("first");
    apply(&mut flat, &outcome);
    assert_eq!(members(&state, id), ["w1", "w2", "w3"]);
    assert_eq!(ids(&flat), ["w1", "w2", "w3"]);
    assert!(outcome.membership.is_empty());
}

#[test]
fn add_into_empty_folder_places_members_at_the_folder() {
    let mut flat = vec![ws("w1"), ws("w2"), ws("w3")];
    let mut state = synced(flat.clone());
    let (id, _) = state
        .create("e".into(), &[], Some(before_ws("w2")))
        .expect("create");
    assert_eq!(order(&state), ["w1", "f1", "w2", "w3"]);
    let outcome = state.add_workspace(id, "w3", None).expect("add");
    apply(&mut flat, &outcome);
    assert_eq!(order(&state), ["w1", "f1", "w2"]);
    assert_eq!(ids(&flat), ["w1", "w3", "w2"]);
}

#[test]
fn positions_must_fit_the_operation() {
    let mut flat = vec![ws("w1"), ws("w2"), ws("w3"), ws("w5")];
    let mut state = synced(flat.clone());
    let (a, outcome) = state.create("a".into(), &["w1".into()], None).expect("a");
    apply(&mut flat, &outcome);
    let (b, outcome) = state.create("b".into(), &["w5".into()], None).expect("b");
    apply(&mut flat, &outcome);
    let before = (order(&state), ids(&flat).join(","));
    let error = state
        .add_workspace(a, "w2", Some(before_ws("w5")))
        .expect_err("w5 is in another folder");
    assert_eq!(error.code, "folder_position_invalid");
    let error = state
        .move_folder(a, before_ws("w5"))
        .expect_err("folder position inside another folder");
    assert_eq!(error.code, "folder_position_invalid");
    let error = state
        .remove_workspace("w1", Some(after_ws("w5")))
        .expect_err("inside folder");
    assert_eq!(error.code, "folder_position_invalid");
    let error = state
        .add_workspace(99, "w2", None)
        .expect_err("unknown folder");
    assert_eq!(error.code, "folder_not_found");
    assert_eq!(before, (order(&state), ids(&flat).join(",")));
    let _ = b;
}

#[test]
fn remove_defaults_to_right_after_the_folder() {
    let mut flat = vec![ws("w1"), ws("w2"), ws("w3"), ws("w4")];
    let mut state = synced(flat.clone());
    let (id, outcome) = state
        .create("a".into(), &["w1".into(), "w2".into(), "w3".into()], None)
        .expect("create");
    apply(&mut flat, &outcome);
    let outcome = state.remove_workspace("w1", None).expect("remove");
    apply(&mut flat, &outcome);
    assert_eq!(order(&state), ["f1", "w1", "w4"]);
    assert_eq!(ids(&flat), ["w2", "w3", "w1", "w4"]);
    assert_eq!(members(&state, id), ["w2", "w3"]);
}

#[test]
fn removing_a_loose_workspace_is_a_no_op_without_position() {
    let mut state = synced(vec![ws("w1"), ws("w2")]);
    let outcome = state.remove_workspace("w1", None).expect("remove");
    assert_eq!(outcome, FolderOutcome::default());
}

#[test]
fn removing_a_loose_workspace_with_position_moves_it_next_to_an_empty_folder() {
    let mut flat = vec![ws("w1"), ws("w2"), ws("w3")];
    let mut state = synced(flat.clone());
    let (id, _) = state
        .create("e".into(), &[], Some(before_ws("w2")))
        .expect("create");
    let outcome = state
        .remove_workspace("w3", Some(Placement::After(FolderRef::Folder(id))))
        .expect("remove");
    apply(&mut flat, &outcome);
    assert_eq!(order(&state), ["w1", "f1", "w3", "w2"]);
    assert_eq!(ids(&flat), ["w1", "w3", "w2"]);
}

#[test]
fn moving_folders_keeps_members_together_and_empty_folders_in_place() {
    let mut flat = vec![ws("w1"), ws("w2"), ws("w3"), ws("w4")];
    let mut state = synced(flat.clone());
    let (a, outcome) = state
        .create("a".into(), &["w1".into(), "w2".into()], None)
        .expect("a");
    apply(&mut flat, &outcome);
    let (empty, _) = state
        .create("e".into(), &[], Some(before_ws("w4")))
        .expect("e");
    assert_eq!(order(&state), ["f1", "w3", "f2", "w4"]);
    let outcome = state.move_folder(a, after_ws("w4")).expect("move");
    apply(&mut flat, &outcome);
    assert_eq!(order(&state), ["w3", "f2", "w4", "f1"]);
    assert_eq!(ids(&flat), ["w3", "w4", "w1", "w2"]);
    let outcome = state
        .move_folder(empty, Placement::Before(FolderRef::Workspace("w3".into())))
        .expect("move empty");
    apply(&mut flat, &outcome);
    assert_eq!(order(&state), ["f2", "w3", "w4", "f1"]);
    assert_eq!(ids(&flat), ["w3", "w4", "w1", "w2"]);
}

#[test]
fn worktree_group_moves_as_one_unit() {
    let mut flat = vec![
        ws("w1"),
        parent("w2", "repo"),
        ws("w3"),
        child("w4", "repo"),
    ];
    let mut state = synced(flat.clone());
    let (id, outcome) = state.create("a".into(), &[], None).expect("create");
    apply(&mut flat, &outcome);
    let outcome = state
        .add_workspace(id, "w4", None)
        .expect("child moves group");
    apply(&mut flat, &outcome);
    assert_eq!(members(&state, id), ["w2", "w4"]);
    assert_eq!(state.folder_of("w2"), Some(id));
    assert_eq!(ids(&flat), ["w1", "w3", "w2", "w4"]);
    let outcome = state.remove_workspace("w2", None).expect("remove group");
    apply(&mut flat, &outcome);
    assert_eq!(state.folder_of("w4"), None);
    assert_eq!(order(&state), ["w1", "w3", "f1", "w2", "w4"]);
}

#[test]
fn linked_worktree_opened_from_parent_in_folder_joins_that_folder() {
    let mut flat = vec![ws("w1"), parent("w2", "repo"), ws("w3")];
    let mut state = synced(flat.clone());
    let (id, outcome) = state
        .create("a".into(), &["w2".into()], None)
        .expect("create");
    apply(&mut flat, &outcome);
    flat.push(child("w4", "repo"));
    let outcome = state.reconcile(flat.clone());
    apply(&mut flat, &outcome);
    assert_eq!(members(&state, id), ["w2", "w4"]);
    assert_eq!(ids(&flat), ["w1", "w2", "w4", "w3"]);
    assert_eq!(
        outcome.membership,
        [MembershipChange {
            workspace_id: "w4".into(),
            from: None,
            to: Some(id),
        }]
    );
}

#[test]
fn parent_created_loose_joins_its_lone_linked_worktree_in_its_folder() {
    let mut flat = vec![ws("w1"), child("w2", "repo"), ws("w3"), ws("w4")];
    let mut state = synced(flat.clone());
    let (id, outcome) = state
        .create("x".into(), &["w2".into(), "w3".into()], None)
        .expect("create");
    apply(&mut flat, &outcome);
    flat[0] = parent("w1", "repo");
    let outcome = state.reconcile(flat.clone());
    apply(&mut flat, &outcome);
    assert_eq!(state.folder_of("w1"), Some(id));
    assert_eq!(members(&state, id), ["w1", "w2", "w3"]);
    assert_eq!(order(&state), ["f1", "w4"]);
    assert_eq!(ids(&flat), ["w1", "w2", "w3", "w4"]);

    let mut flat = vec![ws("w1"), child("w2", "repo"), ws("w3")];
    let mut state = synced(flat.clone());
    let (id, outcome) = state
        .create("x".into(), &["w2".into()], None)
        .expect("create");
    apply(&mut flat, &outcome);
    flat.push(parent("w5", "repo"));
    let outcome = state.reconcile(flat.clone());
    apply(&mut flat, &outcome);
    assert_eq!(members(&state, id), ["w5", "w2"]);
    assert_eq!(ids(&flat), ["w1", "w5", "w2", "w3"]);
}

#[test]
fn parents_folder_wins_over_a_childs_other_folder() {
    let mut flat = vec![parent("w1", "repo"), ws("w2"), child("w3", "other-repo")];
    let mut state = synced(flat.clone());
    let (a, outcome) = state.create("a".into(), &["w1".into()], None).expect("a");
    apply(&mut flat, &outcome);
    let (b, outcome) = state.create("b".into(), &["w3".into()], None).expect("b");
    apply(&mut flat, &outcome);
    let index = flat.iter().position(|fact| fact.id == "w3").expect("w3");
    flat[index] = child("w3", "repo");
    let outcome = state.reconcile(flat.clone());
    apply(&mut flat, &outcome);
    assert_eq!(members(&state, a), ["w1", "w3"]);
    assert!(members(&state, b).is_empty());
    assert_eq!(order(&state), ["f1", "w2", "f2"]);
}

#[test]
fn members_leaving_a_group_in_a_folder_stay_right_after_it() {
    let mut flat = vec![
        ws("w1"),
        parent("w2", "repo"),
        child("w3", "repo"),
        child("w4", "repo"),
        ws("w5"),
        ws("w6"),
    ];
    let mut state = synced(flat.clone());
    let (id, outcome) = state
        .create("x".into(), &["w2".into(), "w5".into()], None)
        .expect("create");
    apply(&mut flat, &outcome);
    assert_eq!(members(&state, id), ["w2", "w3", "w4", "w5"]);
    let index = flat.iter().position(|fact| fact.id == "w3").expect("w3");
    flat[index] = ws("w3");
    let outcome = state.reconcile(flat.clone());
    apply(&mut flat, &outcome);
    assert!(outcome.membership.is_empty());
    assert_eq!(members(&state, id), ["w2", "w4", "w3", "w5"]);
    let flat2 = flat.iter().map(|fact| ws(&fact.id)).collect::<Vec<_>>();
    state.reconcile(flat2);
    assert_eq!(members(&state, id), ["w2", "w4", "w3", "w5"]);
}

#[test]
fn loose_group_break_up_keeps_flat_order() {
    let flat = vec![parent("w1", "repo"), ws("w2"), child("w3", "repo")];
    let mut state = synced(flat);
    assert_eq!(order(&state), ["w1", "w3", "w2"]);
    let outcome = state.reconcile(vec![ws("w1"), ws("w2"), ws("w3")]);
    assert!(outcome.flat.is_none());
    assert_eq!(order(&state), ["w1", "w2", "w3"]);
}

#[test]
fn restored_folders_reconcile_against_an_upstream_rewritten_order() {
    let mut state = FolderState::default();
    state.restore(
        vec![
            Folder {
                id: 1,
                name: "a".into(),
                members: vec!["w2".into(), "w3".into(), "w4".into()],
            },
            Folder {
                id: 2,
                name: "gone".into(),
                members: vec!["w9".into()],
            },
            Folder {
                id: 3,
                name: "empty".into(),
                members: vec![],
            },
        ],
        vec![
            Item::Workspace("w1".into()),
            Item::Folder(1),
            Item::Folder(2),
            Item::Workspace("w5".into()),
            Item::Folder(3),
            Item::Workspace("w6".into()),
        ],
        4,
    );
    let outcome = state.reconcile(vec![
        ws("w4"),
        ws("w1"),
        ws("w2"),
        ws("w5"),
        ws("w6"),
        ws("w7"),
    ]);
    assert_eq!(members(&state, 1), ["w4", "w2"]);
    assert_eq!(
        outcome.flat.as_deref(),
        Some(&["w4", "w2", "w1", "w5", "w6", "w7"].map(String::from)[..])
    );
    assert_eq!(order(&state), ["f1", "f2", "w1", "w5", "f3", "w6", "w7"]);
    assert_eq!(state.next_id(), 4);
}

#[test]
fn legacy_moves_never_change_membership() {
    let mut flat = vec![ws("w1"), ws("w2"), ws("w3"), ws("w4"), ws("w5")];
    let mut state = synced(flat.clone());
    let (_, outcome) = state
        .create("a".into(), &["w2".into(), "w3".into(), "w4".into()], None)
        .expect("create");
    apply(&mut flat, &outcome);
    let remaining = ["w1", "w2", "w4", "w5"];
    assert_eq!(state.clamp_flat_insert(&remaining, &["w3"], 0), 1);
    assert_eq!(state.clamp_flat_insert(&remaining, &["w3"], 4), 3);
    assert_eq!(state.clamp_flat_insert(&remaining, &["w3"], 2), 2);
    let remaining = ["w2", "w3", "w4", "w5"];
    assert_eq!(state.clamp_flat_insert(&remaining, &["w1"], 1), 0);
    assert_eq!(state.clamp_flat_insert(&remaining, &["w1"], 2), 3);
    let remaining = ["w1", "w2", "w3", "w4"];
    assert_eq!(state.clamp_flat_insert(&remaining, &["w5"], 3), 4);
    assert_eq!(state.clamp_flat_insert(&remaining, &["w5"], 0), 0);
}

#[test]
fn reconcile_is_stable_once_synced() {
    let mut flat = vec![
        ws("w1"),
        parent("w2", "repo"),
        ws("w3"),
        child("w4", "repo"),
    ];
    let mut state = synced(flat.clone());
    let (_, outcome) = state
        .create("a".into(), &["w2".into()], None)
        .expect("create");
    apply(&mut flat, &outcome);
    assert!(state.is_synced(flat.iter().map(|fact| {
        (
            fact.id.as_str(),
            fact.worktree
                .as_ref()
                .map(|worktree| (worktree.key.as_str(), worktree.linked)),
        )
    })));
    let revision = state.revision();
    let outcome = state.reconcile(flat.clone());
    assert_eq!(outcome, FolderOutcome::default());
    assert_eq!(state.revision(), revision);
}
