use self::fixture_support::{
    PR_URL, bind_fixture_provider_remote, bind_task_to_ticket, save_refresh_state,
    write_provider_response,
};
use super::{Fixture, pr_authorization::current_support, spec_sync, support};
use crate::core::lifecycle::{
    GateActor, GateDecision, GateReceipt, LifecycleStore, PullRequestPreparationStatus,
    SpecSyncTriggerKindV1, TaskEvent,
};
use std::fs;

#[path = "provider_refresh/fixture_support.rs"]
mod fixture_support;

/// Production boundary: the synchronous provider refresh adapter writes the
/// shared PlanningStore PR record and queue. Calling the store transition alone
/// cannot prove provider state parsing and lifecycle wiring. Uses local Git,
/// a temporary fake `gh` executable, no network/Docker, and moderate setup.
#[cfg(unix)]
#[test]
fn provider_refresh_persists_and_queues_pr_open_close_reopen_and_merge() {
    let _environment = crate::test_support::lock_process_environment();
    let fixture = Fixture::new();
    let (base_sha, head_sha, merge_sha) = current_support::init_merged_repo(&fixture.0);
    let store = fixture.store();
    let _ssh = bind_fixture_provider_remote(&fixture.0, &store);
    let lifecycle = LifecycleStore::new(&store);
    let (feature_uid, task_uid, _, _) = support::create_feature_graph(&lifecycle);
    let (mut task, _) = support::finish_task_through_qa_with_head(
        &fixture,
        &lifecycle,
        &task_uid,
        head_sha.clone(),
    );
    let work_uid = uuid::Uuid::new_v4().hyphenated().to_string();
    let spec_receipt = GateReceipt {
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
        author_session_id: None,
        reviewer_session_id: None,
    };
    task = lifecycle
        .apply_task_event(
            &task_uid,
            task.revision,
            &uuid::Uuid::new_v4().hyphenated().to_string(),
            TaskEvent::ReconcileSpec {
                receipt: spec_receipt,
                pr_work_item_uid: work_uid.clone(),
            },
        )
        .unwrap();
    let origin_events = lifecycle
        .list_spec_sync_queue(&feature_uid, false)
        .unwrap()
        .into_iter()
        .map(|entry| entry.source_event_uid)
        .collect();
    spec_sync::append_current_revision(
        &lifecycle,
        &store,
        &feature_uid,
        0,
        origin_events,
        task.reviewed_code_head_sha.clone(),
    );
    let work = lifecycle.load_pr_preparation(&work_uid).unwrap();
    let authorization = GateReceipt {
        receipt_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
        subject_uid: work_uid.clone(),
        subject_sha256: task.implementation_sha256.clone().unwrap(),
        actor: GateActor::User,
        decision: GateDecision::Approved,
        recorded_at_unix_ms: 11,
        evidence_refs: vec![format!("user-action:create-pr:{work_uid}")],
        prompt_sha256: None,
        prompt_manifest: None,
        reviewer_run_id: None,
        author_run_id: None,
        reviewer_session_id: None,
        author_session_id: None,
    };
    lifecycle
        .authorize_pr_publication(
            &work_uid,
            work.revision,
            &uuid::Uuid::new_v4().hyphenated().to_string(),
            &authorization,
        )
        .unwrap();

    let ticket = ".koolade-packet/planning/tasks/feature/001-pr-refresh.md";
    let ticket_markdown = crate::domain::ArtifactIdentity::preserve_markdown(
        "# PR refresh\n\nRepository: root\n",
        None,
        "TASK-PR-REFRESH",
        "PR refresh",
    )
    .unwrap();
    let task_ticket_uid = crate::domain::ArtifactIdentity::from_markdown(&ticket_markdown)
        .unwrap()
        .unwrap()
        .uid;
    fs::create_dir_all(fixture.0.join(ticket).parent().unwrap()).unwrap();
    fs::write(fixture.0.join(ticket), ticket_markdown).unwrap();
    bind_task_to_ticket(&fixture.0, &task_uid, &task_ticket_uid);
    let gh = fixture.0.join("fake-gh");
    save_refresh_state(
        &fixture.0,
        ticket,
        &task_ticket_uid,
        &base_sha,
        &head_sha,
        &gh,
    );
    let initial_pr_trigger_count = lifecycle
        .list_spec_sync_queue(&feature_uid, false)
        .unwrap()
        .into_iter()
        .filter(|entry| entry.trigger == SpecSyncTriggerKindV1::PullRequest)
        .count();

    for (provider_state, expected, transition_count) in [
        ("OPEN", PullRequestPreparationStatus::Opened, 1),
        ("CLOSED", PullRequestPreparationStatus::ClosedUnmerged, 2),
        ("OPEN", PullRequestPreparationStatus::Opened, 3),
        ("MERGED", PullRequestPreparationStatus::Merged, 4),
    ] {
        write_provider_response(&gh, provider_state, &base_sha, &head_sha, &merge_sha);
        crate::core::implementation::refresh_pr_with_store_for_test(
            &store,
            &fixture.0,
            ticket,
            gh.to_str().unwrap(),
        )
        .unwrap();

        let work = lifecycle.load_pr_preparation(&work_uid).unwrap();
        assert_eq!(work.status, expected);
        assert_eq!(work.provider_pr_url.as_deref(), Some(PR_URL));
        let triggers = lifecycle
            .list_spec_sync_queue(&feature_uid, false)
            .unwrap()
            .into_iter()
            .filter(|entry| entry.trigger == SpecSyncTriggerKindV1::PullRequest)
            .collect::<Vec<_>>();
        assert_eq!(triggers.len(), initial_pr_trigger_count + transition_count);
        assert!(triggers.iter().all(|entry| {
            entry.source_task_uid.as_deref() == Some(task_uid.as_str())
                && entry.source_event_uid == entry.uid
        }));
        if transition_count == 1 {
            let trigger_uids = triggers
                .iter()
                .map(|entry| entry.uid.clone())
                .collect::<Vec<_>>();
            crate::core::implementation::refresh_pr_with_store_for_test(
                &store,
                &fixture.0,
                ticket,
                gh.to_str().unwrap(),
            )
            .unwrap();
            let after_duplicate = lifecycle
                .list_spec_sync_queue(&feature_uid, false)
                .unwrap()
                .into_iter()
                .filter(|entry| entry.trigger == SpecSyncTriggerKindV1::PullRequest)
                .map(|entry| entry.uid)
                .collect::<Vec<_>>();
            assert_eq!(after_duplicate, trigger_uids);
        }
    }
    assert_eq!(
        lifecycle
            .load_current_implementation(&feature_uid)
            .unwrap()
            .unwrap()
            .merge_commit_sha,
        merge_sha
    );
}
