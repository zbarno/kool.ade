use super::*;
use crate::artifacts::planning_store::{PlanningStore, StoreMode};
use crate::core::validation::NormalizedTurn;
use crate::domain::{ArtifactIdentity, ChangeMetadata, ChangeStatus};
#[path = "managed_store_equivalence/support.rs"]
mod support;
use support::{CleanupDir, copy_directory, copy_store, git, init_git, snapshot};

#[test]
fn existing_project_writes_match_legacy_after_edit_approval_and_generation() {
    let sandbox = std::env::temp_dir().join(format!(
        "koolade_store_equivalence_{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&sandbox).unwrap();
    let _cleanup = CleanupDir::new(sandbox.clone());
    let code = sandbox.join("code");
    let managed_root = sandbox.join("planning");
    std::fs::create_dir_all(&code).unwrap();
    git(&code, &["init", "-q", "-b", "main"]);
    git(&code, &["config", "user.name", "Koolade Test"]);
    git(&code, &["config", "user.email", "koolade@example.test"]);
    std::fs::write(code.join("README.md"), "existing code checkout\n").unwrap();
    git(&code, &["add", "README.md"]);
    git(&code, &["commit", "-q", "-m", "existing project"]);

    let project_id = uuid::Uuid::new_v4();
    let legacy = PlanningStore::legacy_embedded(project_id, &code);
    let mut initial = PlannerState::load_with_store(&code, &legacy).unwrap();
    initial.bootstrap_missing().unwrap();
    seed_existing_feature(&legacy);
    git(&code, &["add", ".koolade-packet"]);
    git(
        &code,
        &["commit", "-q", "-m", "seed existing planning data"],
    );

    std::fs::create_dir_all(&managed_root).unwrap();
    git(&managed_root, &["init", "-q", "-b", "main"]);
    git(&managed_root, &["config", "user.name", "Koolade Test"]);
    git(
        &managed_root,
        &["config", "user.email", "koolade@example.test"],
    );
    let managed = PlanningStore::new(project_id, &managed_root, StoreMode::ManagedLocal);
    copy_store(&legacy, &managed);
    git(&managed_root, &["add", "."]);
    git(
        &managed_root,
        &["commit", "-q", "-m", "seed managed planning data"],
    );
    assert_stores_equal(&legacy, &managed, "initial state");

    let mut legacy_state = PlannerState::load_with_store(&code, &legacy).unwrap();
    let mut managed_state = PlannerState::load_with_store(&code, &managed).unwrap();
    let turn = NormalizedTurn {
        assistant_message: "Updated the feature and created its decision record.".into(),
        change_summary: Some("Update managed import behavior".into()),
        spec_markdown: None,
        document_updates: vec![
            (
                "product:overview".into(),
                "# Overview\n\nPlanning writes preserve existing project data.\n".into(),
            ),
            ("feature:F1".into(), updated_feature()),
        ],
        planning_tasks: Vec::new(),
        change_status_updates: [("F1".into(), ChangeStatus::Ready)].into(),
        additional_planning_artifacts: vec![(
            "planning/decisions/ADR-001.md".into(),
            "# ADR-001: Keep planning data shared across code checkouts\n\nThe selected planning store remains authoritative across source branch changes.\n".into(),
        )],
        added: Vec::new(),
        updates: Vec::new(),
        resolved: Vec::new(),
        next_question_id: None,
        requested_action: None,
        follow_up_task: None,
        warnings: Vec::new(),
        workflow: None,
        task_batch: None,
        plan_comparison: None,
    };
    crate::core::apply::apply(&mut legacy_state, &turn).unwrap();
    crate::core::apply::apply(&mut managed_state, &turn).unwrap();
    assert_stores_equal(&legacy, &managed, "edit");

    crate::core::workflow::approve_feature(&legacy, &mut legacy_state.workflow, "F1").unwrap();
    crate::core::workflow::approve_feature(&managed, &mut managed_state.workflow, "F1").unwrap();
    assert_stores_equal(&legacy, &managed, "approval");

    let specification = String::from_utf8(
        managed
            .read("planning/changes/F1-shared-import-context/specification.md")
            .unwrap(),
    )
    .unwrap();
    let batch = task_batch(specification);

    let checkpoint_root = sandbox.join("checkpoint");
    init_git(&checkpoint_root);
    let checkpoint = PlanningStore::new(project_id, &checkpoint_root, StoreMode::ManagedLocal);
    copy_store(&legacy, &checkpoint);
    crate::artifacts::task_docs::save_progress(&checkpoint, "equivalence-run", &batch, 1).unwrap();
    copy_directory(
        &checkpoint,
        &[&legacy, &managed],
        "planning/tasks/F1-shared-import-context",
    );
    assert_stores_equal(&legacy, &managed, "task generation checkpoint");
    crate::artifacts::task_docs::save_progress(&legacy, "equivalence-run", &batch, 1).unwrap();
    crate::artifacts::task_docs::save_progress(&managed, "equivalence-run", &batch, 1).unwrap();
    assert_stores_equal(&legacy, &managed, "task progress");

    let mut legacy_workflow = crate::artifacts::task_docs::load_workflow(&legacy).unwrap();
    let mut managed_workflow = crate::artifacts::task_docs::load_workflow(&managed).unwrap();
    crate::artifacts::task_docs::write_batch(&legacy, &batch, &mut legacy_workflow).unwrap();
    crate::artifacts::task_docs::write_batch(&managed, &batch, &mut managed_workflow).unwrap();
    assert_stores_equal(&legacy, &managed, "task generation");
}

fn assert_stores_equal(legacy: &PlanningStore, managed: &PlanningStore, stage: &str) {
    let left = snapshot(&legacy.root);
    let right = snapshot(&managed.root);
    let paths = left
        .keys()
        .chain(right.keys())
        .collect::<std::collections::BTreeSet<_>>();
    let differences = paths
        .into_iter()
        .filter(|path| {
            !equivalent_record_bytes(
                path,
                left.get(*path).map(Vec::as_slice),
                right.get(*path).map(Vec::as_slice),
            )
        })
        .collect::<Vec<_>>();
    let details = differences
        .iter()
        .map(|path| {
            (
                path,
                left.get(*path)
                    .map(|bytes| String::from_utf8_lossy(bytes).into_owned()),
                right
                    .get(*path)
                    .map(|bytes| String::from_utf8_lossy(bytes).into_owned()),
            )
        })
        .collect::<Vec<_>>();
    assert!(differences.is_empty(), "{stage} differs: {details:#?}");
}

fn equivalent_record_bytes(path: &str, left: Option<&[u8]>, right: Option<&[u8]>) -> bool {
    let (Some(left), Some(right)) = (left, right) else {
        return left == right;
    };
    let record_path = path.starts_with("state/work/")
        || path.starts_with("state/workflow/")
        || path.starts_with("state/items/")
        || path.starts_with("state/tasks/");
    if !record_path {
        return left == right;
    }
    fn without_timestamps(bytes: &[u8]) -> Option<serde_json::Value> {
        let mut value: serde_json::Value = serde_json::from_slice(bytes).ok()?;
        fn strip(value: &mut serde_json::Value) {
            match value {
                serde_json::Value::Object(object) => {
                    object.remove("createdAtMs");
                    object.remove("updatedAtMs");
                    for nested in object.values_mut() {
                        strip(nested);
                    }
                }
                serde_json::Value::Array(values) => {
                    for nested in values {
                        strip(nested);
                    }
                }
                _ => {}
            }
        }
        strip(&mut value);
        Some(value)
    }
    without_timestamps(left)
        .zip(without_timestamps(right))
        .is_some_and(|(left, right)| left == right)
}

fn seed_existing_feature(store: &PlanningStore) {
    let identity = ArtifactIdentity {
        uid: "11111111-1111-4111-8111-111111111111".into(),
        display_id: "F1".into(),
        title: "Shared import context".into(),
        parent_uid: None,
    };
    let base = "# F1: Shared import context\n\n## Intent\n\nExpose stored reference data to planning turns.\n\n## Current Behavior\n\nImports are listed by path.\n\n## Desired Behavior\n\nThe planner can use imported text.\n\n## Scope\n\nPlanning context only.\n\n## Affected Product Areas\n\n`overview.md`\n\n## Requirements\n\n- FR-1 (MUST). Read shared imports.\n\n## Decisions and Assumptions\n\n- **D-1:** Imported documents are reference data.\n\n## Acceptance Criteria\n\n1. The planner can use an imported document.\n";
    let marker = format!(
        "<!-- koolade-artifact-id:v1 {} -->",
        serde_json::to_string(&identity).unwrap()
    );
    let identified = base.replacen("\n\n", &format!("\n\n{marker}\n\n"), 1);
    let feature =
        ChangeMetadata::write_markdown(&identified, &identity, ChangeStatus::Ready).unwrap();
    store
        .atomic_write(
            "planning/changes/F1-shared-import-context/specification.md",
            feature.as_bytes(),
        )
        .unwrap();
}

fn updated_feature() -> String {
    "# F1: Shared import context\n\n## Intent\n\nExpose stored reference data to planning turns.\n\n## Current Behavior\n\nImports are listed by path.\n\n## Desired Behavior\n\nThe planner receives bounded text from the selected store.\n\n## Scope\n\nPlanning context only.\n\n## Affected Product Areas\n\n`overview.md`\n\n## Requirements\n\n- FR-1 (MUST). Read shared imports.\n\n## Decisions and Assumptions\n\n- **D-1:** Imported documents are reference data.\n\n## Acceptance Criteria\n\n1. The planner can use an imported document.\n".into()
}

fn task_batch(specification: String) -> crate::core::workflow::TaskBatch {
    crate::core::workflow::TaskBatch {
        brief: crate::core::workflow::InterviewBrief {
            feature_name: "Shared import context (F1)".into(),
            goal: "Make imported evidence available to planning turns.".into(),
            target_users: "Project operators".into(),
            intended_outcome: "Managed imports inform planning.".into(),
            success_criteria: vec!["The imported content reaches the prompt.".into()],
            in_scope: vec!["Load text imports from the store.".into()],
            ready_for_tasks: true,
            ..Default::default()
        },
        specification,
        feature_id: Some("F1".into()),
        contract: None,
        branch_targets: None,
        task_routing: Default::default(),
        stories: vec![crate::core::workflow::TaskStory {
            title: "Load managed imports".into(),
            purpose: "Give the planner imported reference evidence.".into(),
            scope_items: vec![1],
            success_criteria: vec![1],
            ..Default::default()
        }],
    }
}
