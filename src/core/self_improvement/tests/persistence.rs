use super::*;

#[test]
fn stale_revisions_and_noncanonical_pull_request_urls_are_rejected() {
    let mut run = run();
    assert!(run.apply(0, Event::BeginPlanning).is_err());
    run.apply(1, Event::BeginPlanning).unwrap();
    assert!(run.apply(1, Event::NeedsAttention).is_err());
    for url in [
        "http://github.com/zbarno/kool.ade/pull/1",
        "https://github.com.evil/zbarno/kool.ade/pull/1",
        "https://github.com/zbarno/kool.ade/pull/1?token=secret",
        "https://github.com/zbarno/kool.ade/pull/not-a-number",
    ] {
        assert!(
            !super::super::validation::valid_pr_url(url),
            "accepted {url}"
        );
    }
}

#[test]
fn persisted_run_and_events_retain_versioned_lifecycle_state() {
    let run = run();
    let encoded = serde_json::to_vec(&run).unwrap();
    assert_eq!(
        serde_json::from_slice::<SelfImprovementRunV1>(&encoded).unwrap(),
        run
    );

    let event = Event::PlanApproved {
        approval_sha256: sha('b'),
        capability_id: Some("self-improvement-lifecycle".into()),
        review_uid: uid(8),
        source_ref: "feature/approved-source".into(),
        destination_ref: "main".into(),
        task_uids: vec![uid(6)],
        owner_task_uid: uid(6),
        parent_task_uids: vec![uid(6)],
    };
    let encoded = serde_json::to_vec(&event).unwrap();
    assert_eq!(serde_json::from_slice::<Event>(&encoded).unwrap(), event);
}

#[test]
fn legacy_approved_run_and_plan_event_remain_readable_without_a_capability_id() {
    let mut run = run();
    run.apply(1, Event::BeginPlanning).unwrap();
    run.apply(
        2,
        Event::PlanApproved {
            approval_sha256: sha('b'),
            capability_id: Some("self-improvement-lifecycle".into()),
            review_uid: uid(8),
            source_ref: "feature/approved-source".into(),
            destination_ref: "main".into(),
            task_uids: vec![uid(6)],
            owner_task_uid: uid(6),
            parent_task_uids: vec![uid(6)],
        },
    )
    .unwrap();
    let mut old_run = serde_json::to_value(run).unwrap();
    old_run["schema_version"] = serde_json::json!(1);
    old_run.as_object_mut().unwrap().remove("capability_id");
    let old_run: SelfImprovementRunV1 = serde_json::from_value(old_run).unwrap();
    old_run
        .validate()
        .expect("schema v1 approved runs remain readable but cannot activate");
    assert!(old_run.capability_id.is_none());

    let mut old_event = serde_json::to_value(Event::PlanApproved {
        approval_sha256: sha('b'),
        capability_id: Some("self-improvement-lifecycle".into()),
        review_uid: uid(8),
        source_ref: "feature/approved-source".into(),
        destination_ref: "main".into(),
        task_uids: vec![uid(6)],
        owner_task_uid: uid(6),
        parent_task_uids: vec![uid(6)],
    })
    .unwrap();
    old_event["plan_approved"]
        .as_object_mut()
        .unwrap()
        .remove("capability_id");
    let decoded: Event = serde_json::from_value(old_event).unwrap();
    assert!(matches!(
        decoded,
        Event::PlanApproved {
            capability_id: None,
            ..
        }
    ));
}

#[test]
fn changed_implementation_hash_invalidates_downstream_evidence_and_terminal_runs_cannot_repair() {
    let mut run = run();
    run.apply(1, Event::BeginPlanning).unwrap();
    run.apply(
        2,
        Event::PlanApproved {
            approval_sha256: sha('b'),
            capability_id: Some("self-improvement-lifecycle".into()),
            review_uid: uid(8),
            source_ref: "feature/approved-source".into(),
            destination_ref: "main".into(),
            task_uids: vec![uid(6)],
            owner_task_uid: uid(6),
            parent_task_uids: vec![uid(6)],
        },
    )
    .unwrap();
    run.apply(3, Event::CloneAllocated { clone_uid: uid(7) })
        .unwrap();
    run.apply(
        4,
        Event::ImplementationFinished {
            head_sha: commit('c'),
            subject_sha256: sha('c'),
        },
    )
    .unwrap();
    run.apply(
        5,
        Event::CodeReviewPassed {
            receipt_uid: uid(9),
            subject_sha256: sha('c'),
        },
    )
    .unwrap();
    run.apply(
        6,
        Event::QaPassed {
            receipt_uid: uid(10),
            subject_sha256: sha('c'),
        },
    )
    .unwrap();
    run.apply(
        7,
        Event::SpecReconciled {
            receipt_uid: uid(11),
            subject_sha256: sha('c'),
        },
    )
    .unwrap();
    run.apply(
        8,
        Event::ImplementationFinished {
            head_sha: commit('d'),
            subject_sha256: sha('d'),
        },
    )
    .unwrap();
    assert_eq!(run.phase, Phase::Reviewing);
    assert_eq!(
        run.implementation_subject_sha256.as_deref(),
        Some(sha('d').as_str())
    );
    assert!(run.code_review_uids.is_empty());
    assert!(run.runtime_qa_uids.is_empty());
    assert!(run.spec_receipt_uid.is_none());
    assert!(run.pr_work_item_uid.is_none());

    let mut cancelled = run;
    cancelled.apply(9, Event::Cancel).unwrap();
    assert!(
        cancelled
            .apply(
                10,
                Event::ImplementationFinished {
                    head_sha: commit('e'),
                    subject_sha256: sha('e'),
                }
            )
            .is_err()
    );
}
