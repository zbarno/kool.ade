use super::*;

#[path = "pr_work.rs"]
mod pr_work;

/// Production boundary: persisted QA evidence and reviewer receipt stay bound to
/// the task revision through restart, repair, and one-time spec reconciliation.
/// Unit tests cannot prove cross-store recovery. Temporary in-process stores need
/// no Docker/model API; rootless candidate-image runs cover real UI interactions.

#[test]
fn qa_finding_repairs_new_code_repasses_and_reconciles_once_to_pr_work_item() {
    let temp = tempfile::tempdir().unwrap();
    let store = PlanningStore::new(uuid::Uuid::new_v4(), temp.path(), StoreMode::ManagedLocal);
    let lifecycle = LifecycleStore::new(&store);
    bootstrap_product(&store);
    let feature_uid = id();
    lifecycle
        .create_feature(&new_feature(feature_uid.clone()), &id())
        .unwrap();
    let feature = plan::reviewed_feature_plan(&store, &lifecycle, &feature_uid);
    let feature_events = lifecycle.events_for_feature(&feature_uid).unwrap();
    let origin_events = feature_events.into_iter().map(|event| event.uid).collect();
    lifecycle
        .append_spec_revision(
            &feature_uid,
            0,
            SpecRevisionDraftV1 {
                content_hash: product_hash(&store),
                source_revision: feature.revision,
                produced_by_run_id: None,
                origin_events,
                decision_refs: feature.decision_refs.clone(),
                delta_approval_refs: Vec::new(),
                approved_target_hash: feature.approved_plan_sha256.clone(),
                implementation_head_sha: None,
                reconciliation_status: SpecReconciliationStatusV1::Synchronized,
                outcome_reason: None,
                no_change_reason: Some("Synthetic lifecycle acceptance review found no specification delta for the approved task graph.".into()),
                deltas: Vec::new(),
            },
        )
        .unwrap();
    let plan_hash = feature.approved_plan_sha256.unwrap();
    let task_uid = id();
    let child_uid = id();
    lifecycle
        .create_task(&task(&feature_uid, &task_uid, &plan_hash, None), &id())
        .unwrap();
    lifecycle
        .create_task(
            &task(&feature_uid, &child_uid, &plan_hash, Some(task_uid.clone())),
            &id(),
        )
        .unwrap();
    parent_subtasks::complete_subtask(temp.path(), &store, &lifecycle, &feature_uid, &child_uid);

    let mut task_record = lifecycle.load_task(&task_uid).unwrap();
    task_record = lifecycle
        .apply_task_event(&task_uid, task_record.revision, &id(), TaskEvent::Start)
        .unwrap();
    let implementation_hash = "d".repeat(64);
    task_record = lifecycle
        .apply_task_event(
            &task_uid,
            task_record.revision,
            &id(),
            TaskEvent::ImplementationFinished {
                code_sha256: implementation_hash.clone(),
            },
        )
        .unwrap();
    let mut code_review = role_receipt(
        koolade::harness::prompt_catalog::PromptRole::CodeReviewer,
        GateDecision::Approved,
        &task_uid,
        &implementation_hash,
    );
    code_review.evidence_refs = vec!["fixture:accepted-review-contract/v1".into()];
    task_record = lifecycle
        .apply_task_event(
            &task_uid,
            task_record.revision,
            &id(),
            TaskEvent::CodeReview {
                receipt: code_review,
                reviewed_head_sha: "e".repeat(64),
                review_subject_sha256: "f".repeat(64),
            },
        )
        .unwrap();

    let qa_store = RuntimeQaStore::new(temp.path().join("qa-records"));
    let (failed_request, failed_outcome, failed_receipt) = qa_report::execute_reviewed_run(
        temp.path(),
        &lifecycle,
        &mut task_record,
        QaAssertionStatus::Failed,
    );
    assert_eq!(failed_outcome, QaOutcome::Fail);
    assert_eq!(
        failed_receipt.as_ref().unwrap().decision,
        GateDecision::ChangesRequested
    );
    let (_, _, failed_evidence, failed_result) =
        qa_store.load_run(&failed_request.qa_run_id).unwrap();
    runtime_qa_v2_rejection::assert_direct_legacy_and_current_completion_events_rejected(
        runtime_qa_v2_rejection::CompletionRejectionCase {
            lifecycle: &lifecycle,
            task_uid: &task_uid,
            expected_revision: task_record.revision,
            direct_legacy_event_uid: &id(),
            direct_current_event_uid: &id(),
            request: &failed_request,
            current_receipt: failed_receipt.as_ref().unwrap(),
            evidence: &failed_evidence,
            result: &failed_result,
        },
    );

    let failed_receipt = failed_receipt.as_ref().expect("failed QA has a receipt");
    let receipt_path = temp
        .path()
        .join("qa-records")
        .join(&failed_request.qa_run_id)
        .join("gate-receipt.json");
    std::fs::remove_file(&receipt_path).unwrap();
    let failed_event_uid = id();
    assert!(
        lifecycle
            .record_runtime_qa_report(
                &task_uid,
                task_record.revision,
                &failed_event_uid,
                &qa_store,
            )
            .is_err()
    );
    assert_eq!(
        lifecycle.load_task(&task_uid).unwrap().revision,
        task_record.revision
    );
    qa_store.save_gate_receipt(failed_receipt).unwrap();

    task_record = lifecycle
        .record_runtime_qa_report(
            &task_uid,
            task_record.revision,
            &failed_event_uid,
            &qa_store,
        )
        .unwrap();
    assert_eq!(task_record.phase, TaskPhase::Repairing);
    assert!(task_record.runtime_qa.is_some());

    task_record = lifecycle
        .apply_task_event(
            &task_uid,
            task_record.revision,
            &id(),
            TaskEvent::BeginRepair,
        )
        .unwrap();
    assert!(task_record.runtime_qa.is_none());
    assert!(task_record.code_review.is_none());
    assert!(task_record.qa_execution.is_none());
    let repaired_hash = "0".repeat(64);
    task_record = lifecycle
        .apply_task_event(
            &task_uid,
            task_record.revision,
            &id(),
            TaskEvent::ImplementationFinished {
                code_sha256: repaired_hash.clone(),
            },
        )
        .unwrap();
    let repaired_review = role_receipt(
        koolade::harness::prompt_catalog::PromptRole::CodeReviewer,
        GateDecision::Approved,
        &task_uid,
        &repaired_hash,
    );
    task_record = lifecycle
        .apply_task_event(
            &task_uid,
            task_record.revision,
            &id(),
            TaskEvent::CodeReview {
                receipt: repaired_review,
                reviewed_head_sha: "1".repeat(64),
                review_subject_sha256: "2".repeat(64),
            },
        )
        .unwrap();
    assert_ne!(
        task_record.reviewed_code_head_sha.as_deref(),
        Some(failed_request.code_head_sha.as_str())
    );

    let (qa_request, qa_outcome, _qa_receipt) = qa_report::execute_reviewed_run(
        temp.path(),
        &lifecycle,
        &mut task_record,
        QaAssertionStatus::Passed,
    );
    assert_eq!(qa_outcome, QaOutcome::Pass);
    assert_ne!(qa_request.qa_run_id, failed_request.qa_run_id);
    assert_ne!(
        qa_request.complete_diff_hash,
        failed_request.complete_diff_hash
    );
    assert_ne!(qa_request.code_head_sha, failed_request.code_head_sha);
    assert_eq!(qa_request.complete_diff_hash, repaired_hash);
    task_record = lifecycle
        .record_runtime_qa_report(&task_uid, task_record.revision, &id(), &qa_store)
        .unwrap();
    assert_eq!(task_record.phase, TaskPhase::AwaitingSpecReconciliation);

    let reopened_store = PlanningStore::new(store.project_id, store.root.clone(), store.mode);
    let reopened = LifecycleStore::new(&reopened_store);
    task_record = reopened.load_task(&task_uid).unwrap();
    assert_eq!(
        task_record.qa_execution.as_ref().unwrap().request.as_ref(),
        Some(&qa_request)
    );
    assert!(task_record.runtime_qa.is_some());

    let work_uid = id();
    parent_subtasks::reconcile_no_change(
        &reopened_store,
        &reopened,
        &feature_uid,
        "Synthetic reopened lifecycle review found no specification delta for the persisted task evidence.",
    );
    let spec_receipt = GateReceipt {
        receipt_uid: id(),
        subject_uid: task_uid.clone(),
        subject_sha256: repaired_hash,
        actor: GateActor::ApplicationPolicy,
        decision: GateDecision::Approved,
        recorded_at_unix_ms: 4,
        evidence_refs: vec!["fixture:accepted-spec-reconciliation/v1".into()],
        prompt_sha256: None,
        prompt_manifest: None,
        reviewer_run_id: None,
        author_run_id: None,
        reviewer_session_id: None,
        author_session_id: None,
    };
    let expected_revision = task_record.revision;
    let done = reopened
        .reconcile_specification(
            &task_uid,
            expected_revision,
            &id(),
            spec_receipt.clone(),
            &work_uid,
        )
        .unwrap();
    assert_eq!(done.phase, TaskPhase::Done);
    assert_eq!(done.pr_work_item_uid.as_deref(), Some(work_uid.as_str()));

    pr_work::assert_done_task_has_one_pr_work_item(pr_work::DonePrWorkCase {
        reopened: &reopened,
        reopened_store: &reopened_store,
        done: &done,
        task_uid: &task_uid,
        child_uid: &child_uid,
        expected_revision,
        spec_receipt,
        work_uid: &work_uid,
    });
}
