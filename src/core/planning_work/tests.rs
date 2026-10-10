use super::*;
use crate::artifacts::planning_store::PlanningRoot;

fn root() -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!(
        "koolade-work-schema-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&path).unwrap();
    assert!(
        std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(&path)
            .status()
            .unwrap()
            .success()
    );
    path
}

#[test]
fn legacy_work_reads_are_pure_and_upgrade_with_revision_fencing() {
    let root = root();
    let path = root.join(FILE);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        &path,
        r#"[{"key":"planning:a","title":"Plan A","request":"A","column":3,"feature":"CHG-001","detail":"Waiting"}]"#,
    )
    .unwrap();

    let store = root.planning_store();
    let original = std::fs::read(&path).unwrap();
    let expected_revision = store.revision().unwrap();
    let work = load(&root).unwrap();
    assert_eq!(work.len(), 1);
    assert_eq!(work[0].status, WorkStatus::NeedsAttention);
    assert_eq!(work[0].kind, WorkKind::Feature);
    assert_eq!(work[0].feature_id.as_deref(), Some("CHG-001"));
    assert!(uuid::Uuid::parse_str(&work[0].uid).is_ok());
    assert_eq!(load(&root).unwrap()[0].uid, work[0].uid);
    assert_eq!(std::fs::read(&path).unwrap(), original);
    assert_eq!(store.revision().unwrap(), expected_revision);

    let (migrated_work, migration_revision) = load_expected(&root, &expected_revision).unwrap();
    assert_eq!(migrated_work, work);
    let migrated: serde_json::Value =
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(migrated["schemaVersion"], SCHEMA_VERSION);
    assert_eq!(migrated["items"][0]["status"], "needs_attention");
    assert!(migrated["items"][0].get("column").is_none());
    assert_eq!(migration_revision, store.revision().unwrap());

    let current_bytes = std::fs::read(root.join(FILE)).unwrap();
    assert!(load_expected(&root, &expected_revision).is_err());
    assert_eq!(std::fs::read(root.join(FILE)).unwrap(), current_bytes);
    let (loaded_again, unchanged_revision) = load_expected(&root, &migration_revision).unwrap();
    assert_eq!(loaded_again, work);
    assert_eq!(unchanged_revision, migration_revision);
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
fn task_routing_overrides_roundtrip_with_parent_provenance_and_manager_is_rejected() {
    use crate::persistence::harness_settings::{IMPLEMENTATION, QA, WorkRoute};
    let root = root();
    let mut parent = Work::new(
        "task:parent".into(),
        "Parent".into(),
        "Plan".into(),
        "Open".into(),
    );
    parent.feature_id = Some("F-37".into());
    parent.routing_overrides = std::collections::BTreeMap::from([
        (
            IMPLEMENTATION.into(),
            WorkRoute {
                harness: "codex".into(),
                model: Some("gpt-model".into()),
            },
        ),
        (
            QA.into(),
            WorkRoute {
                harness: "claude".into(),
                model: None,
            },
        ),
    ]);
    let mut child = Work::new(
        "task:child".into(),
        "Child".into(),
        "Follow up".into(),
        "Open".into(),
    );
    child.parent_uid = Some(parent.uid.clone());
    child.routing_inherited_from = Some(parent.uid.clone());
    child.routing_overrides = parent.routing_overrides.clone();
    save(&root, &[parent.clone(), child.clone()]).unwrap();

    let restored = load(&root).unwrap();
    assert_eq!(restored[1].routing_overrides, child.routing_overrides);
    assert_eq!(
        restored[1].routing_inherited_from.as_deref(),
        Some(parent.uid.as_str())
    );
    let snapshot = routing_for_feature(&root, "F-37", Some(&parent.uid)).unwrap();
    assert_eq!(snapshot.overrides, parent.routing_overrides);
    assert_eq!(
        snapshot.source_work_uid.as_deref(),
        Some(parent.uid.as_str())
    );

    child.routing_overrides.insert(
        crate::persistence::harness_settings::MANAGER.into(),
        WorkRoute {
            harness: "pi".into(),
            model: None,
        },
    );
    assert!(validate(&[parent, child]).is_err());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn discovered_refresh_tasks_keep_questions_in_attention_and_findings_in_todo() {
    let root = root();
    let content = append_discovered(
        &root,
        &[
            crate::harness::PlanningTaskDraft {
                title: "Clarify authentication behavior".into(),
                description: "README.md conflicts with src/auth.rs.".into(),
                kind: WorkKind::Question,
                status: WorkStatus::NeedsAttention,
            },
            crate::harness::PlanningTaskDraft {
                title: "Triage possible token leak".into(),
                description: "src/auth.rs logs a token-shaped value; verify exposure.".into(),
                kind: WorkKind::Bug,
                status: WorkStatus::Todo,
            },
        ],
    )
    .unwrap()
    .unwrap();
    let file: serde_json::Value = serde_json::from_str(&content).unwrap();
    let items = file["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0]["status"], "needs_attention");
    assert_eq!(items[0]["kind"], "question");
    assert_eq!(items[1]["status"], "todo");
    assert_eq!(items[1]["kind"], "bug");
    assert!(
        items[1]["detail"]
            .as_str()
            .unwrap()
            .contains("triage before acting")
    );
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
            .contains("unless expected behavior is genuinely unclear")
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
