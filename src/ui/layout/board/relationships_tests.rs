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
        path: format!(".koolade-packet/planning/tasks/{path}"),
        title: identity.title.clone(),
        text: String::new(),
        identity: Some(identity),
        metadata: Some(metadata),
        task_state: None,

        metadata_error: None,
    }
}
