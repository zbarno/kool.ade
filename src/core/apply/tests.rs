use super::*;
use crate::core::validation;
use crate::domain::{CurrentUser, ItemKind, Priority};
use crate::harness::TurnEnvelope;

fn state_at(tag: &str) -> (PlannerState, std::path::PathBuf) {
    let root = std::env::temp_dir().join(format!("koolade_apply_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    for args in [
        ["init"].as_slice(),
        ["config", "user.email", "koolade@test.local"].as_slice(),
        ["config", "user.name", "Kool.ad/e Test"].as_slice(),
    ] {
        let _ = std::process::Command::new("git")
            .args(args)
            .current_dir(&root)
            .output();
    }
    let mut st = PlannerState::load(&root).unwrap();
    st.bootstrap_missing().unwrap();
    (st, root)
}

fn make_norm(change: Option<&str>, spec: Option<&str>) -> NormalizedTurn {
    NormalizedTurn {
        assistant_message: "done".into(),
        change_summary: change.map(str::to_string),
        spec_markdown: spec.map(str::to_string),
        document_updates: Vec::new(),
        planning_tasks: Vec::new(),
        change_status_updates: Default::default(),
        additional_planning_artifacts: vec![],
        added: vec![],
        updates: vec![],
        resolved: vec![],
        next_question_id: None,
        requested_action: None,
        follow_up_task: None,
        workflow: None,
        task_batch: None,
        warnings: vec![],
        plan_comparison: None,
    }
}

fn task_batch() -> crate::core::workflow::TaskBatch {
    crate::core::workflow::TaskBatch {
        brief: crate::core::workflow::InterviewBrief {
            feature_name: "Revision-fenced generation".into(),
            goal: "Publish one generated task batch safely.".into(),
            target_users: "Project operators".into(),
            intended_outcome: "Task stories appear on the board.".into(),
            success_criteria: vec!["The workflow records the batch.".into()],
            in_scope: vec!["Generate one story.".into()],
            ready_for_tasks: true,
            ..Default::default()
        },
        specification: "# Revision-fenced generation\n".into(),
        feature_id: None,
        contract: None,
        branch_targets: None,
        task_routing: Default::default(),
        stories: vec![crate::core::workflow::TaskStory {
            title: "Write the generated task".into(),
            purpose: "Show a completed task batch.".into(),
            ..Default::default()
        }],
    }
}

fn task_batch_turn(batch: crate::core::workflow::TaskBatch) -> NormalizedTurn {
    let mut turn = make_norm(None, None);
    turn.task_batch = Some(batch);
    turn
}

#[test]
fn generated_progress_revision_is_used_by_the_real_apply_finalizer() {
    let (mut state, root) = state_at("task_generation_revision");
    let store = state.planning_store.clone();
    let batch = task_batch();
    let (_, revision) = crate::artifacts::task_docs::save_progress_expected(
        &store,
        "revision-fenced-run",
        &batch,
        1,
        &state.baseline_planning_revision,
    )
    .unwrap();
    state.baseline_planning_revision = revision;

    let receipt = apply(&mut state, &task_batch_turn(batch)).unwrap();

    assert_eq!(state.workflow.task_batches.len(), 1);
    assert_eq!(
        crate::artifacts::task_docs::load_workflow(&store)
            .unwrap()
            .task_batches,
        state.workflow.task_batches
    );
    assert!(
        receipt
            .repo_relative_paths
            .iter()
            .any(|path| { path.ends_with(".koolade-progress.json") })
    );
    assert_eq!(state.baseline_planning_revision, store.revision().unwrap());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn external_store_change_between_generation_and_apply_is_rejected_as_stale() {
    let (mut state, root) = state_at("task_generation_stale");
    let store = state.planning_store.clone();
    let batch = task_batch();
    let (_, generation_revision) = crate::artifacts::task_docs::save_progress_expected(
        &store,
        "stale-generation-run",
        &batch,
        1,
        &state.baseline_planning_revision,
    )
    .unwrap();
    store
        .transaction_with_revision(
            &[("planning/external.md".into(), b"another writer".to_vec())],
            Some(&generation_revision),
        )
        .unwrap();
    state.baseline_planning_revision = generation_revision;

    assert!(apply(&mut state, &task_batch_turn(batch)).is_err());
    assert!(state.workflow.task_batches.is_empty());
    assert!(
        crate::artifacts::task_docs::load_workflow(&store)
            .unwrap()
            .task_batches
            .is_empty()
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn writes_changed_files_and_labels_checkpoint() {
    let (mut st, root) = state_at("both");
    st.items.push(crate::domain::OpenItem::new(
        "CLR-001".into(),
        Priority::Blocking,
        ItemKind::Question,
        "General".into(),
        Some("All".into()),
        "deploy frequency?".into(),
        "ops cadence".into(),
    ));
    st.baseline_items_md = items_io::serialize(&[]); // queue as of the PREVIOUS commit
    let nt = make_norm(
        Some("Add caching policy"),
        Some("# Spec v2\nCache: Redis\n"),
    );
    let rc = apply(&mut st, &nt).unwrap();
    assert!(rc.spec_written && rc.items_written);
    assert_eq!(rc.repo_relative_paths, vec![SPEC_FILE, OPEN_ITEMS_FILE]);
    assert_eq!(rc.commit_message, "planner: add caching policy");
    let spec_disk =
        crate::artifacts::read_utf8_lossy(&crate::artifacts::repo_artifact(&root, SPEC_FILE))
            .unwrap();
    assert!(spec_disk.contains("Redis"));
    let items_disk =
        crate::artifacts::read_utf8_lossy(&crate::artifacts::repo_artifact(&root, OPEN_ITEMS_FILE))
            .unwrap();
    assert!(
        items_disk.contains("deploy frequency?"),
        "queued item must round-trip to disk"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn refresh_discovery_tasks_are_committed_with_the_planning_transaction() {
    let (mut state, root) = state_at("refresh_discoveries");
    let mut turn = make_norm(Some("refresh repository documentation"), None);
    turn.planning_tasks = vec![
        crate::harness::PlanningTaskDraft {
            title: "Clarify persistence behavior".into(),
            description: "README.md differs from src/storage.rs; ask which behavior is intended."
                .into(),
            kind: crate::core::planning_work::WorkKind::Question,
            status: crate::core::planning_work::WorkStatus::NeedsAttention,
        },
        crate::harness::PlanningTaskDraft {
            title: "Triage possible secret exposure".into(),
            description: "src/auth.rs logs a token-shaped value; confirm whether it is sensitive."
                .into(),
            kind: crate::core::planning_work::WorkKind::Bug,
            status: crate::core::planning_work::WorkStatus::Todo,
        },
    ];
    let receipt = apply(&mut state, &turn).unwrap();
    assert!(
        receipt
            .repo_relative_paths
            .contains(&crate::core::planning_work::FILE.to_owned())
    );
    let saved = crate::core::planning_work::load(&root).unwrap();
    assert_eq!(saved.len(), 2);
    assert_eq!(
        saved[0].status,
        crate::core::planning_work::WorkStatus::NeedsAttention
    );
    assert_eq!(
        saved[1].status,
        crate::core::planning_work::WorkStatus::Todo
    );
    assert_eq!(saved[1].kind, crate::core::planning_work::WorkKind::Bug);
    let _ = std::fs::remove_dir_all(&root);
}

#[path = "tests/feature_workflow.rs"]
mod feature_workflow;

#[test]
fn no_change_turn_touches_nothing() {
    let (mut st, root) = state_at("noop");
    let nt = make_norm(None, None);
    let rc = apply(&mut st, &nt).unwrap();
    assert!(!rc.spec_written && !rc.items_written);
    assert!(rc.repo_relative_paths.is_empty());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn unowned_used_category_spawns_single_ownership_item() {
    let (mut st, root) = state_at("own");
    st.config.user = Some(CurrentUser::new("Zach", vec!["Engineering".into()]));
    st.items.push(crate::domain::OpenItem::new(
        "CLR-001".into(),
        Priority::Normal,
        ItemKind::Question,
        "Operations".into(),
        Some("Ops lead".into()),
        "who watches prod logs?".into(),
        String::new(),
    ));
    st.baseline_items_md = items_io::serialize(&st.items);
    let e = TurnEnvelope {
        schema_version: Some(2),
        assistant_message: Some("logged".into()),
        change_summary: None,
        document_updates: None,
        planning_tasks: None,
        updated_specification: None,
        open_items_added: None,
        open_items_updated: None,
        open_items_resolved: None,
        next_question_id: None,
        requested_action: None,
        follow_up_task: None,
        interview: None,
        task_stories: None,
        task_outline: None,
        plans: None,
        recommendation: None,
    };
    let nt = validation::validate(&e, &st, &st.effective_user()).unwrap();
    let rc = apply(&mut st, &nt).unwrap();
    // Only the USED category (Operations) spawns an ownership item, and
    // exactly one; a re-apply must not multiply them.
    let n_own = st
        .items
        .iter()
        .filter(|i| i.kind == ItemKind::Ownership)
        .count();
    assert_eq!(n_own, 1);
    assert_eq!(rc.synthesized_open_items.len(), 1);
    let nt2 = validation::validate(&e, &st, &st.effective_user()).unwrap();
    let rc2 = apply(&mut st, &nt2).unwrap();
    assert!(rc2.synthesized_open_items.is_empty());
    assert_eq!(
        st.items
            .iter()
            .filter(|i| i.kind == ItemKind::Ownership)
            .count(),
        1
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn commit_phrase_lowercases_agent_summary() {
    let (st, root) = state_at("msg");
    let nt = make_norm(Some("Establish initial specification"), None);
    assert_eq!(
        commit_message::compose(&nt, true, false, 0, 0, 0),
        "planner: establish initial specification"
    );
    let _ = (st, std::fs::remove_dir_all(&root));
}
