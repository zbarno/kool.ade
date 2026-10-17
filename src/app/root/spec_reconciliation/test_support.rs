use super::*;
use crate::{
    app::root::{KooladeApp, Screen},
    core::lifecycle::{LifecycleStore, TaskPhase},
};

pub(crate) fn fixture(count: usize) -> (KooladeApp, String, Vec<String>, String) {
    let mut app = crate::app::root::board_tests::fixture();
    let (feature_uid, delta, task_uid, _, _, _, _, _, _, _, _, _) =
        app.seed_trace_navigation_fixture();
    let Screen::Connected(project) = &mut app.screen else {
        panic!("connected")
    };
    let store = project.state.planning_store.clone();
    let lifecycle = LifecycleStore::new(&store);
    let original = lifecycle.load_task(&task_uid).unwrap();
    let mut ids = Vec::new();
    for index in 0..count {
        let mut task = original.clone();
        if index > 0 {
            task.task_uid = uuid::Uuid::new_v4().to_string();
            task.planning_item_uid = Some(uuid::Uuid::new_v4().to_string());
            task.pr_work_item_uid = None;
            task.revision = 0;
            task.code_review.as_mut().unwrap().subject_uid = task.task_uid.clone();
            task.runtime_qa.as_mut().unwrap().subject_uid = task.task_uid.clone();
            task.qa_execution
                .as_mut()
                .unwrap()
                .request
                .as_mut()
                .unwrap()
                .parent_task_uid = task.task_uid.clone();
            lifecycle
                .create_task(&task, &uuid::Uuid::new_v4().to_string())
                .unwrap();
        }
        let mut task = lifecycle.load_task(&task.task_uid).unwrap();
        task.phase = TaskPhase::AwaitingSpecReconciliation;
        let path = format!("state/lifecycle_tasks/{}.json", task.task_uid);
        let (mut wrapper, revision) = store.read_record::<serde_json::Value>(&path).unwrap();
        task.revision = revision + 1;
        wrapper["revision"] = task.revision.into();
        wrapper["record"] = serde_json::to_value(&task).unwrap();
        store.save_record(&path, &wrapper, revision).unwrap();
        ids.push(task.task_uid);
    }
    (app, feature_uid, ids, delta.requirement_id)
}

pub(crate) fn complete(
    project: &mut Project,
    task_uid: &str,
    proposal: crate::harness::prompt_catalog::SpecDeltaV1,
    pr_refresh: bool,
) {
    use crate::harness::prompt_catalog::{PromptRole, RoleInputV1, RolePayloadV1};
    let store = &project.state.planning_store;
    let lifecycle = LifecycleStore::new(store);
    let task = lifecycle.load_task(task_uid).unwrap();
    let feature = lifecycle.load_feature(&task.feature_uid).unwrap();
    let spec_sha256 = crate::core::lifecycle::product_spec::current_product_hash(store).unwrap();
    let workspace = store.project_id.to_string();
    let spec_ref = format!("current-spec:{spec_sha256}");
    let prompt = crate::harness::prompt_catalog::render(&RoleInputV1 {
        role: PromptRole::SpecReconciler,
        workspace_uid: workspace.clone(),
        feature_uid: task.feature_uid.clone(),
        task_uid: Some(task_uid.to_owned()),
        original_goal: feature.original_intent,
        approved_spec_ref: feature.plan_snapshot_uid,
        approved_spec_hash: Some(task.approved_plan_sha256.clone()),
        decision_refs: Vec::new(),
        evidence_manifest: vec![
            evidence_item(
                &spec_ref,
                &workspace,
                "fixture",
                "Synthetic verified fixture",
            )
            .unwrap(),
        ],
        execution_policy_ref: "spec-reconciliation/test-result/v1".into(),
        output_schema_version: PromptRole::SpecReconciler.output_schema_version().into(),
        payload: RolePayloadV1::SpecSync {
            approved_requirements: task.approved_requirement_ids.clone(),
            verified_implementation_refs: vec![spec_ref],
        },
    })
    .unwrap()
    .manifest;
    let outcome = SpecReconciliationOutcome {
        run_uid: uuid::Uuid::new_v4().to_string(),
        task_uid: task_uid.to_owned(),
        expected_revision: task.revision,
        spec_sha256: spec_sha256.clone(),
        proposal,
        prompt,
    };
    let (done, result) = mpsc::channel();
    assert!(done.send(Ok(outcome)).is_ok());
    project.activity.spec_reconciliation = Some(SpecReconciliation {
        task_uid: task_uid.to_owned(),
        expected_revision: task.revision,
        spec_sha256,
        post_completion_pr_refresh: pr_refresh,
        result,
        cancel: Arc::new(AtomicBool::new(false)),
    });
    // Exercise the real polling/result-dispatch path with a deterministic
    // completed worker result; no provider execution is involved.
    crate::app::root::tick::activity::spec_reconciliation::advance_spec_reconciliation(
        project, &mut None,
    );
}
