use super::*;

pub(crate) fn complete_spec_racing_pr_authorization(
    app: &mut crate::app::root::KooladeApp,
    lifecycle: &crate::core::lifecycle::LifecycleStore<'_>,
    task_uid: &str,
) -> (String, GateReceipt) {
    let crate::app::root::Screen::Connected(project) = &mut app.screen else {
        panic!("fixture should remain connected")
    };
    let task = lifecycle.load_task(task_uid).unwrap();
    let code_sha256 = task.implementation_sha256.clone().unwrap();
    let work_uid = uuid::Uuid::new_v5(
        &uuid::Uuid::parse_str(task_uid).unwrap(),
        format!(
            "lifecycle-pr-work:v1:{}:{code_sha256}",
            task.approved_plan_sha256
        )
        .as_bytes(),
    )
    .hyphenated()
    .to_string();
    let authorization = GateReceipt {
        receipt_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
        subject_uid: work_uid.clone(),
        subject_sha256: code_sha256,
        actor: GateActor::User,
        decision: GateDecision::Approved,
        recorded_at_unix_ms: 20,
        evidence_refs: vec![format!("user-action:create-pr:{work_uid}")],
        prompt_sha256: None,
        prompt_manifest: None,
        reviewer_run_id: None,
        author_run_id: None,
        reviewer_session_id: None,
        author_session_id: None,
    };
    let store = project.state.planning_store.clone();
    let event_uid = uuid::Uuid::new_v4().to_string();
    let barrier = std::sync::Barrier::new(2);
    let initial_authorization = std::thread::scope(|scope| {
        let authorization_barrier = &barrier;
        let authorization_store = store.clone();
        let authorization_work_uid = work_uid.clone();
        let authorization_event_uid = event_uid.clone();
        let authorization_receipt = authorization.clone();
        let authorization = scope.spawn(move || {
            authorization_barrier.wait();
            crate::core::lifecycle::LifecycleStore::new(&authorization_store)
                .authorize_pr_publication(
                    &authorization_work_uid,
                    1,
                    &authorization_event_uid,
                    &authorization_receipt,
                )
        });
        barrier.wait();
        crate::app::root::spec_reconciliation::test_support::complete(
            project,
            task_uid,
            crate::harness::prompt_catalog::SpecDeltaV1 { deltas: Vec::new() },
            false,
        );
        authorization.join().unwrap()
    });
    let initial_authorization = match initial_authorization {
        Ok(record) => record,
        Err(initial_error) => {
            let pending = lifecycle.load_pr_preparation(&work_uid).unwrap();
            assert_eq!(
                pending.status,
                crate::core::lifecycle::PullRequestPreparationStatus::AwaitingAuthorization,
                "only retry if completion committed an unauthorised item: {initial_error}"
            );
            lifecycle
                .authorize_pr_publication(&work_uid, pending.revision, &event_uid, &authorization)
                .unwrap()
        }
    };
    assert_eq!(
        initial_authorization.status,
        crate::core::lifecycle::PullRequestPreparationStatus::Publishing
    );
    assert_eq!(
        initial_authorization.publication_authorization.as_ref(),
        Some(&authorization)
    );
    (work_uid, authorization)
}
