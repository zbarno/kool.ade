use super::*;

fn record(task_uid: &str, decision_uid: &str, revision: u64, text: &str) -> TaskDecisionRecordV1 {
    TaskDecisionRecordV1 {
        schema_version: 1,
        decision_uid: decision_uid.into(),
        task_uid: task_uid.into(),
        task_intent_uid: "00000000-0000-4000-8000-000000000001".into(),
        task_intent_cycle_uid: None,
        feature_uid: "00000000-0000-4000-8000-000000000002".into(),
        task_spec_sha256: "a".repeat(64),
        approved_plan_sha256: "b".repeat(64),
        source_message_id: format!("message-{revision}"),
        source_context_sha256: "c".repeat(64),
        source: TaskDecisionSourceV1::ExplicitImplementationDirection,
        authority: crate::core::lifecycle::GateActor::User,
        authorization_receipt: TaskDecisionAuthorityReceiptV1 {
            schema_version: 1,
            receipt_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
            task_uid: task_uid.into(),
            task_intent_uid: "00000000-0000-4000-8000-000000000001".into(),
            feature_uid: "00000000-0000-4000-8000-000000000002".into(),
            task_spec_sha256: "a".repeat(64),
            approved_plan_sha256: "b".repeat(64),
            source_context_sha256: "c".repeat(64),
            source_message_id: format!("message-{revision}"),
            subject_sha256: format!("{:x}", sha2::Sha256::digest(text.as_bytes())),
            recorded_at_unix_ms: revision,
        },
        affected_requirement_ids: vec![format!("REQ-{revision}")],
        created_at_unix_ms: revision,
        created_revision: revision,
        status: TaskDecisionStatusV1::Active,
        text: text.into(),
    }
}

#[test]
fn context_includes_every_active_direction_and_ignores_superseded_or_other_tasks() {
    let task = "00000000-0000-4000-8000-000000000003";
    let other = "00000000-0000-4000-8000-000000000004";
    let mut old = record(task, "00000000-0000-4000-8000-000000000005", 1, "old");
    old.status = TaskDecisionStatusV1::Superseded;
    let records = [
        old,
        record(
            task,
            "00000000-0000-4000-8000-000000000006",
            2,
            "independent one",
        ),
        record(
            task,
            "00000000-0000-4000-8000-000000000007",
            3,
            "independent two",
        ),
        record(
            other,
            "00000000-0000-4000-8000-000000000008",
            4,
            "unrelated",
        ),
    ];
    let context = task_decision_context(
        &records,
        task,
        "00000000-0000-4000-8000-000000000001",
        None,
        "00000000-0000-4000-8000-000000000002",
        &"a".repeat(64),
        &"b".repeat(64),
    )
    .unwrap()
    .unwrap();
    assert!(context.contains("independent one"));
    assert!(context.contains("independent two"));
    assert!(!context.contains("old"));
    assert!(!context.contains("unrelated"));
}

#[test]
fn stale_plan_binding_fails_closed() {
    let task = "00000000-0000-4000-8000-000000000003";
    let record = record(task, "00000000-0000-4000-8000-000000000005", 1, "direction");
    assert!(
        task_decision_context(
            &[record],
            task,
            "00000000-0000-4000-8000-000000000001",
            None,
            "00000000-0000-4000-8000-000000000002",
            &"a".repeat(64),
            &"d".repeat(64),
        )
        .is_err()
    );
}

#[test]
fn serialized_decisions_reconstruct_the_same_worker_context() {
    let task = "00000000-0000-4000-8000-000000000003";
    let records = vec![record(
        task,
        "00000000-0000-4000-8000-000000000005",
        1,
        "preserve the independent constraint",
    )];
    let restored: Vec<TaskDecisionRecordV1> =
        serde_json::from_str(&serde_json::to_string(&records).unwrap()).unwrap();
    let render = |records: &[TaskDecisionRecordV1]| {
        task_decision_context(
            records,
            task,
            "00000000-0000-4000-8000-000000000001",
            None,
            "00000000-0000-4000-8000-000000000002",
            &"a".repeat(64),
            &"b".repeat(64),
        )
        .unwrap()
    };
    assert_eq!(render(&records), render(&restored));
}

#[test]
// Production boundary: private PlanningStore record writes, serialization, and reopen.
// This proves superseding and independent answers survive a process restart; the context
// formatter unit test cannot establish durable store behavior. Uses only a temporary local
// store, with no network, Docker, or provider dependency and minimal setup cost.
fn local_store_restart_preserves_replacements_and_independent_answers() {
    use crate::artifacts::planning_store::{PlanningStore, StoreMode};
    use crate::core::planning_work::{Work, WorkKind, load, save};

    let directory = tempfile::tempdir().unwrap();
    let intent_uid = "00000000-0000-4000-8000-000000000001";
    let feature_uid = "00000000-0000-4000-8000-000000000002";
    let task_uid = "00000000-0000-4000-8000-000000000003";
    let store = PlanningStore::new(
        uuid::Uuid::new_v4(),
        directory.path(),
        StoreMode::ManagedLocal,
    );
    let mut intent = Work::new(
        "task:root".into(),
        "Request".into(),
        "Goal".into(),
        String::new(),
    );
    intent.uid = intent_uid.into();
    intent.kind = WorkKind::Feature;
    intent.task_intent_uid = Some(intent_uid.into());
    intent.feature_uid = Some(feature_uid.into());
    save(&store, &[intent]).unwrap();

    let first = record(
        task_uid,
        "00000000-0000-4000-8000-000000000005",
        1,
        "first answer",
    );
    save_task_decision(&store, intent_uid, first).unwrap();
    let replacement = record(
        task_uid,
        "00000000-0000-4000-8000-000000000006",
        2,
        "replacement answer",
    );
    save_task_decision(&store, intent_uid, replacement).unwrap();
    let mut independent = record(
        task_uid,
        "00000000-0000-4000-8000-000000000007",
        3,
        "independent answer",
    );
    independent.source_context_sha256 = "d".repeat(64);
    independent.authorization_receipt.source_context_sha256 = "d".repeat(64);
    save_task_decision(&store, intent_uid, independent).unwrap();

    let reopened = PlanningStore::new(store.project_id, directory.path(), StoreMode::ManagedLocal);
    let work = load(&reopened).unwrap();
    let root = work.iter().find(|item| item.uid == intent_uid).unwrap();
    assert_eq!(
        root.task_decisions[0].status,
        TaskDecisionStatusV1::Superseded
    );
    let context = task_decision_context(
        &root.task_decisions,
        task_uid,
        intent_uid,
        None,
        feature_uid,
        &"a".repeat(64),
        &"b".repeat(64),
    )
    .unwrap()
    .unwrap();
    assert!(context.contains("replacement answer"));
    assert!(context.contains("independent answer"));
    assert!(!context.contains("first answer"));
}
