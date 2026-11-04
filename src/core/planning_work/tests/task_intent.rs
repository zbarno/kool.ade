use super::*;

#[test]
fn planning_children_keep_the_root_identity_separate_from_nesting() {
    let root = super::root();
    let mut intent = Work::new(
        String::new(),
        "Feature: safer recovery".into(),
        "Make recovery safe after interruption".into(),
        "Planning in progress".into(),
    );
    intent.kind = WorkKind::Feature;
    intent.key = format!("task:{}", intent.uid);
    intent.task_intent_uid = Some(intent.uid.clone());

    let mut child = Work::new(
        String::new(),
        "Question: retry policy".into(),
        "Clarify retry behavior".into(),
        "Waiting for an answer".into(),
    );
    child.kind = WorkKind::Question;
    child.key = format!("task:{}", child.uid);
    child.parent_uid = Some(intent.uid.clone());
    child.task_intent_uid = Some(intent.uid.clone());

    save(&root, &[intent.clone(), child.clone()]).unwrap();
    let reopened = load(&root).unwrap();
    let reopened_child = reopened.iter().find(|work| work.uid == child.uid).unwrap();
    let reopened_root = reopened.iter().find(|work| work.uid == intent.uid).unwrap();

    assert_eq!(
        reopened_child.parent_uid.as_deref(),
        Some(intent.uid.as_str())
    );
    assert_eq!(
        reopened_child.task_intent_root_uid(),
        Some(intent.uid.as_str())
    );
    assert_eq!(
        reopened_root
            .task_intent_history
            .iter()
            .map(|event| event.kind.clone())
            .collect::<Vec<_>>(),
        vec![
            TaskIntentEventKindV1::Created,
            TaskIntentEventKindV1::WorkLinked
        ]
    );
    assert_eq!(
        reopened
            .iter()
            .find(|work| work.uid == intent.uid)
            .unwrap()
            .uid,
        intent.uid
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn task_intent_immutability_and_child_identity_survive_restart() {
    let repo = super::root();
    let mut intent = Work::new(
        "task:legacy-root".into(),
        "Original request".into(),
        "Keep the requested outcome stable".into(),
        String::new(),
    );
    intent.kind = WorkKind::Question;
    let original_uid = intent.uid.clone();
    let mut child = Work::new(
        "task:legacy-child".into(),
        "Follow-up question".into(),
        "Clarify a detail".into(),
        String::new(),
    );
    child.kind = WorkKind::Question;
    child.parent_uid = Some(original_uid.clone());
    save(&repo, &[intent.clone(), child.clone()]).unwrap();

    let mut reopened = load(&repo).unwrap();
    let root = reopened
        .iter_mut()
        .find(|work| work.uid == original_uid)
        .unwrap();
    root.request = "Refined scope after clarification".into();
    save(&repo, &reopened).unwrap();
    let reopened = load(&repo).unwrap();
    let root = reopened
        .iter()
        .find(|work| work.uid == original_uid)
        .unwrap();
    let child = reopened.iter().find(|work| work.uid == child.uid).unwrap();
    assert_eq!(
        root.original_goal.as_deref(),
        Some("Keep the requested outcome stable")
    );
    assert_eq!(
        child.task_intent_uid.as_deref(),
        Some(original_uid.as_str())
    );
    assert!(root.task_intent_history.iter().any(|event| {
        event.kind == TaskIntentEventKindV1::GoalRevised
            && event.summary == "Refined scope after clarification"
    }));

    let mut tampered = reopened;
    tampered
        .iter_mut()
        .find(|work| work.uid == original_uid)
        .unwrap()
        .original_goal = Some("Changed original request".into());
    assert!(save(&repo, &tampered).is_err());
    std::fs::remove_dir_all(repo).unwrap();
}

#[test]
fn loading_an_incomplete_task_intent_rejects_it_without_backfilling_storage() {
    let repo = super::root();
    let mut intent = Work::new(
        String::new(),
        "Feature: explicit task intent".into(),
        "Keep task identity explicit".into(),
        String::new(),
    );
    intent.kind = WorkKind::Feature;
    intent.task_intent_uid = Some(intent.uid.clone());
    save(&repo, &[intent.clone()]).unwrap();

    let path = repo.join(format!("state/work/{}.json", intent.uid));
    let mut record: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    let data = record["data"].as_object_mut().unwrap();
    data.remove("taskIntentUid");
    data.remove("originalGoal");
    let bytes = serde_json::to_vec_pretty(&record).unwrap();
    std::fs::write(&path, &bytes).unwrap();

    let before_revision = crate::artifacts::planning_store::PlanningStore::new(
        uuid::Uuid::nil(),
        &repo,
        crate::artifacts::planning_store::StoreMode::ManagedLocal,
    )
    .revision()
    .unwrap();
    assert!(load_expected(&repo, &before_revision).is_err());
    assert_eq!(std::fs::read(path).unwrap(), bytes);
}

#[test]
fn planning_work_cannot_claim_an_unknown_or_non_root_intent() {
    let root = super::root();
    let mut child = Work::new(
        "task:child".into(),
        "Question".into(),
        "Clarify the behavior".into(),
        "Open".into(),
    );
    child.task_intent_uid = Some(uuid::Uuid::new_v4().to_string());

    assert!(save(&root, &[child]).is_err());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn follow_up_cycles_keep_prior_outcomes_and_fence_replayed_starts() {
    let repo = super::root();
    let mut root_work = Work::new(
        String::new(),
        "Feature: original request".into(),
        "Build the original capability".into(),
        "Original cycle completed".into(),
    );
    root_work.key = format!("task:{}", root_work.uid);
    root_work.kind = WorkKind::Feature;
    root_work.status = WorkStatus::Done;
    root_work.feature_uid = Some(uuid::Uuid::new_v4().to_string());
    root_work.task_intent_uid = Some(root_work.uid.clone());
    let root_uid = root_work.uid.clone();
    let old_feature_uid = root_work.feature_uid.clone();
    save(&repo, &[root_work]).unwrap();

    let first_snapshot = load(&repo).unwrap();
    let second_snapshot = first_snapshot.clone();
    let mut first_writer = first_snapshot;
    let cycle_uid = start_follow_up_cycle(
        &mut first_writer,
        &root_uid,
        "Add an explicitly requested follow-up change",
    )
    .unwrap();
    save(&repo, &first_writer).unwrap();

    let reopened = load(&repo).unwrap();
    let root = reopened.iter().find(|item| item.uid == root_uid).unwrap();
    let cycle = root
        .task_intent_cycles
        .iter()
        .find(|cycle| cycle.uid == cycle_uid)
        .unwrap();
    let cycle_work = reopened
        .iter()
        .find(|item| item.uid == cycle.work_uid)
        .unwrap();
    assert_eq!(root.task_intent_cycles.len(), 2);
    assert_eq!(
        root.original_goal.as_deref(),
        Some("Build the original capability")
    );
    assert_eq!(root.feature_uid, old_feature_uid);
    assert_eq!(cycle.sequence, 2);
    assert_eq!(cycle_work.parent_uid.as_deref(), Some(root_uid.as_str()));
    assert_eq!(
        cycle_work.task_intent_uid.as_deref(),
        Some(root_uid.as_str())
    );
    assert_eq!(
        cycle_work.task_intent_cycle_uid.as_deref(),
        Some(cycle_uid.as_str())
    );
    assert_eq!(cycle_work.feature_uid, None);
    assert!(root.task_intent_history.iter().any(|event| {
        event.kind == TaskIntentEventKindV1::FollowUpCycleStarted
            && event.related_uid.as_deref() == Some(cycle_work.uid.as_str())
    }));

    let mut stale_writer = second_snapshot;
    start_follow_up_cycle(&mut stale_writer, &root_uid, "Replayed follow-up").unwrap();
    assert!(save(&repo, &stale_writer).is_err());
    std::fs::remove_dir_all(repo).unwrap();
}

#[test]
fn task_intent_archive_and_restore_are_durable_history_events() {
    let repo = super::root();
    let mut intent = Work::new(
        String::new(),
        "Answered question".into(),
        "Preserve the original question".into(),
        "Answer recorded".into(),
    );
    intent.kind = WorkKind::Question;
    intent.status = WorkStatus::Done;
    intent.key = format!("task:{}", intent.uid);
    intent.task_intent_uid = Some(intent.uid.clone());
    let uid = intent.uid.clone();
    save(&repo, &[intent]).unwrap();

    let mut work = load(&repo).unwrap();
    work.iter_mut()
        .find(|item| item.uid == uid)
        .unwrap()
        .task_intent_archived_at_ms = Some(1_800_000_000_000);
    save(&repo, &work).unwrap();
    let mut work = load(&repo).unwrap();
    assert!(
        work.iter()
            .find(|item| item.uid == uid)
            .unwrap()
            .task_intent_history
            .iter()
            .any(|event| event.kind == TaskIntentEventKindV1::Archived)
    );

    work.iter_mut()
        .find(|item| item.uid == uid)
        .unwrap()
        .task_intent_archived_at_ms = None;
    save(&repo, &work).unwrap();
    let restored = load(&repo).unwrap();
    let intent = restored.iter().find(|item| item.uid == uid).unwrap();
    assert_eq!(intent.task_intent_archived_at_ms, None);
    assert!(
        intent
            .task_intent_history
            .iter()
            .any(|event| event.kind == TaskIntentEventKindV1::Restored)
    );
    std::fs::remove_dir_all(repo).unwrap();
}

#[test]
fn decision_and_approval_history_is_cycle_scoped_and_idempotent() {
    let repo = super::root();
    let mut root = Work::new(
        String::new(),
        "Feature: scoped history".into(),
        "Keep decisions and approvals attached to the request".into(),
        "Planning".into(),
    );
    root.kind = WorkKind::Feature;
    root.key = format!("task:{}", root.uid);
    root.feature_id = Some("CHG-009".into());
    root.feature_uid = Some(uuid::Uuid::new_v4().to_string());
    root.task_intent_uid = Some(root.uid.clone());
    let root_uid = root.uid.clone();
    let mut decision = crate::domain::OpenItem::new(
        "CLR-009".into(),
        crate::domain::Priority::Blocking,
        crate::domain::ItemKind::Question,
        "Product".into(),
        "Should the migration be reversible?".into(),
        "The approved scope depends on this choice.".into(),
    );
    decision.task_intent_uid = Some(root_uid.clone());
    decision.task_intent_cycle_uid = Some(root_uid.clone());
    super::normalize_task_intents(std::slice::from_mut(&mut root));
    let make_plan_decision = |decision| {
        (
            root.feature_uid.clone().unwrap(),
            crate::core::lifecycle::GateReceipt {
                receipt_uid: uuid::Uuid::new_v4().to_string(),
                subject_uid: uuid::Uuid::new_v4().to_string(),
                subject_sha256: uuid::Uuid::new_v4().simple().to_string().repeat(2),
                actor: crate::core::lifecycle::GateActor::User,
                decision,
                recorded_at_unix_ms: 1,
                evidence_refs: Vec::new(),
                prompt_sha256: None,
                prompt_manifest: None,
                reviewer_run_id: None,
                author_run_id: None,
                reviewer_session_id: None,
                author_session_id: None,
            },
        )
    };
    let plan_decisions = [
        make_plan_decision(crate::core::lifecycle::GateDecision::Approved),
        make_plan_decision(crate::core::lifecycle::GateDecision::Rejected),
        make_plan_decision(crate::core::lifecycle::GateDecision::ChangesRequested),
    ];

    assert!(capture_authoritative_events(
        std::slice::from_mut(&mut root),
        std::slice::from_ref(&decision),
        &[],
        &plan_decisions,
        &[],
        &[],
    ));
    save(&repo, &[root.clone()]).unwrap();
    let persisted = load(&repo).unwrap();
    let history = &persisted[0].task_intent_history;
    let decision_event = history
        .iter()
        .find(|event| event.kind == TaskIntentEventKindV1::DecisionRecorded)
        .unwrap();
    assert_eq!(decision_event.cycle_uid.as_deref(), Some(root_uid.as_str()));
    assert!(matches!(
        &decision_event.evidence,
        Some(
            crate::core::planning_work::TaskIntentEventEvidenceV1::Decision {
                status: crate::domain::ItemStatus::Open,
                ..
            }
        )
    ));
    let decision_events = history
        .iter()
        .filter(|event| event.kind == TaskIntentEventKindV1::PlanDecisionRecorded)
        .collect::<Vec<_>>();
    assert_eq!(decision_events.len(), 3);
    for (event, (_, receipt)) in decision_events.iter().zip(plan_decisions.iter()) {
        assert!(matches!(
            &event.evidence,
            Some(crate::core::planning_work::TaskIntentEventEvidenceV1::PlanDecision {
                receipt: actual,
                ..
            }) if actual == receipt
        ));
    }

    let mut malformed_history = persisted[0].clone();
    let Some(crate::core::planning_work::TaskIntentEventEvidenceV1::PlanDecision {
        receipt, ..
    }) = malformed_history
        .task_intent_history
        .iter_mut()
        .find(|event| event.kind == TaskIntentEventKindV1::PlanDecisionRecorded)
        .and_then(|event| event.evidence.as_mut())
    else {
        panic!("plan decision event should carry typed provenance");
    };
    receipt.subject_sha256 = "b".repeat(64);
    assert!(save(&repo, &[malformed_history]).is_err());

    let mut cross_feature_history = persisted[0].clone();
    let Some(crate::core::planning_work::TaskIntentEventEvidenceV1::PlanDecision {
        feature_uid,
        ..
    }) = cross_feature_history
        .task_intent_history
        .iter_mut()
        .find(|event| event.kind == TaskIntentEventKindV1::PlanDecisionRecorded)
        .and_then(|event| event.evidence.as_mut())
    else {
        panic!("plan decision event should carry typed provenance");
    };
    *feature_uid = uuid::Uuid::new_v4().to_string();
    assert!(save(&repo, &[cross_feature_history]).is_err());

    let mut persisted_root = persisted[0].clone();
    assert!(!capture_authoritative_events(
        std::slice::from_mut(&mut persisted_root),
        std::slice::from_ref(&decision),
        &[],
        &plan_decisions,
        &[],
        &[],
    ));
    decision.status = crate::domain::ItemStatus::Resolved;
    assert!(capture_authoritative_events(
        std::slice::from_mut(&mut persisted_root),
        std::slice::from_ref(&decision),
        &[],
        &plan_decisions,
        &[],
        &[],
    ));
    assert_eq!(
        persisted_root
            .task_intent_history
            .iter()
            .filter(|event| event.kind == TaskIntentEventKindV1::DecisionRecorded)
            .count(),
        2
    );
    std::fs::remove_dir_all(repo).unwrap();
}
