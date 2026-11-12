use super::*;

/// Production rendering boundary: a Done TaskIntent and Done planning children
/// must still expose an unmet lifecycle QA gate through the Board projection.
/// The lifecycle reducer tests receipt rules; this verifies the persisted
/// projection and rendered task card. Local file store and egui only; moderate
/// setup, with synthetic but validated lifecycle snapshots.
#[test]
fn done_planning_children_keep_runtime_qa_action_visible_on_board() {
    let mut app = fixture();
    let (root_uid, feature_uid, task_uid) = {
        let Screen::Connected(project) = &mut app.screen else {
            unreachable!()
        };
        let mut root = crate::core::planning_work::Work::new(
            String::new(),
            "Feature: restart recovery".into(),
            "Keep the recovery request moving through its gates".into(),
            String::new(),
        );
        root.kind = crate::core::planning_work::WorkKind::Feature;
        root.status = crate::core::planning_work::WorkStatus::Done;
        root.key = format!("task:{}", root.uid);
        root.task_intent_uid = Some(root.uid.clone());
        let root_uid = root.uid.clone();
        root.task_intent_cycle_uid = Some(root_uid.clone());
        root.task_intent_cycles = vec![crate::core::planning_work::TaskIntentCycleV1 {
            uid: root_uid.clone(),
            sequence: 1,
            goal: root.request.clone(),
            work_uid: root_uid.clone(),
            started_at_ms: None,
        }];
        let feature_uid = uuid::Uuid::new_v4().hyphenated().to_string();
        root.feature_uid = Some(feature_uid.clone());
        let children = (0..2)
            .map(|index| {
                let mut child = crate::core::planning_work::Work::new(
                    String::new(),
                    format!("Resolved planning item {index}"),
                    "Planning requirement resolved".into(),
                    String::new(),
                );
                child.kind = crate::core::planning_work::WorkKind::Question;
                child.status = crate::core::planning_work::WorkStatus::Done;
                child.parent_uid = Some(root_uid.clone());
                child.task_intent_uid = Some(root_uid.clone());
                child.task_intent_cycle_uid = Some(root_uid.clone());
                child
            })
            .collect::<Vec<_>>();
        project.planning_work = std::iter::once(root).chain(children).collect();
        project.save_planning_work().unwrap();

        let document = &mut project.task_documents[0];
        let planning_uid = document.identity.as_ref().unwrap().uid.clone();
        document.metadata = Some(
            document
                .metadata
                .take()
                .unwrap()
                .with_task_intent_uid(Some(&root_uid))
                .unwrap()
                .with_task_intent_cycle_uid(Some(&root_uid))
                .unwrap(),
        );
        let task_uid = uuid::Uuid::new_v4().hyphenated().to_string();
        let plan_hash = "a".repeat(64);
        let code_hash = "b".repeat(64);
        let reviewed_head = "c".repeat(40);
        let reviewed_subject = "d".repeat(64);
        let author_run = uuid::Uuid::new_v4().hyphenated().to_string();
        let reviewer_run = uuid::Uuid::new_v4().hyphenated().to_string();
        let author_session = uuid::Uuid::new_v4().hyphenated().to_string();
        let reviewer_session = uuid::Uuid::new_v4().hyphenated().to_string();
        let manifest =
            crate::harness::prompt_catalog::render(&crate::harness::prompt_catalog::RoleInputV1 {
                role: crate::harness::prompt_catalog::PromptRole::CodeReviewer,
                workspace_uid: project
                    .state
                    .planning_store
                    .project_id
                    .hyphenated()
                    .to_string(),
                feature_uid: feature_uid.clone(),
                task_uid: Some(task_uid.clone()),
                original_goal: "Review the approved recovery task".into(),
                approved_spec_ref: None,
                approved_spec_hash: None,
                decision_refs: vec![],
                evidence_manifest: vec![],
                execution_policy_ref: "code-review/fixture/v1".into(),
                output_schema_version: crate::harness::prompt_catalog::PromptRole::CodeReviewer
                    .output_schema_version()
                    .into(),
                payload: crate::harness::prompt_catalog::RolePayloadV1::Review {
                    subject_revision: "fixture-revision".into(),
                    subject_hash: code_hash.clone(),
                    author_run_id: author_run.clone(),
                    reviewer_run_id: reviewer_run.clone(),
                    author_session_id: Some(author_session.clone()),
                    reviewer_session_id: Some(reviewer_session.clone()),
                    requirements_matrix: vec![],
                    explicit_exclusions: vec![],
                },
            })
            .unwrap()
            .manifest;
        let manifest_hash = manifest.identity_sha256();
        let review_receipt = serde_json::json!({
            "receipt_uid": uuid::Uuid::new_v4().hyphenated().to_string(),
            "subject_uid": task_uid,
            "subject_sha256": code_hash,
            "actor": "independent_reviewer",
            "decision": "approved",
            "recorded_at_unix_ms": 1,
            "evidence_refs": ["fixture/code-review"],
            "prompt_sha256": manifest_hash,
            "prompt_manifest": manifest,
            "reviewer_run_id": reviewer_run,
            "author_run_id": author_run,
            "reviewer_session_id": reviewer_session,
            "author_session_id": author_session
        });
        let feature = serde_json::json!({
            "uid": feature_uid,
            "schemaVersion": 1,
            "createdAtMs": 1,
            "updatedAtMs": 1,
            "revision": 1,
            "record": {
                "schema_version": 1,
                "feature_uid": feature_uid,
                "original_intent": "Keep the recovery request moving through its gates",
                "draft": "Approved recovery behavior",
                "scope": [],
                "non_goals": [],
                "revision": 1,
                "phase": "executing",
                "approved_plan_sha256": plan_hash,
                "plan_snapshot_uid": null,
                "plan_snapshot_sha256": null,
                "unresolved_questions": [],
                "decision_refs": [],
                "review_refs": [],
                "readiness_evidence_refs": [],
                "task_uids": [task_uid]
            }
        });
        let task = serde_json::json!({
            "uid": task_uid,
            "schemaVersion": 1,
            "createdAtMs": 1,
            "updatedAtMs": 1,
            "revision": 1,
            "record": {
                "schema_version": 1,
                "task_uid": task_uid,
                "feature_uid": feature_uid,
                "planning_item_uid": planning_uid,
                "task_spec_sha256": null,
                "parent_task_uid": null,
                "subtask_uids": [],
                "dependency_task_uids": [],
                "approved_requirement_ids": [],
                "phase": "awaiting_runtime_qa",
                "revision": 1,
                "approved_plan_sha256": plan_hash,
                "implementation_sha256": code_hash,
                "reviewed_code_head_sha": reviewed_head,
                "reviewed_code_subject_sha256": reviewed_subject,
                "code_review": review_receipt,
                "review_history": [],
                "repair_attempts": 0,
                "runtime_qa": null,
                "qa_execution": {
                    "schemaVersion": 1,
                    "status": "pending",
                    "request": null,
                    "requestTaskRevision": null,
                    "dispatchTaskRevision": null,
                    "blockedReason": null,
                    "resultSha256": null,
                    "evidenceManifestSha256": null,
                    "reviewerExecutionId": null,
                    "reviewerSessionId": null,
                    "applicability": null
                },
                "spec_reconciliation": null,
                "spec_reconciliation_blocker": null,
                "spec_reconciliation_proposal_sha256": null,
                "spec_reconciliation_spec_sha256": null,
                "spec_reconciliation_proposal_json": null,
                "pr_work_item_uid": null
            }
        });
        let store = &project.state.planning_store;
        store
            .save_record(
                &format!("state/lifecycle_features/{feature_uid}.json"),
                &feature,
                0,
            )
            .unwrap();
        store
            .save_record(&format!("state/lifecycle_tasks/{task_uid}.json"), &task, 0)
            .unwrap();
        (root_uid, feature_uid, task_uid)
    };

    let board = app.planning_board();
    let root = board
        .planning_work
        .iter()
        .find(|work| work.uid == root_uid)
        .unwrap();
    assert_eq!(
        board.task_intent_lifecycle(root),
        crate::core::task_intent::TaskIntentLifecycle::AwaitingReview
    );
    assert!(
        board
            .lifecycle_features
            .iter()
            .any(|feature| feature.feature_uid == feature_uid)
    );
    assert!(board.lifecycle_feature_tasks.iter().any(|task| {
        task.task_uid == task_uid
            && task.phase == crate::core::lifecycle::TaskPhase::AwaitingRuntimeQa
    }));

    let context = super::mockup_layout::styled_context();
    let mut output = None;
    for _ in 0..5 {
        output = Some(frame(&mut app, &context, vec![]));
    }
    let text = canvas_text(&output.unwrap());
    assert!(text.contains("Runtime QA"), "rendered board: {text}");
    assert!(
        text.contains("Awaiting runtime QA"),
        "task remains explicitly gated: {text}"
    );
    assert!(text.contains("TASK INTENT"));
}
