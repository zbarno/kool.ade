use super::*;

#[test]
// Production boundary: restart recovery inspects the append-only QA store
// under the same per-run lock before claiming the lifecycle task. A real
// report-write marker plus partial report records prove recovery will not
// repeat driver actions; unit checks cannot cover this persisted boundary.
// No Docker, network, or credentials are needed; setup cost is low.
fn partial_report_artifact_prevents_qa_driver_redispatch_after_restart() {
    let root = tempfile::tempdir().unwrap();
    let project_id = uuid::Uuid::new_v4();
    let planning =
        crate::artifacts::planning_store::PlanningStore::legacy_embedded(project_id, root.path());
    let lifecycle = crate::core::lifecycle::LifecycleStore::new(&planning);
    let (task_uid, request, revision) = super::dispatch_fixture::queued_task(&lifecycle);
    let run_dir = root.path().join(&request.qa_run_id);
    std::fs::create_dir_all(&run_dir).unwrap();
    std::fs::write(
        run_dir.join("request.json"),
        serde_json::to_vec_pretty(&request).unwrap(),
    )
    .unwrap();
    std::fs::write(
        run_dir.join("report-write.json"),
        serde_json::to_vec_pretty(&request).unwrap(),
    )
    .unwrap();
    std::fs::write(run_dir.join("scenarios.json"), b"[").unwrap();
    let lock_path = run_dir.join("worker.lock");

    let result = acquire_lock_and_claim(&lock_path, &lifecycle, &task_uid, revision, &request);

    assert!(
        matches!(result, Err(StartError::Failed(reason)) if reason.contains("persisted Runtime QA report"))
    );
    assert!(acquire_worker_lock(&lock_path).is_ok());
    let task = lifecycle.load_task(&task_uid).unwrap();
    let execution = task.qa_execution.unwrap();
    assert_eq!(
        execution.status,
        crate::core::lifecycle::QaExecutionStatusV1::Pending
    );
    assert_eq!(execution.request, Some(request));
}

#[test]
// Production boundary: ordinary request, scenario approval, applicability,
// and service-lease records must not look like a completed report during the
// initial claim. File-backed lifecycle and record paths; no Docker/network,
// with low setup cost. A unit predicate alone would miss the launch effect.
fn approved_scenario_metadata_does_not_prevent_initial_worker_claim() {
    let root = tempfile::tempdir().unwrap();
    let project_id = uuid::Uuid::new_v4();
    let planning =
        crate::artifacts::planning_store::PlanningStore::legacy_embedded(project_id, root.path());
    let lifecycle = crate::core::lifecycle::LifecycleStore::new(&planning);
    let (task_uid, request, revision) = super::dispatch_fixture::queued_task(&lifecycle);
    let run_dir = root.path().join(&request.qa_run_id);
    std::fs::create_dir_all(&run_dir).unwrap();
    let qa_store = crate::runtime_qa::RuntimeQaStore::new(root.path());
    let uid = || uuid::Uuid::new_v4().hyphenated().to_string();
    qa_store
        .save_applicability(
            &request,
            &crate::runtime_qa::QaApplicabilityAssessmentV1 {
                schema_version: 1,
                task_uid: request.parent_task_uid.clone(),
                target_id: "web-app".into(),
                app_surface: crate::project_runtime::AppSurface::Web,
                code_sha: request.code_head_sha.clone(),
                approved_spec_hash: request.approved_spec_hash.clone(),
                applicability: crate::runtime_qa::QaApplicability::Required,
                reason: "The approved web target requires browser runtime QA.".into(),
                evidence_refs: vec!["target:web-app".into()],
                reviewer_execution_id: uid(),
                reviewer_session_id: uid(),
                approved_by: uid(),
                manager_session_id: uid(),
                recorded_at_unix_ms: 1,
            },
        )
        .unwrap();
    for filename in [
        "scenario-proposal-v2.json",
        "approved-scenarios-v2.json",
        "service-lease.json",
        "relay-binding-00000000-0000-0000-0000-000000000000.json",
    ] {
        std::fs::write(run_dir.join(filename), b"{}").unwrap();
    }
    let lock_path = run_dir.join("worker.lock");

    let (worker_lock, claimed) =
        acquire_lock_and_claim(&lock_path, &lifecycle, &task_uid, revision, &request).unwrap();

    assert_eq!(
        claimed.qa_execution.as_ref().unwrap().status,
        crate::core::lifecycle::QaExecutionStatusV1::Running
    );
    drop(worker_lock);
}

#[test]
// Production boundary: a pre-upgrade Pending task may have been run by an
// older worker that left only request.json before report records. Restart
// recovery rejects that ambiguous report state. The lifecycle and evidence
// stores are temporary local files; no Docker/network is needed, low setup.
fn legacy_request_only_artifact_prevents_pending_worker_redispatch() {
    let root = tempfile::tempdir().unwrap();
    let project_id = uuid::Uuid::new_v4();
    let planning =
        crate::artifacts::planning_store::PlanningStore::legacy_embedded(project_id, root.path());
    let lifecycle = crate::core::lifecycle::LifecycleStore::new(&planning);
    let (task_uid, request, revision) = super::dispatch_fixture::queued_task(&lifecycle);
    let run_dir = root.path().join("runtime-qa").join(&request.qa_run_id);
    std::fs::create_dir_all(&run_dir).unwrap();
    std::fs::write(
        run_dir.join("request.json"),
        serde_json::to_vec_pretty(&request).unwrap(),
    )
    .unwrap();
    let lock_path = run_dir.join("worker.lock");

    let result = acquire_lock_and_claim(&lock_path, &lifecycle, &task_uid, revision, &request);

    assert!(matches!(result, Err(StartError::Failed(reason)) if reason.contains("legacy")));
    assert!(acquire_worker_lock(&lock_path).is_ok());
    let task = lifecycle.load_task(&task_uid).unwrap();
    assert_eq!(task.revision, revision);
    let execution = task.qa_execution.unwrap();
    assert_eq!(
        execution.status,
        crate::core::lifecycle::QaExecutionStatusV1::Pending
    );
    assert_eq!(execution.request, Some(request));
}

#[test]
// Production boundary: pending recovery must validate applicability against
// the exact queued request before it treats request.json as normal metadata.
// A file-backed lifecycle/store regression covers the redispatch decision;
// no Docker/network is needed, with low setup cost.
fn mismatched_applicability_does_not_hide_legacy_request_artifact() {
    let root = tempfile::tempdir().unwrap();
    let project_id = uuid::Uuid::new_v4();
    let planning =
        crate::artifacts::planning_store::PlanningStore::legacy_embedded(project_id, root.path());
    let lifecycle = crate::core::lifecycle::LifecycleStore::new(&planning);
    let (task_uid, request, revision) = super::dispatch_fixture::queued_task(&lifecycle);
    let mut persisted = request.clone();
    persisted.code_head_sha = "c".repeat(request.code_head_sha.len());
    let qa_store = crate::runtime_qa::RuntimeQaStore::new(root.path().join("runtime-qa"));
    let uid = || uuid::Uuid::new_v4().hyphenated().to_string();
    qa_store
        .save_applicability(
            &persisted,
            &crate::runtime_qa::QaApplicabilityAssessmentV1 {
                schema_version: 1,
                task_uid: persisted.parent_task_uid.clone(),
                target_id: "web-app".into(),
                app_surface: crate::project_runtime::AppSurface::Web,
                code_sha: persisted.code_head_sha.clone(),
                approved_spec_hash: persisted.approved_spec_hash.clone(),
                applicability: crate::runtime_qa::QaApplicability::Required,
                reason: "The approved web target requires browser runtime QA.".into(),
                evidence_refs: vec!["target:web-app".into()],
                reviewer_execution_id: uid(),
                reviewer_session_id: uid(),
                approved_by: uid(),
                manager_session_id: uid(),
                recorded_at_unix_ms: 1,
            },
        )
        .unwrap();
    let lock_path = qa_store.root().join(&request.qa_run_id).join("worker.lock");

    let result = acquire_lock_and_claim(&lock_path, &lifecycle, &task_uid, revision, &request);

    assert!(matches!(result, Err(StartError::Failed(reason)) if reason.contains("legacy")));
    assert!(acquire_worker_lock(&lock_path).is_ok());
    let task = lifecycle.load_task(&task_uid).unwrap();
    assert_eq!(task.revision, revision);
    assert_eq!(
        task.qa_execution.unwrap().status,
        crate::core::lifecycle::QaExecutionStatusV1::Pending
    );
}

#[test]
// Production boundary: after a stale Running task loses its process lock,
// restart recovery examines legacy on-disk report files before claiming the
// worker. The pending-metadata case is covered separately. This file-backed
// test needs no Docker, network, or credentials and has low setup cost.
fn legacy_request_file_prevents_stale_running_worker_redispatch() {
    let root = tempfile::tempdir().unwrap();
    let project_id = uuid::Uuid::new_v4();
    let planning =
        crate::artifacts::planning_store::PlanningStore::legacy_embedded(project_id, root.path());
    let lifecycle = crate::core::lifecycle::LifecycleStore::new(&planning);
    let (task_uid, request, revision) = super::dispatch_fixture::queued_task(&lifecycle);
    let lock_path = root
        .path()
        .join("runtime-qa")
        .join(&request.qa_run_id)
        .join("worker.lock");

    let (worker_lock, running) =
        acquire_lock_and_claim(&lock_path, &lifecycle, &task_uid, revision, &request).unwrap();
    drop(worker_lock);
    let run_dir = lock_path.parent().unwrap();
    std::fs::write(
        run_dir.join("request.json"),
        serde_json::to_vec_pretty(&request).unwrap(),
    )
    .unwrap();

    let result = acquire_lock_and_claim(
        &lock_path,
        &lifecycle,
        &task_uid,
        running.revision,
        &request,
    );

    assert!(matches!(result, Err(StartError::Failed(reason)) if reason.contains("legacy")));
    assert!(acquire_worker_lock(&lock_path).is_ok());
    let task = lifecycle.load_task(&task_uid).unwrap();
    assert_eq!(task.revision, running.revision);
    let execution = task.qa_execution.unwrap();
    assert_eq!(
        execution.status,
        crate::core::lifecycle::QaExecutionStatusV1::Running
    );
    assert_eq!(execution.request, Some(request));
}
