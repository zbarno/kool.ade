use super::*;

#[test]
fn related_cards_follow_parent_feature_dependency_and_batch_links() {
    let mut board = crate::ui::planning_board::ViewModel::default();
    let feature_uid = uuid::Uuid::new_v4().to_string();
    let mut parent = crate::core::planning_work::Work::new(
        "feature:F12".into(),
        "Feature planning".into(),
        "Plan the feature".into(),
        "".into(),
    );
    parent.feature_id = Some("F12".into());
    parent.feature_uid = Some(feature_uid.clone());
    let mut child = crate::core::planning_work::Work::new(
        "question:F12".into(),
        "Open question".into(),
        "Resolve the question".into(),
        "".into(),
    );
    child.parent_uid = Some(parent.uid.clone());
    child.feature_uid = Some(feature_uid);

    let blocker = crate::domain::item::OpenItem::new(
        "CLR-001".into(),
        crate::domain::Priority::Normal,
        crate::domain::ItemKind::Question,
        "Product".into(),
        None,
        "Choose a behavior".into(),
        "The implementation needs this decision".into(),
    );
    let mut dependent = crate::domain::item::OpenItem::new(
        "CLR-002".into(),
        crate::domain::Priority::Normal,
        crate::domain::ItemKind::Question,
        "Product".into(),
        None,
        "Resolve a follow-up".into(),
        "This follows the earlier decision".into(),
    );
    dependent.blocked_by = vec![blocker.id.clone()];

    board.planning_work.extend([parent.clone(), child.clone()]);
    board
        .planning_items
        .extend([blocker.clone(), dependent.clone()]);
    let map = build(&board);
    assert!(map[&parent.key].contains(&child.key));
    assert!(map[&blocker.id].contains(&dependent.id));
}

#[test]
fn task_map_planning_work_projection_follows_explicit_parent_uids() {
    let mut parent = crate::core::planning_work::Work::new(
        "feature:F12".into(),
        "Feature planning".into(),
        "Plan feature".into(),
        String::new(),
    );
    parent.feature_uid = Some("feature-uid".into());
    let mut child = crate::core::planning_work::Work::new(
        "question:F12".into(),
        "Release decision".into(),
        "Resolve decision".into(),
        String::new(),
    );
    child.parent_uid = Some(parent.uid.clone());
    let unrelated = crate::core::planning_work::Work::new(
        "feature:F13".into(),
        "Unrelated feature".into(),
        "Plan another feature".into(),
        String::new(),
    );

    let relations = planning_work_relations(&[parent.clone(), child.clone(), unrelated]);
    let related = &relations["feature-uid"];
    assert_eq!(related.len(), 2);
    assert!(related.iter().any(|node| node.0 == parent.uid));
    assert!(
        related
            .iter()
            .any(|node| node.0 == child.uid && node.4.as_deref() == Some(parent.uid.as_str()))
    );
}

#[test]
fn ambiguous_planning_work_uids_are_not_navigable() {
    let original = crate::core::planning_work::Work::new(
        "feature:F12".into(),
        "Feature planning".into(),
        "Plan feature".into(),
        String::new(),
    );
    let mut duplicate = original.clone();
    duplicate.key = "different-record".into();
    let keys = unique_planning_work_keys(&[original.clone(), duplicate.clone()]);

    assert!(!keys.contains_key(&original.uid));
    assert!(keys.is_empty());
}

#[test]
fn task_map_planning_items_follow_blocker_ids_and_current_authority() {
    use crate::domain::item::{Authority, ItemKind, ItemStatus, OpenItem, Priority};
    let mut prerequisite = OpenItem::new(
        "CLR-001".into(),
        Priority::Blocking,
        ItemKind::Question,
        "Product".into(),
        None,
        "Choose a policy".into(),
        "The implementation depends on it".into(),
    );
    prerequisite.feature_uid = Some("feature-uid".into());
    prerequisite.status = ItemStatus::Resolved;
    let mut decision = OpenItem::new(
        "CLR-002".into(),
        Priority::High,
        ItemKind::Ambiguity,
        "Product".into(),
        None,
        "Confirm the fallback".into(),
        "The chosen policy defines fallback behavior".into(),
    );
    decision.blocked_by = vec![prerequisite.id.clone()];
    decision.authority = Authority::Review;
    let unrelated = OpenItem::new(
        "CLR-003".into(),
        Priority::Normal,
        ItemKind::Question,
        "General".into(),
        None,
        "Unrelated question".into(),
        "No relation to this feature".into(),
    );

    let relations = planning_item_relations(
        &[prerequisite.clone(), decision.clone(), unrelated],
        &std::collections::HashMap::new(),
        |_| false,
    );
    let related = &relations["feature-uid"];
    assert_eq!(related.len(), 2);
    assert!(
        related
            .iter()
            .any(|node| node.planning_item_id == prerequisite.id && node.status == "Resolved")
    );
    assert!(related.iter().any(|node| {
        node.planning_item_id == decision.id
            && node.status == "Needs user review"
            && node.blocked_by == vec![prerequisite.id.clone()]
    }));

    let mut saved_decision = decision.clone();
    saved_decision.feature_uid = Some("feature-uid".into());
    let answer_saved =
        planning_item_relations(&[saved_decision], &std::collections::HashMap::new(), |_| {
            true
        });
    assert_eq!(answer_saved["feature-uid"][0].status, "Answer saved");
}

#[test]
fn legacy_feature_id_links_are_visible_and_feature_uid_conflicts_are_flagged() {
    use crate::domain::item::{ItemKind, OpenItem, Priority};
    let mut feature = crate::core::planning_work::Work::new(
        "feature:F12".into(),
        "Feature planning".into(),
        "Plan feature".into(),
        String::new(),
    );
    feature.feature_id = Some("F12".into());
    feature.feature_uid = Some("feature-uid".into());
    let feature_uids = unique_feature_uids_by_id(&[feature]);
    let mut legacy = OpenItem::new(
        "CLR-010".into(),
        Priority::Normal,
        ItemKind::Question,
        "Product".into(),
        None,
        "Legacy linked question".into(),
        "Old artifact has no feature UID".into(),
    );
    legacy.feature_id = Some("F12".into());
    let mut conflict = OpenItem::new(
        "CLR-011".into(),
        Priority::Normal,
        ItemKind::Question,
        "Product".into(),
        None,
        "Conflicting feature reference".into(),
        "Its UID and feature ID disagree".into(),
    );
    conflict.feature_id = Some("F12".into());
    conflict.feature_uid = Some("different-feature-uid".into());

    let relations = planning_item_relations(&[legacy, conflict.clone()], &feature_uids, |_| false);
    let linked = &relations["feature-uid"];
    assert!(linked.iter().any(|node| node.planning_item_id == "CLR-010"
        && node.legacy_feature_link
        && !node.feature_reference_conflict));
    assert!(
        linked.iter().any(|node| {
            node.planning_item_id == conflict.id && node.feature_reference_conflict
        })
    );
    assert!(
        relations["different-feature-uid"]
            .iter()
            .any(|node| node.planning_item_id == conflict.id && node.feature_reference_conflict)
    );
}

#[test]
fn ambiguous_planning_item_ids_are_not_navigable() {
    let item = crate::domain::item::OpenItem::new(
        "CLR-001".into(),
        crate::domain::Priority::Normal,
        crate::domain::ItemKind::Question,
        "General".into(),
        None,
        "One question".into(),
        "Context".into(),
    );
    assert!(unique_planning_item_keys(&[item.clone(), item.clone()]).is_empty());
}

#[test]
fn task_cards_link_through_shared_batch_and_feature_path() {
    let mut board = crate::ui::planning_board::ViewModel::default();
    let batch_uid = uuid::Uuid::new_v4().to_string();
    let first_identity = task_identity("TASK-001", &batch_uid);
    let second_identity = task_identity("TASK-002", &batch_uid);
    let first_metadata =
        crate::artifacts::task_docs::TaskMetadata::new(&first_identity, "root", Vec::new())
            .unwrap();
    let second_metadata = crate::artifacts::task_docs::TaskMetadata::new(
        &second_identity,
        "root",
        vec![first_identity.uid.clone()],
    )
    .unwrap();
    board.task_documents.extend([
        task_doc(
            "F12-feature-01/F12-TASK-001-one.md",
            first_identity,
            first_metadata,
        ),
        task_doc(
            "F12-feature-01/F12-TASK-002-two.md",
            second_identity,
            second_metadata,
        ),
    ]);
    let map = build(&board);
    let first = &board.task_documents[0].path;
    let second = &board.task_documents[1].path;
    assert!(map[first].contains(second));
    assert!(map[second].contains(first));
}

#[test]
fn task_cards_link_to_chg_feature_decisions() {
    let mut board = crate::ui::planning_board::ViewModel::default();
    let mut item = crate::domain::item::OpenItem::new(
        "CLR-900".into(),
        crate::domain::Priority::Blocking,
        crate::domain::ItemKind::Question,
        "Product".into(),
        None,
        "Choose a release behavior".into(),
        "This decision gates implementation".into(),
    );
    item.feature_id = Some("CHG-900".into());
    board.planning_items.push(item);
    let mut identity =
        crate::domain::ArtifactIdentity::new("CHG-900-TASK-1", "Preserve implementation");
    identity.parent_uid = Some(uuid::Uuid::new_v4().to_string());
    let metadata =
        crate::artifacts::task_docs::TaskMetadata::new(&identity, "root", Vec::new()).unwrap();
    board.task_documents.push(task_doc(
        "CHG-900-migration/CHG-900-TASK-preserve-implementation.md",
        identity,
        metadata,
    ));

    let map = build(&board);
    assert!(map["CLR-900"].contains(&board.task_documents[0].path));
}

#[test]
fn downstream_task_projection_uses_only_recorded_dependency_uids() {
    let mut board = crate::ui::planning_board::ViewModel::default();
    let batch = uuid::Uuid::new_v4().to_string();
    let dependency = task_identity("TASK-1", &batch);
    let dependent = task_identity("TASK-2", &batch);
    let unrelated = task_identity("TASK-3", &batch);
    let dependency_uid = dependency.uid.clone();
    board.task_documents.push(task_doc(
        "TASK-1.md",
        dependency.clone(),
        crate::artifacts::task_docs::TaskMetadata::new(&dependency, "root", Vec::new()).unwrap(),
    ));
    board.task_documents.push(task_doc(
        "TASK-2.md",
        dependent.clone(),
        crate::artifacts::task_docs::TaskMetadata::new(&dependent, "root", vec![dependency_uid])
            .unwrap(),
    ));
    board.task_documents.push(task_doc(
        "TASK-3.md",
        unrelated.clone(),
        crate::artifacts::task_docs::TaskMetadata::new(&unrelated, "root", Vec::new()).unwrap(),
    ));

    let projection = task_dependents(&board);
    let dependents = &projection[&board.task_documents[0].path];
    assert_eq!(dependents.len(), 1);
    assert_eq!(dependents[0].0, board.task_documents[1].path);
    assert_eq!(dependents[0].1, "TASK-2");
    assert_eq!(dependents[0].2, "Status not recorded");
    board.lifecycle_tasks.insert(
        board.task_documents[1].path.clone(),
        crate::core::lifecycle::TaskPhase::Blocked,
    );
    let refreshed = task_dependents(&board);
    assert_eq!(refreshed[&board.task_documents[0].path][0].2, "Blocked");
}

#[test]
fn task_path_projection_rejects_ambiguous_duplicate_uids() {
    let mut board = crate::ui::planning_board::ViewModel::default();
    let batch = uuid::Uuid::new_v4().to_string();
    let duplicate = task_identity("TASK-DUP", &batch);
    for path in ["first.md", "second.md"] {
        board.task_documents.push(task_doc(
            path,
            duplicate.clone(),
            crate::artifacts::task_docs::TaskMetadata::new(&duplicate, "root", Vec::new()).unwrap(),
        ));
    }
    let unique = task_identity("TASK-UNIQUE", &batch);
    board.task_documents.push(task_doc(
        "unique.md",
        unique.clone(),
        crate::artifacts::task_docs::TaskMetadata::new(&unique, "root", Vec::new()).unwrap(),
    ));
    let dependent = task_identity("TASK-DEP", &batch);
    board.task_documents.push(task_doc(
        "dependent.md",
        dependent.clone(),
        crate::artifacts::task_docs::TaskMetadata::new(
            &dependent,
            "root",
            vec![duplicate.uid.clone()],
        )
        .unwrap(),
    ));
    let paths = unique_task_paths(&board);
    assert_eq!(paths.len(), 2);
    assert_eq!(paths[&unique.uid], "planning/tasks/unique.md");
    assert_eq!(paths[&dependent.uid], "planning/tasks/dependent.md");
    assert!(!paths.contains_key(&duplicate.uid));
    let dependents = task_dependents(&board);
    assert!(!dependents.contains_key("planning/tasks/first.md"));
    assert!(!dependents.contains_key("planning/tasks/second.md"));
    assert!(dependents.values().all(|items| {
        items
            .iter()
            .all(|(path, _, _)| path != "planning/tasks/dependent.md")
    }));
}

fn task_identity(id: &str, batch_uid: &str) -> crate::domain::ArtifactIdentity {
    let mut identity = crate::domain::ArtifactIdentity::new(id, id);
    identity.parent_uid = Some(batch_uid.into());
    identity
}

fn task_doc(
    path: &str,
    identity: crate::domain::ArtifactIdentity,
    metadata: crate::artifacts::task_docs::TaskMetadata,
) -> crate::artifacts::task_docs::TaskDocument {
    crate::artifacts::task_docs::TaskDocument {
        path: format!("planning/tasks/{path}"),
        title: identity.title.clone(),
        text: String::new(),
        identity: Some(identity),
        metadata: Some(metadata),
        task_state: None,

        metadata_error: None,
    }
}
