use super::*;

fn root() -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!(
        "packet-work-schema-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&path).unwrap();
    path
}

#[test]
fn legacy_work_is_migrated_once_to_versioned_typed_records() {
    let root = root();
    let path = root.join(FILE);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        &path,
        r#"[{"key":"planning:a","title":"Plan A","request":"A","column":3,"feature":"CHG-001","detail":"Waiting"}]"#,
    )
    .unwrap();

    let work = load(&root).unwrap();
    assert_eq!(work.len(), 1);
    assert_eq!(work[0].status, WorkStatus::NeedsAttention);
    assert_eq!(work[0].kind, WorkKind::Feature);
    assert_eq!(work[0].feature_id.as_deref(), Some("CHG-001"));
    assert!(uuid::Uuid::parse_str(&work[0].uid).is_ok());
    let migrated: serde_json::Value =
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(migrated["schemaVersion"], SCHEMA_VERSION);
    assert_eq!(migrated["items"][0]["status"], "needs_attention");
    assert!(migrated["items"][0].get("column").is_none());

    assert_eq!(load(&root).unwrap()[0].uid, work[0].uid);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn typed_work_roundtrips_kind_parent_and_feature_uid() {
    let root = root();
    let mut item = Work::new(
        "task:question".into(),
        "Question".into(),
        "Why?".into(),
        "Open".into(),
    );
    item.kind = WorkKind::Question;
    item.status = WorkStatus::NeedsAttention;
    item.parent_uid = Some(uuid::Uuid::new_v4().to_string());
    item.feature_uid = Some(uuid::Uuid::new_v4().to_string());
    item.follow_up_task = Some(FollowUpTaskOffer {
        title: "Add hosted provider support".into(),
        description: "Plan the provider integration.".into(),
    });
    save(&root, &[item.clone()]).unwrap();
    let loaded = load(&root).unwrap();
    assert_eq!(loaded, vec![item]);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn work_uid_and_parent_link_survive_task_rename_and_reload() {
    let root = root();
    let parent = Work::new(
        "task:parent".into(),
        "Original parent title".into(),
        "Plan a feature".into(),
        "Open".into(),
    );
    let mut child = Work::new(
        "task:child".into(),
        "Original child title".into(),
        "Answer a related question".into(),
        "Open".into(),
    );
    child.parent_uid = Some(parent.uid.clone());
    let parent_uid = parent.uid.clone();
    let child_uid = child.uid.clone();
    save(&root, &[parent, child]).unwrap();

    let mut renamed = load(&root).unwrap();
    renamed[0].key = "task:renamed-parent".into();
    renamed[0].title = "Renamed parent title".into();
    renamed[1].key = "task:renamed-child".into();
    renamed[1].title = "Renamed child title".into();
    save(&root, &renamed).unwrap();
    let restored = load(&root).unwrap();
    assert_eq!(restored[0].uid, parent_uid);
    assert_eq!(restored[1].uid, child_uid);
    assert_eq!(restored[1].parent_uid.as_deref(), Some(parent_uid.as_str()));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn statuses_project_to_existing_board_without_persisting_numeric_columns() {
    assert_eq!(WorkStatus::Todo.board_column(), 0);
    assert_eq!(WorkStatus::InProgress.board_column(), 1);
    assert_eq!(WorkStatus::InReview.board_column(), 2);
    assert_eq!(WorkStatus::NeedsAttention.board_column(), 3);
    assert_eq!(WorkStatus::Done.board_column(), 4);
}

#[test]
fn task_kinds_carry_concise_kind_specific_planning_guidance() {
    assert!(WorkKind::Feature.planning_guidance().contains("concise"));
    assert!(
        WorkKind::Bug
            .planning_guidance()
            .contains("Inspect current behavior")
    );
    assert!(
        WorkKind::Bug
            .planning_guidance()
            .contains("unless the expected behavior is genuinely unclear")
    );
    assert!(
        WorkKind::NewProject
            .planning_guidance()
            .contains("progressively")
    );
    assert!(
        WorkKind::Question
            .planning_guidance()
            .contains("answer directly")
    );
}
