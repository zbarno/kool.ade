use super::*;

#[path = "relationships_tests/planning.rs"]
mod planning;

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
    assert_eq!(
        dependents[0].planning_item_uid,
        board.task_documents[1].identity.as_ref().unwrap().uid
    );
    assert_eq!(dependents[0].title, "TASK-2");
    assert_eq!(dependents[0].status, "Status not recorded");
    board.lifecycle_tasks.insert(
        board.task_documents[1].path.clone(),
        crate::core::lifecycle::TaskPhase::Blocked,
    );
    let refreshed = task_dependents(&board);
    assert_eq!(
        refreshed[&board.task_documents[0].path][0].status,
        "Blocked"
    );
}

#[test]
fn task_path_projection_rejects_ambiguous_duplicate_uids() {
    let mut board = crate::ui::planning_board::ViewModel::default();
    let batch = uuid::Uuid::new_v4().to_string();
    let unique = task_identity("TASK-UNIQUE", &batch);
    board.task_documents.push(task_doc(
        "unique.md",
        unique.clone(),
        crate::artifacts::task_docs::TaskMetadata::new(&unique, "root", Vec::new()).unwrap(),
    ));
    let duplicate = task_identity("TASK-DUP", &batch);
    for path in ["first.md", "second.md"] {
        board.task_documents.push(task_doc(
            path,
            duplicate.clone(),
            crate::artifacts::task_docs::TaskMetadata::new(
                &duplicate,
                "root",
                vec![unique.uid.clone()],
            )
            .unwrap(),
        ));
    }
    let paths = unique_task_paths(&board);
    assert_eq!(paths.len(), 1);
    assert_eq!(paths[&unique.uid], "planning/tasks/unique.md");
    assert!(!paths.contains_key(&duplicate.uid));
    let dependents = task_dependents(&board);
    assert!(!dependents.contains_key("planning/tasks/first.md"));
    assert!(!dependents.contains_key("planning/tasks/second.md"));
    assert_eq!(dependents["planning/tasks/unique.md"].len(), 1);
    assert_eq!(
        dependents["planning/tasks/unique.md"][0].title,
        "Related task unavailable"
    );
    assert_eq!(
        dependents["planning/tasks/unique.md"][0].status,
        "Relationship data incomplete"
    );
    assert_eq!(
        dependents["planning/tasks/unique.md"][0].planning_item_uid,
        duplicate.uid
    );
}

#[test]
fn task_child_projection_follows_explicit_parent_uids_through_nested_levels() {
    let mut board = crate::ui::planning_board::ViewModel::default();
    let batch = uuid::Uuid::new_v4().to_string();
    let root = task_identity("TASK-ROOT", &batch);
    let child = task_identity("TASK-CHILD", &batch);
    let grandchild = task_identity("TASK-LEAF", &batch);
    for (path, identity, parent_uid) in [
        ("root.md", root.clone(), None),
        ("child.md", child.clone(), Some(root.uid.as_str())),
        ("leaf.md", grandchild.clone(), Some(child.uid.as_str())),
    ] {
        let metadata =
            crate::artifacts::task_docs::TaskMetadata::new(&identity, "root", Vec::new())
                .unwrap()
                .with_parent_task_uid(parent_uid)
                .unwrap();
        board
            .task_documents
            .push(task_doc(path, identity, metadata));
    }
    let ambiguous_child = task_identity("TASK-DUP-CHILD", &batch);
    for path in ["ambiguous-one.md", "ambiguous-two.md"] {
        let metadata =
            crate::artifacts::task_docs::TaskMetadata::new(&ambiguous_child, "root", Vec::new())
                .unwrap()
                .with_parent_task_uid(Some(root.uid.as_str()))
                .unwrap();
        board
            .task_documents
            .push(task_doc(path, ambiguous_child.clone(), metadata));
    }
    let missing_identity = task_identity("TASK-MISSING-IDENTITY", &batch);
    let missing_metadata =
        crate::artifacts::task_docs::TaskMetadata::new(&missing_identity, "root", Vec::new())
            .unwrap()
            .with_parent_task_uid(Some(root.uid.as_str()))
            .unwrap();
    let mut missing_doc = task_doc(
        "missing-identity.md",
        missing_identity.clone(),
        missing_metadata,
    );
    missing_doc.identity = None;
    board.task_documents.push(missing_doc);

    let children = task_children(&board);
    assert!(
        children["planning/tasks/root.md"]
            .iter()
            .any(|relation| relation.planning_item_uid == child.uid)
    );
    assert_eq!(
        children["planning/tasks/child.md"][0].planning_item_uid,
        grandchild.uid
    );
    let unavailable = children["planning/tasks/root.md"]
        .iter()
        .find(|child| child.planning_item_uid == ambiguous_child.uid)
        .unwrap();
    assert_eq!(unavailable.title, "Related task unavailable");
    assert_eq!(unavailable.status, "Relationship data incomplete");
    let missing_identity = children["planning/tasks/root.md"]
        .iter()
        .find(|child| child.planning_item_uid == missing_identity.uid)
        .unwrap();
    assert_eq!(missing_identity.title, "Related task unavailable");
    assert_eq!(missing_identity.status, "Relationship data incomplete");
    assert!(!children.contains_key("planning/tasks/leaf.md"));
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
