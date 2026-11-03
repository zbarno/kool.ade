use super::*;

#[path = "metadata_tests/task_parent_persistence.rs"]
mod task_parent_persistence;

fn init_git(root: &std::path::Path) {
    assert!(
        std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(root)
            .status()
            .unwrap()
            .success()
    );
}

fn identified_story() -> (crate::domain::ArtifactIdentity, String, String) {
    let batch = crate::domain::ArtifactIdentity::new("BATCH-001", "Batch");
    let mut task = crate::domain::ArtifactIdentity::new("TASK-001", "Story");
    task.parent_uid = Some(batch.uid.clone());
    (task, batch.uid, uuid::Uuid::new_v4().to_string())
}

#[test]
fn execution_metadata_round_trips_and_body_formatting_cannot_change_it() {
    let (identity, batch_uid, dependency) = identified_story();
    let metadata = TaskMetadata::new(&identity, "api", vec![dependency.clone()]).unwrap();
    let markdown = embed(
        "# Story\n\nRepository: web\n\n## Dependencies\n\nNone.\n",
        &metadata,
    )
    .unwrap();
    let parsed = parse(&markdown).unwrap().unwrap();
    assert_eq!(parsed, metadata);
    assert_eq!(parsed.batch_uid, batch_uid);
    assert_eq!(parsed.dependency_uids, vec![dependency]);
    assert!(visible_content(&markdown).contains("Repository: web"));
    assert!(!visible_content(&markdown).starts_with("---"));

    let edited_body = markdown
        .replace("Repository: web", "Repository: root")
        .replace("## Dependencies", "## Prerequisites");
    assert_eq!(parse(&edited_body).unwrap(), Some(metadata));
}

#[test]
fn invalid_metadata_fails_closed_and_legacy_parser_is_bounded_to_dependency_section() {
    let malformed = "---\nkoolade-task: {broken}\n---\n# Story\n";
    assert!(parse(malformed).is_err());
    let identity = crate::domain::ArtifactIdentity::new("TASK-1", "Story");
    let bad_repo = TaskMetadata {
        schema_version: 1,
        uid: identity.uid.clone(),
        batch_uid: uuid::Uuid::new_v4().to_string(),
        repository_id: "../api".into(),
        task_intent_uid: None,
        task_intent_cycle_uid: None,
        dependency_uids: vec![],
        parent_task_uid: None,
        source_branch: None,
        destination_branch: None,
        source_sha: None,
        parallelizable: None,
        routing_overrides: Default::default(),
        routing_source_uid: None,
        routing_inherited_from: None,
    };
    assert!(bad_repo.validate(None).is_err());

    let markdown =
        "# Task\n\n[not a dependency](other.md)\n\n## Dependencies\n\n- [Task](001-task.md)\n";
    assert_eq!(legacy_dependencies(markdown).unwrap(), vec!["001-task.md"]);
}

#[test]
fn branch_targets_round_trip_and_legacy_metadata_keeps_optional_intent_empty() {
    let (identity, _, _) = identified_story();
    let targets = crate::core::workflow::BranchTargets {
        source: "feature/platform-refactor".into(),
        destination: "integration".into(),
    };
    let metadata = TaskMetadata::new(&identity, "root", vec![])
        .unwrap()
        .with_branch_targets(Some(&targets))
        .unwrap();
    assert_eq!(
        metadata.source_branch.as_deref(),
        Some("feature/platform-refactor")
    );
    assert_eq!(metadata.destination_branch.as_deref(), Some("integration"));

    let parent_uid = uuid::Uuid::new_v4().hyphenated().to_string();
    let child = TaskMetadata::new(&identity, "root", vec![])
        .unwrap()
        .with_parent_task_uid(Some(&parent_uid))
        .unwrap();
    assert_eq!(child.parent_task_uid.as_deref(), Some(parent_uid.as_str()));
    let encoded = serde_json::to_string(&child).unwrap();
    let reopened: TaskMetadata = serde_json::from_str(&encoded).unwrap();
    assert_eq!(reopened, child);

    let legacy = serde_json::json!({
        "schemaVersion": 1,
        "uid": identity.uid.clone(),
        "batchUid": identity.parent_uid.clone().unwrap(),
        "repositoryId": "root",
        "dependencyUids": []
    });
    let legacy: TaskMetadata = serde_json::from_value(legacy).unwrap();
    legacy.validate(None).unwrap();
    assert_eq!(legacy.source_branch, None);
    assert_eq!(legacy.destination_branch, None);

    let legacy_snake_case: TaskMetadata = serde_json::from_value(serde_json::json!({
        "schemaVersion": 1,
        "uid": identity.uid,
        "batchUid": uuid::Uuid::new_v4().to_string(),
        "repositoryId": "root",
        "dependencyUids": [],
        "source_branch": "release/1.2",
        "destination_branch": "develop"
    }))
    .unwrap();
    assert_eq!(
        legacy_snake_case.source_branch.as_deref(),
        Some("release/1.2")
    );
    assert_eq!(
        legacy_snake_case.destination_branch.as_deref(),
        Some("develop")
    );
}

#[test]
fn legacy_task_intent_resolves_from_its_authenticated_routing_source() {
    let (identity, _, _) = identified_story();
    let mut root = crate::core::planning_work::Work::new(
        "planning:root".into(),
        "Feature request".into(),
        "Preserve the user's request".into(),
        String::new(),
    );
    root.task_intent_uid = Some(root.uid.clone());
    let expected_uid = root.uid.clone();
    let metadata = TaskMetadata::new(&identity, "root", vec![])
        .unwrap()
        .with_task_routing(&Default::default(), Some(&root.uid), None)
        .unwrap();
    let document = crate::artifacts::task_docs::TaskDocument {
        path: "planning/tasks/fixture/001-story.md".into(),
        title: "Story".into(),
        text: "# Story".into(),
        identity: Some(identity),
        metadata: Some(metadata),
        task_state: None,
        metadata_error: None,
    };
    assert_eq!(
        crate::artifacts::task_docs::task_intent_uid_for_document(
            &document,
            std::slice::from_ref(&document),
            std::slice::from_ref(&root),
        ),
        Some(expected_uid)
    );
}

#[test]
fn task_intent_link_is_persisted_separately_from_batch_and_task_parent() {
    let (identity, batch_uid, dependency) = identified_story();
    let intent_uid = uuid::Uuid::new_v4().to_string();
    let metadata = TaskMetadata::new(&identity, "api", vec![dependency.clone()])
        .unwrap()
        .with_task_intent_uid(Some(&intent_uid))
        .unwrap();
    let persisted = parse(&embed("# Story\n", &metadata).unwrap())
        .unwrap()
        .unwrap();

    assert_eq!(
        persisted.task_intent_uid.as_deref(),
        Some(intent_uid.as_str())
    );
    assert_eq!(persisted.batch_uid, batch_uid);
    assert_eq!(persisted.parent_task_uid, None);
    assert_eq!(persisted.dependency_uids, vec![dependency]);
    assert!(
        metadata
            .clone()
            .with_task_intent_uid(Some("not-a-uid"))
            .is_err()
    );
}

#[test]
fn task_intent_cycle_is_persisted_and_resolved_from_its_routing_source() {
    let (identity, _, _) = identified_story();
    let root_uid = uuid::Uuid::new_v4().to_string();
    let cycle_uid = uuid::Uuid::new_v4().to_string();
    let mut cycle_work = crate::core::planning_work::Work::new(
        "task:cycle-work".into(),
        "Follow-up cycle".into(),
        "Keep this cycle scoped".into(),
        String::new(),
    );
    cycle_work.uid = uuid::Uuid::new_v4().to_string();
    cycle_work.task_intent_uid = Some(root_uid.clone());
    cycle_work.task_intent_cycle_uid = Some(cycle_uid.clone());
    let mut root_work = crate::core::planning_work::Work::new(
        format!("task:{root_uid}"),
        "Root intent".into(),
        "Original request".into(),
        String::new(),
    );
    root_work.uid = root_uid.clone();
    root_work.task_intent_uid = Some(root_uid.clone());
    root_work.task_intent_cycle_uid = Some(cycle_uid.clone());
    root_work
        .task_intent_cycles
        .push(crate::core::planning_work::TaskIntentCycleV1 {
            uid: cycle_uid.clone(),
            sequence: 2,
            goal: "Keep this cycle scoped".into(),
            work_uid: cycle_work.uid.clone(),
            started_at_ms: Some(1),
        });
    let metadata = TaskMetadata::new(&identity, "root", vec![])
        .unwrap()
        .with_task_intent_uid(Some(&root_uid))
        .unwrap()
        .with_task_intent_cycle_uid(Some(&cycle_uid))
        .unwrap()
        .with_task_routing(&Default::default(), Some(&cycle_work.uid), None)
        .unwrap();
    let mut legacy_metadata = metadata.clone();
    legacy_metadata.task_intent_cycle_uid = None;
    let document = crate::artifacts::task_docs::TaskDocument {
        path: "planning/tasks/fixture/001-story.md".into(),
        title: "Story".into(),
        text: "# Story".into(),
        identity: Some(identity),
        metadata: Some(legacy_metadata),
        task_state: None,
        metadata_error: None,
    };
    let persisted = parse(&embed("# Story\n", &metadata).unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(
        persisted.task_intent_cycle_uid.as_deref(),
        Some(cycle_uid.as_str())
    );
    assert_eq!(
        crate::artifacts::task_docs::task_intent_cycle_uid_for_document(
            &document,
            std::slice::from_ref(&document),
            &[root_work, cycle_work],
        ),
        Some(cycle_uid)
    );
}

#[test]
fn generated_task_routing_round_trips_with_parent_attribution_and_rejects_manager() {
    use crate::persistence::harness_settings::{
        IMPLEMENTATION, INVESTIGATION, MANAGER, QA, RECONCILIATION, WorkRoute,
    };
    let (identity, _, _) = identified_story();
    let source_uid = uuid::Uuid::new_v4().to_string();
    let parent_uid = uuid::Uuid::new_v4().to_string();
    let routes = std::collections::BTreeMap::from([
        (
            IMPLEMENTATION.into(),
            WorkRoute {
                harness: "codex".into(),
                model: Some("gpt-task".into()),
            },
        ),
        (
            QA.into(),
            WorkRoute {
                harness: "claude".into(),
                model: None,
            },
        ),
        (
            INVESTIGATION.into(),
            WorkRoute {
                harness: "opencode".into(),
                model: Some("openai/gpt-test".into()),
            },
        ),
        (
            RECONCILIATION.into(),
            WorkRoute {
                harness: "codex".into(),
                model: None,
            },
        ),
    ]);
    let metadata = TaskMetadata::new(&identity, "root", vec![])
        .unwrap()
        .with_task_routing(&routes, Some(&source_uid), Some(&parent_uid))
        .unwrap();
    let saved = embed("# Story\n", &metadata).unwrap();
    let loaded = parse(&saved).unwrap().unwrap();
    assert_eq!(loaded.routing_overrides, routes);
    assert_eq!(
        loaded.routing_source_uid.as_deref(),
        Some(source_uid.as_str())
    );
    assert_eq!(
        loaded.routing_inherited_from.as_deref(),
        Some(parent_uid.as_str())
    );

    let mut invalid = metadata;
    invalid.routing_overrides.insert(
        MANAGER.into(),
        WorkRoute {
            harness: "pi".into(),
            model: None,
        },
    );
    assert!(invalid.validate(None).is_err());
}

#[test]
fn generated_task_stories_copy_the_batch_source_and_destination() {
    let root = std::env::temp_dir().join(format!(
        "koolade-branch-metadata-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).unwrap();
    init_git(&root);
    let targets = crate::core::workflow::BranchTargets {
        source: "release/2.1".into(),
        destination: "integration".into(),
    };
    let batch = crate::core::workflow::TaskBatch {
        brief: crate::core::workflow::InterviewBrief {
            feature_name: "Branch target feature".into(),
            ..Default::default()
        },
        specification: "# Branch target feature".into(),
        feature_id: None,
        contract: None,
        branch_targets: Some(targets),
        task_routing: Default::default(),
        stories: vec![crate::core::workflow::TaskStory {
            source_sha: Some("a".repeat(40)),
            parallelizable: Some(false),
            title: "Implement branch selection".into(),
            ..Default::default()
        }],
    };
    crate::artifacts::task_docs::save_progress(&root, "branch-run", &batch, 1).unwrap();
    let docs =
        crate::artifacts::task_docs::load_board(&root, &crate::core::workflow::Workflow::default());
    let metadata = docs[0].metadata.as_ref().unwrap();
    assert_eq!(metadata.source_branch.as_deref(), Some("release/2.1"));
    assert_eq!(metadata.destination_branch.as_deref(), Some("integration"));
    assert_eq!(
        metadata.source_sha.as_deref(),
        Some("a".repeat(40).as_str())
    );
    assert_eq!(metadata.parallelizable, Some(false));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn decomposer_task_uid_survives_progress_replay_and_is_deduplicated() {
    let root = std::env::temp_dir().join(format!(
        "koolade-decomposer-uid-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).unwrap();
    init_git(&root);
    let graph_uid = uuid::Uuid::new_v4().hyphenated().to_string();
    let batch = crate::core::workflow::TaskBatch {
        brief: crate::core::workflow::InterviewBrief {
            feature_name: "Decomposer UID fixture".into(),
            ..Default::default()
        },
        specification: "# Decomposer UID fixture".into(),
        feature_id: None,
        contract: None,
        branch_targets: None,
        task_routing: Default::default(),
        stories: vec![crate::core::workflow::TaskStory {
            graph_uid: Some(graph_uid.clone()),
            title: "Persist graph identity".into(),
            ..Default::default()
        }],
    };
    crate::artifacts::task_docs::save_progress(&root, "decomposer-run", &batch, 1).unwrap();
    crate::artifacts::task_docs::save_progress(&root, "decomposer-run", &batch, 1).unwrap();
    let docs =
        crate::artifacts::task_docs::load_board(&root, &crate::core::workflow::Workflow::default());
    let tasks = docs
        .iter()
        .filter(|document| document.metadata.is_some())
        .collect::<Vec<_>>();
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].identity.as_ref().unwrap().uid, graph_uid);
    assert_eq!(tasks[0].metadata.as_ref().unwrap().uid, graph_uid);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn generated_task_stories_embed_the_batch_routing_snapshot() {
    use crate::persistence::harness_settings::{IMPLEMENTATION, WorkRoute};
    let root = std::env::temp_dir().join(format!(
        "koolade-task-routing-metadata-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).unwrap();
    init_git(&root);
    let source_uid = uuid::Uuid::new_v4().to_string();
    let inherited_from = uuid::Uuid::new_v4().to_string();
    let routes = std::collections::BTreeMap::from([(
        IMPLEMENTATION.into(),
        WorkRoute {
            harness: "codex".into(),
            model: Some("gpt-task".into()),
        },
    )]);
    let batch = crate::core::workflow::TaskBatch {
        brief: crate::core::workflow::InterviewBrief {
            feature_name: "Task routing metadata".into(),
            ..Default::default()
        },
        specification: "# Task routing metadata".into(),
        feature_id: None,
        contract: None,
        branch_targets: None,
        task_routing: crate::core::workflow::TaskRoutingSnapshot {
            overrides: routes.clone(),
            source_work_uid: Some(source_uid.clone()),
            inherited_from: Some(inherited_from.clone()),
        },
        stories: vec![crate::core::workflow::TaskStory {
            title: "Implement task routing".into(),
            ..Default::default()
        }],
    };
    crate::artifacts::task_docs::save_progress(&root, "routing-run", &batch, 1).unwrap();
    let docs =
        crate::artifacts::task_docs::load_board(&root, &crate::core::workflow::Workflow::default());
    let metadata = docs[0].metadata.as_ref().unwrap();
    assert_eq!(metadata.routing_overrides, routes);
    assert_eq!(
        metadata.routing_source_uid.as_deref(),
        Some(source_uid.as_str())
    );
    assert_eq!(
        metadata.routing_inherited_from.as_deref(),
        Some(inherited_from.as_str())
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn foreign_legacy_front_matter_loads_without_becoming_koolade_metadata() {
    let markdown = "---\ntitle: Legacy task\nowner: team\n---\n\n# Legacy task\n";
    assert!(parse(markdown).unwrap().is_none());
    assert!(visible_content(markdown).starts_with("# Legacy task"));
}
