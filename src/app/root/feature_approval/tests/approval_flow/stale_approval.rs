use super::*;

#[path = "stale_approval/readiness_generation.rs"]
mod readiness_generation;

struct MutatingHarness {
    response: String,
    planning_store: crate::artifacts::planning_store::PlanningStore,
    calls: Arc<Mutex<usize>>,
}

fn mutate_private_specification(
    store: &crate::artifacts::planning_store::PlanningStore,
) -> anyhow::Result<()> {
    let current = crate::artifacts::product_docs::current_document(store, "feature:CHG-004")?
        .ok_or_else(|| anyhow::anyhow!("fixture feature is missing"))?;
    let changed = current.replace(
        "Persist saved searches and restore them after restarting.",
        "Preserve the search selection after restarting.",
    );
    let changes = crate::core::apply::private_document_draft_changes(
        store,
        &[("feature:CHG-004".into(), changed)],
    )?;
    store.transaction(&changes, None)?;
    Ok(())
}

impl crate::harness::AiHarness for MutatingHarness {
    fn label(&self) -> String {
        "mutating approval fixture".into()
    }

    fn check_available(&self) -> Result<String, crate::error::AppError> {
        Ok(self.label())
    }

    fn execute(
        &self,
        _request: &crate::harness::PlanningRequest,
    ) -> Result<crate::harness::HarnessOutcome, crate::error::AppError> {
        *self.calls.lock().unwrap() += 1;
        mutate_private_specification(&self.planning_store)
            .map_err(|error| crate::error::AppError::Other(error.to_string()))?;
        Ok(crate::harness::HarnessOutcome {
            final_text: self.response.clone(),
            envelope: None,
            stderr_tail: String::new(),
        })
    }
}

fn approve(app: &mut KooladeApp) {
    ensure_test_plan_adopted(app, "CHG-004");
    record_test_plan_author(app, "CHG-004");
    app.approve_feature_only("CHG-004");
    record_test_plan_review_and_merge(
        app,
        "CHG-004",
        crate::core::lifecycle::GateDecision::Approved,
    );
    app.approve_feature_from_board("CHG-004");
    assert!(app.feature_approved("CHG-004"));
}

fn task_files(store_root: &std::path::Path) -> Vec<(String, Vec<u8>)> {
    let base = store_root.join("planning/tasks");
    let Ok(batches) = std::fs::read_dir(base) else {
        return Vec::new();
    };
    let mut files = Vec::new();
    for batch in batches.flatten() {
        let Ok(entries) = std::fs::read_dir(batch.path()) else {
            continue;
        };
        for entry in entries.flatten() {
            if !entry.file_type().is_ok_and(|kind| kind.is_file()) {
                continue;
            }
            if let Ok(contents) = std::fs::read(entry.path()) {
                files.push((entry.path().to_string_lossy().into_owned(), contents));
            }
        }
    }
    files.sort_by(|left, right| left.0.cmp(&right.0));
    files
}

fn graph_response(app: &KooladeApp) -> String {
    use crate::harness::prompt_catalog::{SubtaskDraftV1, TaskDraftV1, TaskGraphV1};
    let Screen::Connected(project) = &app.screen else {
        panic!("project stays connected")
    };
    let (feature_id, markdown) = project.state.active_feature.as_ref().unwrap();
    let identity = crate::domain::ArtifactIdentity::from_markdown(markdown)
        .unwrap()
        .unwrap();
    let snapshot = crate::core::lifecycle::plan_snapshot::from_feature_with_author(
        &project.state.planning_store,
        feature_id,
        &identity,
        markdown,
        &crate::core::workflow::feature_contract(markdown),
    )
    .unwrap();
    let goal = crate::core::lifecycle::LifecycleStore::new(&project.state.planning_store)
        .load_feature(&identity.uid)
        .unwrap();
    let branch = if !project.git.default_branch.is_empty() {
        project.git.default_branch.clone()
    } else if !project.git.branch.is_empty() {
        project.git.branch.clone()
    } else {
        "main".into()
    };
    let repository = project.state.repositories.repositories.first().unwrap();
    let checkout = project
        .state
        .repositories
        .target(
            &project.state.planning_store,
            &project.state.repo_root,
            &repository.id,
        )
        .unwrap();
    let sha = std::process::Command::new("git")
        .args([
            "rev-parse",
            "--verify",
            &format!("refs/heads/{branch}^{{commit}}"),
        ])
        .current_dir(checkout)
        .output()
        .unwrap();
    assert!(
        sha.status.success(),
        "branch {branch:?} could not be resolved: {}",
        String::from_utf8_lossy(&sha.stderr)
    );
    let source_sha = String::from_utf8(sha.stdout).unwrap().trim().to_owned();
    let requirement_ids = snapshot
        .requirements
        .iter()
        .map(|requirement| requirement.stable_id.clone())
        .collect::<Vec<_>>();
    let graph = TaskGraphV1 {
        feature_uid: identity.uid,
        approved_spec_ref: snapshot.uid,
        decision_refs: goal.decision_refs,
        approved_plan_hash: snapshot.content_hash,
        approved_requirement_ids: requirement_ids.clone(),
        tasks: vec![TaskDraftV1 {
            uid: uuid::Uuid::new_v4().hyphenated().to_string(),
            title: "Persist approved search behavior".into(),
            goal: "Keep the approved saved searches available after restart.".into(),
            intent: "Restore saved searches reliably for analysts.".into(),
            non_goals: vec!["Do not expand the approved search feature.".into()],
            requirements: requirement_ids,
            acceptance_criteria: vec!["A saved search survives an application restart.".into()],
            tests: vec!["Run the saved search persistence tests.".into()],
            relevant_files: vec!["src/search_store.rs".into()],
            dependencies: Vec::new(),
            repository_id: repository.id.clone(),
            source_branch: branch.clone(),
            source_sha: Some(source_sha),
            destination_branch: branch,
            estimated_concurrency_risks: vec!["Only the saved search store is changed.".into()],
            subtasks: Vec::<SubtaskDraftV1>::new(),
        }],
    };
    serde_json::to_string(&graph).unwrap()
}

fn generation_key(app: &KooladeApp) -> String {
    let Screen::Connected(project) = &app.screen else {
        panic!("project stays connected")
    };
    project
        .planning_work
        .iter()
        .find(|work| {
            work.kind == crate::core::planning_work::WorkKind::TaskGeneration
                && work.feature_id.as_deref() == Some("CHG-004")
        })
        .unwrap()
        .key
        .clone()
}

#[test]
fn stale_approval_at_generation_start_does_not_write_task_artifacts() {
    let _shield = crate::core::gitops::test_support::shield("stale-task-generation-start");
    let (mut app, root, _) = setup();
    approve(&mut app);
    let key = generation_key(&app);
    approve_test_git_intent(&mut app, &key, "CHG-004");
    let store_root = private_store_root(&app);
    let task_baseline = task_files(&store_root);
    let Screen::Connected(project) = &app.screen else {
        panic!("project stays connected")
    };
    mutate_private_specification(&project.state.planning_store).unwrap();
    let (harness, calls) = harness(Vec::new());
    app.task_harness = Some(harness);
    app.start_task_generation(&key, "CHG-004");
    assert!(calls.lock().unwrap().is_empty());
    assert!(task_files(&store_root).is_empty());
    assert_eq!(task_files(&store_root), task_baseline);
    drop(app);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn approval_change_during_decomposition_does_not_publish_task_artifacts() {
    let _shield = crate::core::gitops::test_support::shield("stale-task-generation-publish");
    let (mut app, root, _) = setup();
    approve(&mut app);
    let store_root = private_store_root(&app);
    let response = graph_response(&app);
    let Screen::Connected(project) = &app.screen else {
        panic!("project stays connected")
    };
    let planning_store = project.state.planning_store.clone();
    let calls = Arc::new(Mutex::new(0));
    let harness: Box<dyn crate::harness::AiHarness> = Box::new(MutatingHarness {
        response,
        planning_store,
        calls: calls.clone(),
    });
    app.task_harness = Some(harness);
    let task_baseline = task_files(&store_root);
    let key = generation_key(&app);
    approve_test_git_intent(&mut app, &key, "CHG-004");
    app.start_task_generation(&key, "CHG-004");
    assert!(!finish(&mut app));
    assert_eq!(*calls.lock().unwrap(), 1);
    assert_eq!(task_files(&store_root), task_baseline);
    drop(app);
    std::fs::remove_dir_all(root).unwrap();
}
