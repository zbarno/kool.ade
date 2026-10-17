use crate::{
    app::root::{
        Screen,
        spec_reconciliation::test_support::{complete, fixture},
    },
    core::lifecycle::{
        LifecycleStore, SpecOriginOutcomeV1, SpecReconciliationStatusV1, SpecSyncQueueStatusV1,
        SpecSyncTriggerKindV1, TaskPhase,
    },
    harness::prompt_catalog::{ApprovalEffect, DeltaClassification, SpecChangeV1, SpecDeltaV1},
};

#[test]
// Production boundary: polling a completed worker result persists history,
// queue acknowledgments and Done/PR work. A reducer-only test cannot prove
// sibling pending events allow both controller results to commit. Synthetic
// local app/Git fixture; moderate setup, no provider, Docker or network.
fn both_standalone_candidates_complete_without_starving_a_sibling() {
    let (mut app, feature_uid, ids, _) = fixture(2);
    let Screen::Connected(project) = &mut app.screen else {
        panic!("connected")
    };
    let mut visited = Vec::new();
    for _ in 0..2 {
        let candidates = LifecycleStore::new(&project.state.planning_store)
            .spec_reconciliation_candidates()
            .unwrap();
        let uid = candidates
            .iter()
            .find(|task| task.phase == TaskPhase::AwaitingSpecReconciliation)
            .unwrap()
            .task_uid
            .clone();
        assert!(!visited.contains(&uid));
        complete(project, &uid, SpecDeltaV1 { deltas: Vec::new() }, false);
        assert_eq!(
            LifecycleStore::new(&project.state.planning_store)
                .load_task(&uid)
                .unwrap()
                .phase,
            TaskPhase::Done
        );
        let lifecycle = LifecycleStore::new(&project.state.planning_store);
        let revision = lifecycle.load_spec_revision(&feature_uid).unwrap().unwrap();
        let dispositions = lifecycle
            .specification_origin_dispositions(&feature_uid, revision.seq)
            .unwrap()
            .unwrap();
        assert!(!dispositions.is_empty());
        assert!(dispositions.iter().all(|disposition| matches!(
            &disposition.outcome,
            SpecOriginOutcomeV1::NoChange { reason } if reason.contains("validated SpecReconciler")
        )));
        let completion = lifecycle
            .list_spec_sync_queue(&feature_uid, true)
            .unwrap()
            .into_iter()
            .find(|entry| {
                entry.source_task_uid.as_deref() == Some(uid.as_str())
                    && entry.trigger == SpecSyncTriggerKindV1::SpecificationReconciliation
                    && entry.status == SpecSyncQueueStatusV1::Declined
            })
            .unwrap();
        assert!(
            completion
                .decline_reason
                .unwrap()
                .contains("introduces no new")
        );
        visited.push(uid);
    }
    assert!(ids.iter().all(|uid| visited.contains(uid)));
    assert!(
        LifecycleStore::new(&project.state.planning_store)
            .load_spec_revision(&feature_uid)
            .unwrap()
            .is_some()
    );
}

#[test]
// Same controller/storage boundary and local setup as the sibling regression.
fn task_result_does_not_acknowledge_an_unprocessed_manual_request() {
    let (mut app, feature_uid, ids, _) = fixture(1);
    let Screen::Connected(project) = &mut app.screen else {
        panic!("connected")
    };
    let trigger = LifecycleStore::new(&project.state.planning_store)
        .enqueue_manual_spec_refresh_correlated(
            &feature_uid,
            "manual-during-task",
            Some("fixture-user-message"),
        )
        .unwrap();
    complete(project, &ids[0], SpecDeltaV1 { deltas: Vec::new() }, false);
    let lifecycle = LifecycleStore::new(&project.state.planning_store);
    let manual = lifecycle
        .list_spec_sync_queue(&feature_uid, false)
        .unwrap()
        .into_iter()
        .find(|entry| entry.uid == trigger.uid)
        .unwrap();
    assert_eq!(manual.status, SpecSyncQueueStatusV1::Pending);
    assert_eq!(
        manual.request_message_id.as_deref(),
        Some("fixture-user-message")
    );
    assert_eq!(
        lifecycle.load_task(&ids[0]).unwrap().phase,
        TaskPhase::AwaitingSpecReconciliation
    );
}

#[test]
// Same controller/storage boundary and local setup; this covers the post-PR
// branch rather than the pre-Done result branch.
fn pr_refresh_proposal_is_persisted_with_a_reason_and_survives_reopen() {
    let (mut app, feature_uid, ids, requirement_id) = fixture(1);
    let Screen::Connected(project) = &mut app.screen else {
        panic!("connected")
    };
    complete(project, &ids[0], SpecDeltaV1 { deltas: Vec::new() }, false);
    complete(
        project,
        &ids[0],
        SpecDeltaV1 {
            deltas: vec![SpecChangeV1 {
                requirement_id,
                old: "Old reviewed wording".into(),
                new: "Proposed wording".into(),
                classification: DeltaClassification::FutureTarget,
                evidence: vec![format!("task:{}", ids[0])],
                approval_effect: ApprovalEffect::RequiresUserApproval,
                followup_task_ids: Vec::new(),
            }],
        },
        true,
    );
    let reopened = project.state.planning_store.clone();
    let (revision, deltas) = LifecycleStore::new(&reopened)
        .spec_revision_history(&feature_uid)
        .unwrap()
        .pop()
        .unwrap();
    assert_eq!(
        revision.reconciliation_status,
        SpecReconciliationStatusV1::NeedsHumanDecision
    );
    assert!(revision.outcome_reason.is_some());
    assert_eq!(deltas.len(), 1);
    assert_eq!(
        deltas[0].proposed_target.as_deref(),
        Some("Proposed wording")
    );
}

#[test]
// Production boundary: the feature-result adapter commits a manual request's
// disposition with its queue acknowledgement while preserving unrelated task
// triggers. Store-only tests cannot prove the adapter supplies the rationale.
// Same local app/Git fixture and moderate setup; no external service dependency.
fn empty_feature_result_records_a_reason_only_for_its_feature_origins() {
    let (mut app, feature_uid, _, _) = fixture(1);
    let Screen::Connected(project) = &mut app.screen else {
        panic!("connected")
    };
    let lifecycle = LifecycleStore::new(&project.state.planning_store);
    let feature = lifecycle.load_feature(&feature_uid).unwrap();
    let trigger = lifecycle
        .enqueue_manual_spec_refresh(&feature_uid, "feature-outcome")
        .unwrap();
    let task_pending = lifecycle
        .list_spec_sync_queue(&feature_uid, false)
        .unwrap()
        .into_iter()
        .filter(|entry| entry.source_task_uid.is_some())
        .map(|entry| entry.uid)
        .collect::<Vec<_>>();
    assert!(!task_pending.is_empty());
    super::finish::record_feature(
        project,
        &feature,
        std::slice::from_ref(&trigger),
        Some(&SpecDeltaV1 { deltas: Vec::new() }),
        Some(&uuid::Uuid::new_v4().to_string()),
        None,
    )
    .unwrap();
    let revision = lifecycle.load_spec_revision(&feature_uid).unwrap().unwrap();
    let dispositions = lifecycle
        .specification_origin_dispositions(&feature_uid, revision.seq)
        .unwrap()
        .unwrap();
    assert_eq!(dispositions.len(), 1);
    assert_eq!(dispositions[0].origin_event_uid, trigger.uid);
    assert!(
        matches!(&dispositions[0].outcome, SpecOriginOutcomeV1::NoChange { reason }
        if reason.contains("no documentation or target change was proposed")
            && reason.contains("no current implementation claim"))
    );
    let pending = lifecycle.list_spec_sync_queue(&feature_uid, false).unwrap();
    assert!(pending.iter().all(|entry| entry.uid != trigger.uid));
    assert!(
        task_pending
            .iter()
            .all(|uid| pending.iter().any(|entry| &entry.uid == uid))
    );
}
