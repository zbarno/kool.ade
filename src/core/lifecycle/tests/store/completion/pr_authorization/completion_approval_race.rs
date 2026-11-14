use super::*;

/// Production boundary: specification completion atomically creates the PR
/// authorization target while a user authorization races it; reopening the
/// file-backed lifecycle store and replaying the stale completion must converge
/// on one Done task, one PR item, and one immutable receipt. Store reducer tests do
/// not exercise this cross-record race or restart replay. Synthetic local Git
/// and private stores only; no provider, Docker, or network; moderate fixture.
#[test]
fn task_completion_racing_user_authorization_converges_after_restart() {
    let fixture = Fixture::new();
    let code_repo = fixture.code_repo();
    let (_base_commit, pr_head, _merge_commit) = current_support::init_merged_repo(&code_repo);
    let store = fixture.store();
    let lifecycle = LifecycleStore::new(&store);
    let (feature_uid, task_uid, _child_uid, _) = support::create_feature_graph(&lifecycle);
    let (task, _) =
        support::finish_task_through_qa_with_head(&fixture, &lifecycle, &task_uid, pr_head);
    let work_uid = uuid::Uuid::new_v4().hyphenated().to_string();
    let spec = GateReceipt {
        receipt_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
        subject_uid: task_uid.clone(),
        subject_sha256: task.implementation_sha256.clone().unwrap(),
        actor: GateActor::ApplicationPolicy,
        decision: GateDecision::Approved,
        recorded_at_unix_ms: 10,
        evidence_refs: vec!["spec/reconciled".into()],
        prompt_sha256: None,
        prompt_manifest: None,
        reviewer_run_id: None,
        author_run_id: None,
        reviewer_session_id: None,
        author_session_id: None,
    };
    let approval = GateReceipt {
        receipt_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
        subject_uid: work_uid.clone(),
        subject_sha256: task.implementation_sha256.clone().unwrap(),
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
    let event_uid = uuid::Uuid::new_v4().to_string();
    let task_revision = task.revision;
    let completion_work_uid = work_uid.clone();
    let completion_event_uid = event_uid.clone();
    let completion_task_uid = task_uid.clone();
    let completion_receipt = spec.clone();
    let authorization_work_uid = work_uid.clone();
    let authorization_receipt = approval.clone();
    let authorization_store = store.clone();
    let completion_store = store.clone();
    let barrier = std::sync::Barrier::new(3);
    let (completion, authorization) = std::thread::scope(|scope| {
        let completion_barrier = &barrier;
        let completion = scope.spawn(move || {
            completion_barrier.wait();
            LifecycleStore::new(&completion_store).apply_task_event(
                &completion_task_uid,
                task_revision,
                &completion_event_uid,
                TaskEvent::ReconcileSpec {
                    receipt: completion_receipt,
                    pr_work_item_uid: completion_work_uid,
                },
            )
        });
        let authorization_barrier = &barrier;
        let authorization = scope.spawn(move || {
            authorization_barrier.wait();
            LifecycleStore::new(&authorization_store).authorize_pr_publication(
                &authorization_work_uid,
                1,
                &uuid::Uuid::new_v4().to_string(),
                &authorization_receipt,
            )
        });
        barrier.wait();
        (completion.join().unwrap(), authorization.join().unwrap())
    });
    assert_eq!(completion.unwrap().phase, TaskPhase::Done);

    // Retry only while the first completion commit left the item awaiting auth.
    let authorization = match authorization {
        Ok(authorization) => authorization,
        Err(initial_error) => {
            let pending = lifecycle
                .load_pr_preparation(&work_uid)
                .expect("completion committed the publication work item");
            assert_eq!(
                pending.status,
                PullRequestPreparationStatus::AwaitingAuthorization,
                "do not retry approval after any other transition: {initial_error}"
            );
            lifecycle
                .authorize_pr_publication(
                    &work_uid,
                    pending.revision,
                    &uuid::Uuid::new_v4().to_string(),
                    &approval,
                )
                .unwrap_or_else(|retry_error| {
                    panic!("approval retry failed after task completion: {retry_error}")
                })
        }
    };
    assert_eq!(
        authorization.status,
        PullRequestPreparationStatus::Publishing
    );
    assert_eq!(
        authorization.publication_authorization.as_ref(),
        Some(&approval)
    );

    let reopened = LifecycleStore::new(&store);
    let done = reopened.load_task(&task_uid).unwrap();
    let saved = reopened.load_pr_preparation(&work_uid).unwrap();
    assert_eq!(done.phase, TaskPhase::Done);
    assert_eq!(done.pr_work_item_uid.as_deref(), Some(work_uid.as_str()));
    assert_eq!(saved.feature_uid, feature_uid);
    assert_eq!(saved.status, PullRequestPreparationStatus::Publishing);
    assert_eq!(saved.publication_authorization.as_ref(), Some(&approval));
    assert_eq!(
        reopened
            .list_tasks()
            .unwrap()
            .iter()
            .filter(|record| record.pr_work_item_uid.as_deref() == Some(work_uid.as_str()))
            .count(),
        1
    );
    assert!(
        reopened
            .apply_task_event(
                &task_uid,
                task_revision,
                &event_uid,
                TaskEvent::ReconcileSpec {
                    receipt: spec,
                    pr_work_item_uid: work_uid.clone(),
                },
            )
            .is_err(),
        "stale completion replay cannot replace the committed task or receipt"
    );
    assert_eq!(
        reopened
            .load_pr_preparation(&work_uid)
            .unwrap()
            .publication_authorization,
        Some(approval)
    );
}
