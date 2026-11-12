use super::*;

pub(super) fn build() -> (crate::app::root::KooladeApp, String, String, String) {
    let mut app = fixture();
    let (root_uid, task_uid, pr_uid) = {
        let Screen::Connected(project) = &mut app.screen else {
            unreachable!()
        };
        let mut root = crate::core::planning_work::Work::new(
            String::new(),
            "Feature: restart recovery".into(),
            "Keep the recovery request moving through publication".into(),
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
        let pr_uid = uuid::Uuid::new_v4().hyphenated().to_string();
        let plan_hash = "a".repeat(64);
        let code_hash = "b".repeat(64);
        let reviewed_head = "c".repeat(40);
        let reviewed_subject = "d".repeat(64);
        let author_run = uuid::Uuid::new_v4().hyphenated().to_string();
        let code_reviewer_run = uuid::Uuid::new_v4().hyphenated().to_string();
        let qa_reviewer_run = uuid::Uuid::new_v4().hyphenated().to_string();
        let author_session = uuid::Uuid::new_v4().hyphenated().to_string();
        let code_reviewer_session = uuid::Uuid::new_v4().hyphenated().to_string();
        let qa_reviewer_session = uuid::Uuid::new_v4().hyphenated().to_string();
        let workspace_uid = project
            .state
            .planning_store
            .project_id
            .hyphenated()
            .to_string();
        let code_review = independent_receipt(
            crate::harness::prompt_catalog::PromptRole::CodeReviewer,
            ReviewReceiptContext {
                workspace_uid: &workspace_uid,
                feature_uid: &feature_uid,
                task_uid: &task_uid,
                code_hash: &code_hash,
                author_run: &author_run,
                reviewer_run: &code_reviewer_run,
                author_session: &author_session,
                reviewer_session: &code_reviewer_session,
            },
            vec!["fixture/code-review".into()],
        );
        let qa_run_uid = uuid::Uuid::new_v4().hyphenated().to_string();
        let evidence_hash = "e".repeat(64);
        let result_hash = "f".repeat(64);
        let qa_evidence = vec![
            format!("runtime-qa:{qa_run_uid}/result"),
            format!("runtime-qa:{qa_run_uid}/evidence:{evidence_hash}"),
        ];
        let runtime_qa = independent_receipt(
            crate::harness::prompt_catalog::PromptRole::RuntimeQaReviewer,
            ReviewReceiptContext {
                workspace_uid: &workspace_uid,
                feature_uid: &feature_uid,
                task_uid: &task_uid,
                code_hash: &code_hash,
                author_run: &author_run,
                reviewer_run: &qa_reviewer_run,
                author_session: &author_session,
                reviewer_session: &qa_reviewer_session,
            },
            qa_evidence,
        );
        let qa_request = crate::runtime_qa::QaRunRequestV1 {
            schema_version: 1,
            mode: crate::runtime_qa::QaRunMode::Production,
            runtime_image_digest: None,
            runtime_image_profile: None,
            qa_run_id: qa_run_uid,
            parent_task_uid: task_uid.clone(),
            target_id: Some("fixture-target".into()),
            planning_item_uid: Some(planning_uid.clone()),
            feature_uid: feature_uid.clone(),
            code_head_sha: reviewed_head.clone(),
            build_artifact_sha256: "7".repeat(64),
            complete_diff_hash: code_hash.clone(),
            review_subject_sha256: reviewed_subject.clone(),
            approved_spec_hash: plan_hash.clone(),
            requirement_ids: vec!["REQ-RECOVERY".into()],
            acceptance_criteria: vec!["The approved recovery action remains visible.".into()],
            relevant_platforms: vec![crate::runtime_qa::QaPlatform::Web],
            startup_recipe_ref: "recipe:fixture/v1".into(),
            startup_recipe_sha256: "8".repeat(64),
            execution_session_id: uuid::Uuid::new_v4().hyphenated().to_string(),
            driver_profile_version: "fixture-driver/v1".into(),
            test_data_fixture_ref: None,
            dependency_cache_refs: vec![],
            bounded_limits: crate::runtime_qa::QaBoundedLimitsV1 {
                max_scenarios: 1,
                max_actions_per_scenario: 8,
                max_total_actions: 8,
                timeout_seconds: 60,
                max_evidence_bytes: 1024 * 1024,
                max_screenshots: 4,
            },
            author_run_id: author_run.clone(),
            author_session_id: author_session.clone(),
        };
        let spec_reconciliation = GateReceipt {
            receipt_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
            subject_uid: task_uid.clone(),
            subject_sha256: code_hash.clone(),
            actor: GateActor::ApplicationPolicy,
            decision: GateDecision::Approved,
            recorded_at_unix_ms: 1,
            evidence_refs: vec!["fixture/spec-reconciled".into()],
            prompt_sha256: None,
            prompt_manifest: None,
            reviewer_run_id: None,
            author_run_id: None,
            reviewer_session_id: None,
            author_session_id: None,
        };
        let feature = serde_json::json!({
            "uid": feature_uid,
            "schemaVersion": 1,
            "createdAtMs": 1,
            "updatedAtMs": 1,
            "revision": 1,
            "record": {
                "schema_version": 1,
                "feature_uid": feature_uid,
                "original_intent": "Keep the recovery request moving through publication",
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
                "approved_requirement_ids": ["REQ-RECOVERY"],
                "phase": "done",
                "revision": 1,
                "approved_plan_sha256": plan_hash,
                "implementation_sha256": code_hash,
                "reviewed_code_head_sha": reviewed_head,
                "reviewed_code_subject_sha256": reviewed_subject,
                "code_review": code_review,
                "review_history": [],
                "repair_attempts": 0,
                "runtime_qa": runtime_qa,
                "qa_execution": {
                    "schemaVersion": 1,
                    "status": "completed",
                    "request": qa_request,
                    "requestTaskRevision": null,
                    "dispatchTaskRevision": null,
                    "blockedReason": null,
                    "resultSha256": result_hash,
                    "evidenceManifestSha256": evidence_hash,
                    "reviewerExecutionId": qa_reviewer_run,
                    "reviewerSessionId": qa_reviewer_session,
                    "applicability": null
                },
                "spec_reconciliation": spec_reconciliation,
                "spec_reconciliation_blocker": null,
                "spec_reconciliation_proposal_sha256": null,
                "spec_reconciliation_spec_sha256": null,
                "spec_reconciliation_proposal_json": null,
                "pr_work_item_uid": pr_uid
            }
        });
        let preparation = serde_json::json!({
            "uid": pr_uid,
            "schemaVersion": 1,
            "createdAtMs": 1,
            "updatedAtMs": 1,
            "revision": 1,
            "record": {
                "schemaVersion": 1,
                "uid": pr_uid,
                "workItemUid": pr_uid,
                "featureUid": feature_uid,
                "taskUid": task_uid,
                "includedTaskUids": [task_uid],
                "revision": 1,
                "createdAtMs": 1,
                "updatedAtMs": 1,
                "status": "awaiting_authorization",
                "publicationAuthorization": null,
                "publicationScope": null,
                "leaseEpoch": 0,
                "publicationAuthorizationEventUids": [],
                "recovery": null,
                "providerPrId": null,
                "providerPrUrl": null,
                "headSha": null,
                "baseSha": null,
                "mergedCommitSha": null
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
        store
            .save_record(
                &format!("state/lifecycle_pr_work_items/{pr_uid}.json"),
                &preparation,
                0,
            )
            .unwrap();
        (root_uid, task_uid, pr_uid)
    };

    (app, root_uid, task_uid, pr_uid)
}
