use super::*;

fn authorize_source(
    runs: &SelfImprovementStore<'_>,
    run: &SelfImprovementRunV1,
    task_uid: &str,
    source_ref: Option<&str>,
    source_sha: Option<&str>,
    destination_ref: Option<&str>,
    isolated_clone: bool,
) -> Result<(), crate::artifacts::planning_store::StoreError> {
    runs.authorize_task_source(TaskSourceAuthorization {
        feature_uid: &run.feature_uid,
        task_uid,
        repository_uid: &run.target_repository_uid,
        repository_remote: &run.canonical_remote,
        source_ref,
        source_sha,
        destination_ref,
        isolated_clone,
    })
}

#[test]
fn dispatch_is_fenced_to_approved_self_improvement_plan_and_target() {
    let temp = TempDir::new();
    let store = PlanningStore::new(
        uuid::Uuid::new_v4(),
        temp.0.join("planning"),
        StoreMode::ManagedLocal,
    );
    let opportunities = OpportunityStore::new(&store);
    let mut proposal = opportunity();
    proposal.target.repository_uid = crate::core::project_repos::stable_uid(
        &store.project_id.hyphenated().to_string(),
        "repository",
        "product",
    );
    let OpportunitySaveResult::Created {
        proposal,
        record_revision,
    } = opportunities.save_advisory(proposal).unwrap()
    else {
        panic!("fixture opportunity should be created")
    };
    let decision = decision_for(&proposal);
    opportunities
        .record_user_decision(record_revision, &decision, "local-controller")
        .unwrap();
    let runs = SelfImprovementStore::new(&store);
    let created = runs
        .create_from_pursued(&opportunities, &decision.uid, run(&decision.uid, &proposal))
        .unwrap();
    let planning = created;
    let mapped_repository_uid = crate::core::project_repos::stable_uid(
        &store.project_id.hyphenated().to_string(),
        "repository",
        "product",
    );
    assert_eq!(mapped_repository_uid, planning.target_repository_uid);
    let task_uid = uuid::Uuid::new_v4().to_string();
    let child_task_uid = uuid::Uuid::new_v4().to_string();
    assert!(
        runs.authorize_task_start(
            &planning.feature_uid,
            Some(&task_uid),
            &mapped_repository_uid,
            "https://github.com/zbarno/kool.ade",
        )
        .is_err(),
        "a persisted Pursue decision cannot dispatch code before reviewed-plan approval"
    );
    let ready = runs
        .apply_event(
            &planning.run_uid,
            planning.revision,
            &uuid::Uuid::new_v4().to_string(),
            SelfImprovementEventV1::PlanApproved {
                approval_sha256: "d".repeat(64),
                capability_id: Some("self-improvement-lifecycle".into()),
                review_uid: uuid::Uuid::new_v4().to_string(),
                source_ref: "feature/approved".into(),
                destination_ref: "main".into(),
                task_uids: vec![task_uid.clone(), child_task_uid.clone()],
                owner_task_uid: task_uid.clone(),
                parent_task_uids: vec![task_uid.clone()],
            },
        )
        .unwrap();
    assert_eq!(ready.phase, SelfImprovementPhaseV1::Ready);
    let allowed = runs.authorize_task_start(
        &ready.feature_uid,
        Some(&task_uid),
        &mapped_repository_uid,
        "https://github.com/zbarno/kool.ade",
    );
    assert!(
        allowed.is_ok(),
        "approved self-improvement task should pass dispatch authorization"
    );
    assert!(
        runs.authorize_task_start(
            &ready.feature_uid,
            Some(&child_task_uid),
            &mapped_repository_uid,
            "https://github.com/zbarno/kool.ade",
        )
        .is_ok(),
        "reviewed child tasks must use the same approved target and clone fence"
    );
    let implementing = runs
        .apply_event(
            &ready.run_uid,
            ready.revision,
            &uuid::Uuid::new_v4().to_string(),
            SelfImprovementEventV1::CloneAllocated {
                clone_uid: uuid::Uuid::new_v4().to_string(),
            },
        )
        .unwrap();
    let reviewing = runs
        .apply_event(
            &ready.run_uid,
            implementing.revision,
            &uuid::Uuid::new_v4().to_string(),
            SelfImprovementEventV1::ImplementationFinished {
                head_sha: "a".repeat(40),
                subject_sha256: "b".repeat(64),
            },
        )
        .unwrap();
    let qa = runs
        .apply_event(
            &ready.run_uid,
            reviewing.revision,
            &uuid::Uuid::new_v4().to_string(),
            SelfImprovementEventV1::CodeReviewPassed {
                receipt_uid: uuid::Uuid::new_v4().to_string(),
                subject_sha256: "b".repeat(64),
            },
        )
        .unwrap();
    assert_eq!(qa.phase, SelfImprovementPhaseV1::Qa);
    assert!(
        runs.authorize_task_start(
            &qa.feature_uid,
            Some(&task_uid),
            &qa.target_repository_uid,
            &qa.canonical_remote,
        )
        .is_ok()
    );
    assert!(
        runs.authorize_task_start(
            &qa.feature_uid,
            Some(&task_uid),
            &qa.target_repository_uid,
            "https://github.com/another/repository",
        )
        .is_err(),
        "a manifest alias pointing at another remote cannot reuse the approved repository ID"
    );
    assert!(
        authorize_source(
            &runs,
            &qa,
            &task_uid,
            qa.source_ref.as_deref(),
            Some(&qa.source_sha),
            qa.destination_ref.as_deref(),
            true,
        )
        .is_ok()
    );
    let wrong_sha = "f".repeat(40);
    assert!(
        authorize_source(
            &runs,
            &qa,
            &task_uid,
            qa.source_ref.as_deref(),
            Some(&wrong_sha),
            qa.destination_ref.as_deref(),
            true,
        )
        .is_err()
    );
    assert!(
        authorize_source(
            &runs,
            &qa,
            &task_uid,
            Some("feature/unapproved"),
            Some(&qa.source_sha),
            qa.destination_ref.as_deref(),
            true,
        )
        .is_err()
    );
    assert!(
        authorize_source(
            &runs,
            &qa,
            &task_uid,
            qa.source_ref.as_deref(),
            Some(&qa.source_sha),
            Some("release"),
            true,
        )
        .is_err()
    );
    assert!(
        authorize_source(
            &runs,
            &qa,
            &task_uid,
            qa.source_ref.as_deref(),
            Some(&qa.source_sha),
            qa.destination_ref.as_deref(),
            false,
        )
        .is_err()
    );
    assert!(
        runs.authorize_task_start(
            &ready.feature_uid,
            Some(&uuid::Uuid::new_v4().to_string()),
            &mapped_repository_uid,
            "https://github.com/zbarno/kool.ade",
        )
        .is_err()
    );
    assert!(
        runs.authorize_task_start(
            &ready.feature_uid,
            Some(&task_uid),
            "product",
            "https://github.com/zbarno/kool.ade",
        )
        .is_err(),
        "manifest aliases must be translated to the stable proposal identity"
    );
    assert!(
        runs.authorize_task_start(
            &ready.feature_uid,
            Some(&task_uid),
            &uuid::Uuid::new_v4().to_string(),
            "https://github.com/zbarno/kool.ade",
        )
        .is_err()
    );
}

#[test]
fn product_opportunity_cannot_fall_back_to_regular_dispatch_when_its_run_is_missing() {
    let temp = TempDir::new();
    let store = PlanningStore::new(
        uuid::Uuid::new_v4(),
        temp.0.join("planning"),
        StoreMode::ManagedLocal,
    );
    let opportunities = OpportunityStore::new(&store);
    let proposal = opportunity();
    let OpportunitySaveResult::Created {
        proposal,
        record_revision,
    } = opportunities.save_advisory(proposal).unwrap()
    else {
        panic!("fixture opportunity should be created")
    };
    let decision = decision_for(&proposal);
    opportunities
        .record_user_decision(record_revision, &decision, "local-controller")
        .unwrap();
    let runs = SelfImprovementStore::new(&store);
    let created = runs
        .create_from_pursued(&opportunities, &decision.uid, run(&decision.uid, &proposal))
        .unwrap();
    std::fs::remove_file(
        store
            .root
            .join("state/self_improvements")
            .join(format!("{}.json", created.run_uid)),
    )
    .unwrap();

    let error = runs
        .authorize_task_start(
            &created.feature_uid,
            Some(&uuid::Uuid::new_v4().to_string()),
            &created.target_repository_uid,
            &created.canonical_remote,
        )
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("missing its persisted self-improvement run")
    );
}
