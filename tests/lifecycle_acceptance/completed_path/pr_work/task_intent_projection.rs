use super::*;

/// Production boundary: persisted lifecycle task records and PR work status
/// flow through the board's shared TaskIntent projection. Store-level transition
/// tests cannot prove the board combines the separate publication record with
/// the completed implementation phase. Uses local synthetic stores only; setup
/// cost is one feature and its already-created task records.
pub(crate) fn assert_task_intent_publication_projection(
    lifecycle: &LifecycleStore<'_>,
    root: &koolade::core::planning_work::Work,
    expected: koolade::core::task_intent::TaskIntentLifecycle,
) {
    use koolade::{
        artifacts::task_docs::{TaskDocument, TaskMetadata},
        core::lifecycle::PullRequestPreparationStatus,
        ui::planning_board::ViewModel,
    };

    let tasks = lifecycle.list_tasks().unwrap();
    let feature = lifecycle
        .load_feature(&root.feature_uid.clone().unwrap())
        .unwrap();
    let batch_uid = id();
    let mut documents = Vec::new();
    let mut phases = std::collections::BTreeMap::new();
    let mut pr_statuses = std::collections::BTreeMap::new();
    for task in &tasks {
        let Some(planning_uid) = task.planning_item_uid.as_deref() else {
            continue;
        };
        let path = format!("planning/tasks/acceptance/{planning_uid}.md");
        let mut identity = koolade::domain::ArtifactIdentity::new(
            planning_uid,
            "Synthetic lifecycle acceptance task",
        );
        identity.uid = planning_uid.to_owned();
        identity.parent_uid = Some(batch_uid.clone());
        let mut metadata = TaskMetadata::new(&identity, "root", Vec::new()).unwrap();
        metadata.task_intent_uid = Some(root.uid.clone());
        documents.push(TaskDocument {
            path: path.clone(),
            title: "Synthetic lifecycle acceptance task".into(),
            text: "Synthetic persisted task for TaskIntent projection.".into(),
            identity: Some(identity),
            metadata: Some(metadata),
            // Deliberately stale success-like projection: the durable PR gate
            // below must remain authoritative until its typed receipt arrives.
            task_state: Some(koolade::artifacts::task_docs::TaskState {
                batch_uid: batch_uid.clone(),
                repository_id: "root".into(),
                dependency_uids: Vec::new(),
                status: koolade::core::planning_work::WorkStatus::Done,
                execution_status: None,
                source_branch: None,
                source_sha: None,
                destination_branch: None,
                git_intent: None,
                implementation_branch: None,
                publication_status:
                    koolade::artifacts::task_docs::ExecutionPublicationStatus::Verified,
                pull_requests: Vec::new(),
                revision: 1,
            }),
            metadata_error: None,
        });
        phases.insert(path.clone(), task.phase);
        if let Some(work_uid) = &task.pr_work_item_uid {
            let preparation = lifecycle.load_pr_preparation(work_uid).unwrap();
            pr_statuses.insert(path, preparation.status);
        }
    }
    let board = ViewModel {
        task_documents: documents,
        planning_work: vec![root.clone()],
        lifecycle_tasks: phases,
        lifecycle_pr_statuses: pr_statuses,
        lifecycle_features: vec![feature],
        lifecycle_feature_tasks: tasks,
        ..ViewModel::default()
    };
    assert_eq!(board.task_intent_lifecycle(root), expected);
    if expected == koolade::core::task_intent::TaskIntentLifecycle::PreparingPullRequest {
        assert_eq!(
            board.lifecycle_pr_statuses.values().next(),
            Some(&PullRequestPreparationStatus::AwaitingAuthorization)
        );
    }
}

pub(crate) fn authorize_publish_and_assert_task_intent(
    lifecycle: &LifecycleStore<'_>,
    store: &PlanningStore,
    done: &koolade::core::lifecycle::TaskRecord,
    work_uid: &str,
    feature_uid: &str,
) {
    use koolade::core::lifecycle::PullRequestPublicationState;

    let mut intent = koolade::core::planning_work::Work::new(
        "task-intent:completed-lifecycle".into(),
        "Complete the search dialog goal".into(),
        "Add an accessible search dialog".into(),
        String::new(),
    );
    intent.status = koolade::core::planning_work::WorkStatus::Done;
    intent.task_intent_uid = Some(intent.uid.clone());
    intent.feature_uid = Some(feature_uid.to_owned());
    assert_task_intent_publication_projection(
        lifecycle,
        &intent,
        koolade::core::task_intent::TaskIntentLifecycle::PreparingPullRequest,
    );

    let pending = lifecycle.load_pr_preparation(work_uid).unwrap();
    let authorization = GateReceipt {
        receipt_uid: id(),
        subject_uid: work_uid.into(),
        subject_sha256: done.implementation_sha256.clone().unwrap(),
        actor: GateActor::User,
        decision: GateDecision::Approved,
        recorded_at_unix_ms: 5,
        evidence_refs: vec![format!("user-action:create-pr:{work_uid}")],
        prompt_sha256: None,
        prompt_manifest: None,
        reviewer_run_id: None,
        author_run_id: None,
        reviewer_session_id: None,
        author_session_id: None,
    };
    let competing_authorization = GateReceipt {
        receipt_uid: id(),
        ..authorization.clone()
    };
    let event_a = id();
    let event_b = id();
    let barrier = std::sync::Barrier::new(3);
    let (first, second) = std::thread::scope(|scope| {
        let first_barrier = &barrier;
        let first_authorization = authorization;
        let first_event = event_a;
        let first = scope.spawn(move || {
            first_barrier.wait();
            LifecycleStore::new(store).authorize_pr_publication(
                work_uid,
                pending.revision,
                &first_event,
                &first_authorization,
            )
        });
        let second_barrier = &barrier;
        let second = scope.spawn(move || {
            second_barrier.wait();
            LifecycleStore::new(store).authorize_pr_publication(
                work_uid,
                pending.revision,
                &event_b,
                &competing_authorization,
            )
        });
        barrier.wait();
        (first.join().unwrap(), second.join().unwrap())
    });
    assert_ne!(
        first.is_ok(),
        second.is_ok(),
        "one concurrent authorization wins and the stale revision is rejected"
    );
    let publishing = first.or(second).unwrap();
    assert_eq!(publishing.status, PullRequestPreparationStatus::Publishing);
    assert_eq!(
        lifecycle
            .load_pr_preparation(work_uid)
            .unwrap()
            .publication_authorization
            .unwrap()
            .receipt_uid,
        publishing
            .publication_authorization
            .as_ref()
            .unwrap()
            .receipt_uid,
        "repeated lifecycle reads preserve the winning authorization"
    );
    const PR_URL: &str = "https://github.com/example/koolade-fixture/pull/123";
    let head_sha = "c".repeat(40);
    let base_sha = "d".repeat(40);
    let opened = lifecycle
        .record_pr_publication_state(
            work_uid,
            publishing.revision,
            &id(),
            PullRequestPublicationState {
                status: PullRequestPreparationStatus::Opened,
                provider_pr_id: Some(123),
                provider_pr_url: Some(PR_URL.into()),
                head_sha: Some(head_sha.clone()),
                base_sha: Some(base_sha.clone()),
                merged_commit_sha: None,
                lease_epoch: Some(publishing.publication_lease_epoch()),
            },
        )
        .unwrap();
    assert_task_intent_publication_projection(
        lifecycle,
        &intent,
        koolade::core::task_intent::TaskIntentLifecycle::PullRequestOpen,
    );

    let merged_commit = "e".repeat(40);
    let merged = lifecycle
        .record_pr_publication_state(
            work_uid,
            opened.revision,
            &id(),
            PullRequestPublicationState {
                status: PullRequestPreparationStatus::Merged,
                provider_pr_id: Some(123),
                provider_pr_url: Some(PR_URL.into()),
                head_sha: Some(head_sha),
                base_sha: Some(base_sha),
                merged_commit_sha: Some(merged_commit.clone()),
                lease_epoch: None,
            },
        )
        .unwrap();
    assert_eq!(
        merged.merged_commit_sha.as_deref(),
        Some(merged_commit.as_str())
    );

    let final_store = reopen_store(store);
    let final_lifecycle = LifecycleStore::new(&final_store);
    assert_eq!(
        final_lifecycle
            .load_pr_preparation(work_uid)
            .unwrap()
            .status,
        PullRequestPreparationStatus::Merged
    );
    assert_task_intent_publication_projection(
        &final_lifecycle,
        &intent,
        koolade::core::task_intent::TaskIntentLifecycle::Merged,
    );
}
