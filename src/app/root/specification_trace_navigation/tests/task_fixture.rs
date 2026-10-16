use crate::core::lifecycle::{
    FeatureEvent, LifecycleStore, SpecDeltaClassificationV1, SpecDeltaOperatorChoiceV1,
    SpecDeltaOperatorDecisionV1, SpecDeltaRecordV1, SpecReconciliationStatusV1,
    SpecRevisionDraftV1,
};

#[path = "task_fixture/records.rs"]
mod records;

pub(super) struct TraceTargets {
    pub(super) feature_uid: String,
    pub(super) delta: SpecDeltaRecordV1,
    pub(super) task_uid: String,
    pub(super) decision_uid: String,
    pub(super) test_ref: String,
    pub(super) evidence_ref: String,
    pub(super) review_uid: String,
    pub(super) qa_uid: String,
    pub(super) pr_ref: String,
    pub(super) event_uid: String,
    pub(super) followup_uid: String,
    pub(super) operator_uid: String,
}

pub(super) fn seed(app: &mut crate::app::root::KooladeApp) -> TraceTargets {
    let crate::app::root::Screen::Connected(project) = &mut app.screen else {
        panic!("fixture is connected")
    };
    let store = &project.state.planning_store;
    crate::artifacts::migration::bootstrap_product_with_store(
        &project.state.repo_root,
        store,
        "Trace Navigation Fixture",
    )
    .unwrap();
    let feature_id = "CHG-901";
    let raw = format!(
        "# {feature_id}: Trace navigation fixture\n\n**Status:** Ready\n\n## Intent\n\nProve persisted reference navigation.\n\n## Current Behavior\n\nTrace references are visible.\n\n## Desired Behavior\n\nTrace references open persisted records.\n\n## Scope\n\nOne synthetic task.\n\n## Affected Product Areas\n\n`product:current-capabilities`\n\n## Requirements\n\n- TRACE-R1 (MUST). Open linked task records.\n\n## Decisions and Assumptions\n\n- **A1 (fixture):** Synthetic local identity.\n\n## Acceptance Criteria\n\n1. A task reference selects its persisted record.\n"
    );
    let path = store
        .layout()
        .change_directory(&format!("{feature_id}-trace-navigation-fixture"))
        .unwrap()
        .join("specification.md");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let markdown = crate::domain::ArtifactIdentity::preserve_markdown(
        &raw,
        None,
        feature_id,
        "Trace navigation fixture",
    )
    .unwrap();
    let identity = crate::domain::ArtifactIdentity::from_markdown(&markdown)
        .unwrap()
        .unwrap();
    let markdown = crate::domain::ChangeMetadata::write_markdown(
        &markdown,
        &identity,
        crate::domain::ChangeStatus::Ready,
    )
    .unwrap();
    std::fs::write(path, &markdown).unwrap();
    crate::core::lifecycle::test_support::approve_and_review_feature(
        store,
        &mut project.state.workflow,
        feature_id,
        &markdown,
    )
    .unwrap();

    let lifecycle = LifecycleStore::new(store);
    let feature = lifecycle.load_feature(&identity.uid).unwrap();
    let plan_hash = feature.approved_plan_sha256.clone().unwrap();
    let snapshot = crate::core::lifecycle::plan_snapshot::load_persisted(
        store,
        feature.plan_snapshot_uid.as_deref().unwrap(),
    )
    .unwrap()
    .unwrap();
    let requirement_id = snapshot.requirements[0].stable_id.clone();
    let graph_event_uid = uuid::Uuid::new_v4().hyphenated().to_string();
    lifecycle
        .apply_feature_event(
            &identity.uid,
            feature.revision,
            &graph_event_uid,
            FeatureEvent::TaskGraphCreated {
                plan_sha256: plan_hash.clone(),
            },
        )
        .unwrap();

    let task_uid = uuid::Uuid::new_v4().hyphenated().to_string();
    let work_uid = uuid::Uuid::new_v4().hyphenated().to_string();
    let implementation_hash = "a".repeat(64);
    let reviewed_head = "b".repeat(40);
    let review_subject = "c".repeat(64);
    let review = records::code_review(
        store,
        &identity.uid,
        snapshot.uid.as_str(),
        &task_uid,
        &plan_hash,
        &implementation_hash,
    );
    let qa_request = records::qa_request(
        &identity.uid,
        &task_uid,
        &requirement_id,
        &plan_hash,
        &implementation_hash,
        &reviewed_head,
        &review_subject,
        review.author_run_id.as_deref().unwrap(),
        review.author_session_id.as_deref().unwrap(),
    );
    let qa_receipt = records::qa_receipt(&task_uid, &implementation_hash);
    let task = records::task_record(
        &identity.uid,
        &task_uid,
        &requirement_id,
        &plan_hash,
        &implementation_hash,
        &reviewed_head,
        &review_subject,
        review,
        qa_receipt.clone(),
        qa_request,
        &work_uid,
    );
    lifecycle
        .create_task(&task, &uuid::Uuid::new_v4().hyphenated().to_string())
        .unwrap();

    let followup_uid = uuid::Uuid::new_v4().hyphenated().to_string();
    let followup = records::followup_task(&task, &followup_uid);
    lifecycle
        .create_task(&followup, &uuid::Uuid::new_v4().hyphenated().to_string())
        .unwrap();
    let decision_uid = records::save_decision(store, &identity.uid, &requirement_id);
    let test_ref = "tests/feature.rs".to_owned();
    records::save_readiness_test(store, &identity.uid, &test_ref);
    let pr_ref = records::save_pr_state(store, &work_uid, &identity.uid, &task_uid);
    let trigger = lifecycle
        .enqueue_manual_spec_refresh(&identity.uid, "trace-navigation-test")
        .unwrap();
    let current = lifecycle
        .load_spec_revision(&identity.uid)
        .unwrap()
        .unwrap();
    let feature = lifecycle.load_feature(&identity.uid).unwrap();
    let operator_uid = uuid::Uuid::new_v4().hyphenated().to_string();
    let evidence_ref = format!("task:{task_uid}");
    let delta = SpecDeltaRecordV1 {
        requirement_id,
        event_id: graph_event_uid.clone(),
        old_target: Some("Task details are text only".into()),
        proposed_target: Some("Task references open their persisted record".into()),
        observed_current: None,
        classification: SpecDeltaClassificationV1::Documentation,
        evidence_refs: vec![
            format!("spec-trigger:{}", trigger.uid),
            evidence_ref.clone(),
            pr_ref.clone(),
        ],
        decision_refs: vec![
            decision_uid.clone(),
            task.code_review.as_ref().unwrap().receipt_uid.clone(),
            qa_receipt.receipt_uid.clone(),
        ],
        test_refs: vec![test_ref.clone()],
        affected_task_ids: vec![task_uid.clone()],
        needs_user_reapproval: false,
        followup_ids: vec![followup_uid.clone()],
        operator_decision: Some(SpecDeltaOperatorDecisionV1 {
            receipt_uid: operator_uid.clone(),
            decision: SpecDeltaOperatorChoiceV1::Declined,
            actor: crate::core::lifecycle::GateActor::User,
            recorded_at_ms: 1,
            resulting_spec_sha256: None,
        }),
    };
    let mut decision_refs = feature.decision_refs;
    decision_refs.push(operator_uid.clone());
    decision_refs.sort();
    decision_refs.dedup();
    let authorization = crate::core::lifecycle::FeatureDeltaAuthorizationV1 {
        schema_version: 1,
        receipt_uid: operator_uid.clone(),
        feature_uid: identity.uid.clone(),
        source_spec_sequence: current.seq,
        requirement_id: delta.requirement_id.clone(),
        event_uid: delta.event_id.clone(),
        old_target_sha256: Some(format!(
            "{:x}",
            sha2::Sha256::digest("Task details are text only".as_bytes())
        )),
        proposed_target_sha256: Some(format!(
            "{:x}",
            sha2::Sha256::digest("Task references open their persisted record".as_bytes())
        )),
        resulting_product_sha256: None,
        decision: SpecDeltaOperatorChoiceV1::Declined,
        actor: crate::core::lifecycle::GateActor::User,
        recorded_at_ms: 1,
    };
    let draft = SpecRevisionDraftV1 {
        content_hash: crate::core::lifecycle::product_spec::current_product_hash(store).unwrap(),
        source_revision: feature.revision,
        produced_by_run_id: None,
        origin_events: vec![trigger.uid, graph_event_uid.clone()],
        decision_refs,
        delta_approval_refs: Vec::new(),
        approved_target_hash: feature.approved_plan_sha256,
        implementation_head_sha: None,
        reconciliation_status: SpecReconciliationStatusV1::OutstandingNonblockingFindings,
        outcome_reason: None,
        deltas: vec![delta.clone()],
    };
    let store_revision = store.revision().unwrap();
    lifecycle
        .append_spec_revision_with_feature_authorization(
            &identity.uid,
            current.seq,
            draft,
            Some(&store_revision),
            authorization,
            Vec::new(),
        )
        .unwrap();

    TraceTargets {
        feature_uid: identity.uid,
        delta,
        task_uid,
        decision_uid,
        test_ref,
        evidence_ref,
        review_uid: task.code_review.unwrap().receipt_uid,
        qa_uid: qa_receipt.receipt_uid,
        pr_ref,
        event_uid: graph_event_uid,
        followup_uid,
        operator_uid,
    }
}
use sha2::Digest as _;
