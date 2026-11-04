use super::*;
use crate::domain::{ItemKind, Priority};

fn state_at(
    tag: &str,
) -> (
    PlannerState,
    std::path::PathBuf,
    std::path::PathBuf,
    crate::test_support::KooladeHomeGuard,
) {
    let root = std::env::temp_dir().join(format!("koolade_apply_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    for args in [
        ["init", "-b", "main"].as_slice(),
        ["config", "user.email", "koolade@test.local"].as_slice(),
        ["config", "user.name", "Kool.ad/e Test"].as_slice(),
    ] {
        let _ = std::process::Command::new("git")
            .args(args)
            .current_dir(&root)
            .output();
    }
    crate::core::state::test_support::seed_published_product_document(&root).unwrap();
    let project_id = crate::core::state::test_support::project_id_for_code_root(&root);
    let home = root.with_extension("home");
    std::fs::create_dir_all(&home).unwrap();
    let home_guard = crate::test_support::KooladeHomeGuard::set(&home);
    let local_root = crate::persistence::project_dir(&project_id.hyphenated().to_string())
        .join("operational-state");
    let store = crate::artifacts::planning_store::PlanningStore::for_code_repository(
        project_id,
        &local_root,
        &root,
    );
    let mut st = PlannerState::load_with_store(&root, &store).unwrap();
    st.bootstrap_missing().unwrap();
    st = PlannerState::load_with_store(&root, &store).unwrap();
    (st, root, local_root, home_guard)
}

fn make_norm(change: Option<&str>, spec: Option<&str>) -> NormalizedTurn {
    NormalizedTurn {
        assistant_message: "done".into(),
        change_summary: change.map(str::to_string),
        document_updates: spec
            .map(|_| {
                vec![(
                    "product:overview".into(),
                    "# Test Product Overview — Living Technical Specification\n\n**Version:** 1.0. **Status:** Accepted fixture.\n\n## Overview\n\nTest product context.\n\n## Users and Outcomes\n\nSynthetic users can complete the tested workflow.\n\n## Current Capabilities\n\nOnly test capabilities are described.\n\n## Architecture and Constraints\n\nCache: Redis.\n\n## Decisions\n\n| ID | Decision | Basis | Status |\n| --- | --- | --- | --- |\n\n## Quality and Acceptance\n\nAssertions verify the requested test boundary.\n".into(),
                )]
            })
            .unwrap_or_default(),
        planning_tasks: Vec::new(),
        task_intent_context: None,
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
        feature_id: Some("CHG-004".into()),
        contract: None,
        repository_git_intents: std::collections::BTreeMap::from([(
            "root".into(),
            crate::test_support::approved_git_intent("root", "main", &"b".repeat(40), "main"),
        )]),
        task_routing: Default::default(),
        stories: vec![crate::core::workflow::TaskStory {
            source_sha: Some("b".repeat(40)),
            title: "Write the generated task".into(),
            purpose: "Show a completed task batch.".into(),
            target_repository: "root".into(),
            ..Default::default()
        }],
    }
}

fn approve_batch_feature(state: &mut PlannerState, batch: &mut crate::core::workflow::TaskBatch) {
    let id = batch.feature_id.as_deref().unwrap();
    let feature_dir = state.repo_root.join("docs/koolade/features");
    std::fs::create_dir_all(&feature_dir).unwrap();
    let markdown = format!(
        "# {id}: {}\n\n**Status:** Ready\n\n## Intent\n\nPublish one generated task batch safely.\n\n## Current Behavior\n\nTask generation has no lifecycle record.\n\n## Desired Behavior\n\nTask generation records approved stories.\n\n## Scope\n\nOne generated story.\n\n## Affected Product Areas\n\n`product:current-capabilities`\n\n## Requirements\n\n- GEN-R1 (MUST). Record the approved task batch.\n\n## Decisions and Assumptions\n\n- **A1 (fixture):** The project operator approves the exact feature plan.\n\n## Acceptance Criteria\n\n1. The approved story has a lifecycle record.\n",
        batch.brief.feature_name
    );
    let markdown =
        crate::artifacts::product_docs::identity::preserve_feature_identity_with_previous(
            None, id, &markdown,
        )
        .unwrap();
    let identity = crate::domain::ArtifactIdentity::from_markdown(&markdown)
        .unwrap()
        .unwrap();
    let markdown = crate::domain::ChangeMetadata::write_markdown(
        &markdown,
        &identity,
        crate::domain::ChangeStatus::Ready,
    )
    .unwrap();
    let path = feature_dir.join(format!("{}.md", identity.uid));
    std::fs::write(&path, &markdown).unwrap();
    let navigation_path = feature_dir.join("AGENTS.md");
    let mut navigation =
        std::fs::read_to_string(&navigation_path).unwrap_or_else(|_| "# Navigation\n".into());
    if !navigation.ends_with('\n') {
        navigation.push('\n');
    }
    navigation.push_str(&format!(
        "\n- [{id}](./{}.md) - Synthetic task generation feature.\n",
        identity.uid
    ));
    std::fs::write(navigation_path, navigation).unwrap();
    for args in [
        ["add", "docs/koolade/features"].as_slice(),
        ["commit", "-qm", "Seed synthetic task generation feature"].as_slice(),
    ] {
        assert!(
            std::process::Command::new("git")
                .args(args)
                .current_dir(&state.repo_root)
                .status()
                .unwrap()
                .success()
        );
    }
    let refreshed_store = crate::artifacts::planning_store::PlanningStore::for_code_repository(
        state.planning_store.project_id,
        &state.planning_store.root,
        &state.repo_root,
    );
    *state = PlannerState::load_with_store(&state.repo_root, &refreshed_store).unwrap();
    let published = crate::artifacts::product_docs::current_document(
        &state.planning_store,
        &format!("feature:{id}"),
    )
    .unwrap()
    .expect("fixture feature is visible from its published Git pin");
    assert_eq!(
        crate::domain::ArtifactIdentity::from_markdown(&published)
            .unwrap()
            .unwrap()
            .uid,
        identity.uid
    );
    state.workflow.brief = Some(batch.brief.clone());
    crate::artifacts::task_docs::save_workflow(&state.planning_store, &state.workflow).unwrap();
    state.workflow = crate::artifacts::task_docs::load_workflow(&state.planning_store).unwrap();
    state.baseline_planning_revision = state.planning_store.revision().unwrap();
    state.active_feature = Some((id.to_owned(), markdown.clone()));
    state.active_features = vec![(id.to_owned(), markdown.clone())];
    let markdown = crate::core::lifecycle::test_support::approve_and_review_feature(
        &state.planning_store,
        &mut state.workflow,
        id,
        &markdown,
    )
    .unwrap();
    state.active_feature = Some((id.to_owned(), markdown.clone()));
    state.active_features = vec![(id.to_owned(), markdown.clone())];
    let identity = crate::domain::ArtifactIdentity::from_markdown(&markdown)
        .unwrap()
        .unwrap();
    let lifecycle = crate::core::lifecycle::LifecycleStore::new(&state.planning_store);
    let approved = lifecycle.load_feature(&identity.uid).unwrap();
    let snapshot = crate::core::lifecycle::plan_snapshot::load_persisted(
        &state.planning_store,
        approved.plan_snapshot_uid.as_deref().unwrap(),
    )
    .unwrap()
    .unwrap();
    batch.stories[0].approved_requirement_ids = vec![snapshot.requirements[0].stable_id.clone()];
    state.baseline_planning_revision = state.planning_store.revision().unwrap();
    batch.specification = markdown;
}

fn task_batch_turn(batch: crate::core::workflow::TaskBatch) -> NormalizedTurn {
    let mut turn = make_norm(None, None);
    turn.task_batch = Some(batch);
    turn
}

#[test]
fn external_store_change_between_generation_and_apply_is_rejected_as_stale() {
    let (mut state, root, local_root, _home_guard) = state_at("task_generation_stale");
    let store = state.planning_store.clone();
    let mut batch = task_batch();
    approve_batch_feature(&mut state, &mut batch);
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
    let _ = std::fs::remove_dir_all(local_root);
}

#[test]
fn writes_changed_files_and_labels_checkpoint() {
    let (mut st, root, local_root, _home_guard) = state_at("both");
    st.items.push(crate::domain::OpenItem::new(
        "CLR-001".into(),
        Priority::Blocking,
        ItemKind::Question,
        "General".into(),
        "deploy frequency?".into(),
        "ops cadence".into(),
    ));
    st.baseline_items_md = items_io::serialize(&[]);
    let nt = make_norm(
        Some("Add caching policy"),
        Some("# Spec v2\nCache: Redis\n"),
    );
    let rc = apply(&mut st, &nt).unwrap();
    assert!(rc.draft_written && rc.items_written);
    assert!(
        rc.changed_store_paths
            .iter()
            .any(|path| path.starts_with("state/spec_drafts/"))
    );
    assert_eq!(rc.changed_store_paths.len(), 4);
    assert_eq!(rc.summary, "add caching policy");
    let draft = load_current(&st.planning_store).unwrap().unwrap();
    assert!(draft.changes[0].content.contains("Redis"));
    assert!(
        !std::fs::read_to_string(root.join("docs/koolade/product/overview.md"))
            .unwrap()
            .contains("Redis")
    );
    let (items, resolved, _) = items_io::load_store(&st.planning_store).unwrap();
    assert!(resolved.is_empty());
    assert_eq!(items[0].question, "deploy frequency?");
    assert!(
        !root.join(crate::artifacts::OPEN_ITEMS_FILE).exists(),
        "new workspaces keep no authoritative monolithic open-items file"
    );
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(local_root);
}

#[test]
fn refresh_discovery_tasks_are_saved_with_the_local_transaction() {
    let (mut state, root, local_root, _home_guard) = state_at("refresh_discoveries");
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
            .changed_store_paths
            .iter()
            .any(|path| path.starts_with("state/work/"))
    );
    let saved = crate::core::planning_work::load(&state.planning_store).unwrap();
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
    let _ = std::fs::remove_dir_all(local_root);
}

#[path = "tests/feature_workflow.rs"]
mod feature_workflow;

#[path = "tests/task_generation.rs"]
mod task_generation;

mod miscellaneous;
