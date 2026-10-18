use super::*;

#[path = "tests/invalid_follow_up.rs"]
mod invalid_follow_up;

fn planner_output(feature_uid: &str) -> crate::harness::prompt_catalog::PlannerOutputV2 {
    use crate::harness::prompt_catalog::*;
    PlannerOutputV2 {
        proposed_questions: vec![ClarificationProposalV1 {
            decision_subject: "Data retention".into(),
            decision_domain: "Privacy".into(),
            origin_requirement_ids: vec!["REQ-1".into()],
            category: ClarificationCategory::Privacy,
            priority: ClarificationPriority::Blocking,
            question: "How long should data be retained?".into(),
            reason: "Retention affects privacy obligations.".into(),
            options: vec![
                ClarificationOptionV1 {
                    id: "short".into(),
                    label: "Short period".into(),
                    tradeoffs: "Less history.".into(),
                },
                ClarificationOptionV1 {
                    id: "long".into(),
                    label: "Long period".into(),
                    tradeoffs: "More retained data.".into(),
                },
            ],
            recommended_option_id: None,
            no_answer_impact: "Retention policy remains unresolved.".into(),
            evidence_refs: vec![format!("feature:{feature_uid}")],
            assumption_if_deferred: None,
            depends_on_question_ids: vec![],
        }],
        resolved_questions: vec![],
        draft_changes: vec![],
        readiness: ReadinessV2 {
            ready: true,
            blockers: vec!["Retention policy remains unresolved.".into()],
            evidence: vec![format!("feature:{feature_uid}")],
            assessed_axes: vec![
                "Who".into(),
                "What".into(),
                "When".into(),
                "Where".into(),
                "Why".into(),
                "How".into(),
                "OutOfScope".into(),
                "CriticalPath".into(),
                "SystemBoundaries".into(),
                "FailureBehavior".into(),
                "DataOwnership".into(),
                "Authorization".into(),
                "MeasurableSuccess".into(),
                "TestCases".into(),
            ],
            acceptance_tests: vec![AcceptanceTestRefV1 {
                requirement_id: "REQ-1".into(),
                test_ref: "tests/retention.rs".into(),
            }],
        },
    }
}

fn install_feature(store: &crate::artifacts::planning_store::PlanningStore) -> (String, String) {
    let draft = String::from(
        "# CHG-997: Clarification readiness\n\n**Status:** Ready\n\n## Intent\n\nTest complete requirement traceability.\n\n## Current Behavior\n\nRetention behavior is unspecified.\n\n## Desired Behavior\n\nRetention is configurable and auditable.\n\n## Scope\n\n- Configure retention.\n\n## Requirements\n\n- REQ-1: Retention must be configurable.\n- REQ-2: Retention changes must be auditable.\n\n## Acceptance Criteria\n\n- AC-001: Retention has an audit trail.\n",
    );
    let identified = crate::domain::ArtifactIdentity::preserve_markdown(
        &draft,
        None,
        "CHG-997",
        "Clarification readiness",
    )
    .unwrap();
    let identity = crate::domain::ArtifactIdentity::from_markdown(&identified)
        .unwrap()
        .unwrap();
    let feature = crate::domain::ChangeMetadata::write_markdown(
        &identified,
        &identity,
        crate::domain::ChangeStatus::Ready,
    )
    .unwrap();
    store
        .transaction(
            &[(
                "planning/changes/CHG-997-clarification-readiness/specification.md".into(),
                feature.as_bytes().to_vec(),
            )],
            None,
        )
        .unwrap();
    let contract = crate::core::workflow::feature_contract(&feature);
    let revision = crate::core::lifecycle::PlanSnapshotV1::from_feature(
        "CHG-997", &identity, &feature, &contract,
    )
    .unwrap()
    .spec_revision;
    (identity.uid, revision)
}

#[test]
// Production boundary: planner output is reconciled with the persisted feature spec
// and durable question/readiness records, including requirements without question cards.
// Unit tests cannot prove the persisted cross-record reconciliation and supersession.
// Dependencies: temporary managed-local planning store; no services.
// Setup cost: low, with synthetic feature artifacts.
fn persists_idempotent_questions_and_blocked_readiness() {
    use crate::{
        artifacts::planning_store::{PlanningStore, StoreMode},
        core::planning::QuestionStore,
    };
    let root = std::env::temp_dir().join(format!("planner-pass-{}", uuid::Uuid::new_v4()));
    let store = PlanningStore::new(uuid::Uuid::new_v4(), &root, StoreMode::ManagedLocal);
    let (feature_uid, spec_revision) = install_feature(&store);
    let result = persist(
        &store,
        &feature_uid,
        1,
        &spec_revision,
        planner_output(&feature_uid),
    )
    .unwrap();
    assert!(matches!(
        result,
        crate::core::planning::ReadinessResult::NeedsWork { .. }
    ));
    persist(
        &store,
        &feature_uid,
        1,
        &spec_revision,
        planner_output(&feature_uid),
    )
    .unwrap();
    let questions = QuestionStore::new(&store);
    assert_eq!(questions.list().unwrap().len(), 1);
    let (checklist, _) = questions.load_readiness(&feature_uid).unwrap().unwrap();
    assert!(!checklist.ready);
    assert_eq!(
        checklist.spec_revision.as_deref(),
        Some(spec_revision.as_str())
    );
    assert_eq!(
        checklist.blocker_notes,
        ["Retention policy remains unresolved."]
    );
    assert_eq!(checklist.evidence_refs, [format!("feature:{feature_uid}")]);
    assert_eq!(
        checklist.required_requirement_ids.as_deref(),
        Some(["REQ-1".to_owned(), "REQ-2".to_owned()].as_slice())
    );
    assert!(matches!(
        persist(
            &store,
            &feature_uid,
            2,
            &spec_revision,
            planner_output(&feature_uid),
        ),
        Ok(crate::core::planning::ReadinessResult::NeedsWork {
            untraced_requirements,
            ..
        }) if untraced_requirements == ["REQ-2"]
    ));
    let retired = questions
        .list()
        .unwrap()
        .into_iter()
        .find(|question| question.plan_revision == 1)
        .unwrap();
    assert_eq!(
        retired.status,
        crate::core::planning::QuestionStatus::Superseded
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
// Production boundary: repeated planner passes update durable progress and eventually
// report a stalled plan instead of hiding the no-progress budget in memory.
// Unit tests cannot prove the behavior survives the actual planning-store path.
// Dependencies: temporary managed-local planning store; no services.
// Setup cost: low, with synthetic feature artifacts.
fn repeated_persisted_planner_passes_stall_after_the_configured_no_progress_budget() {
    use crate::{
        artifacts::planning_store::{PlanningStore, StoreMode},
        core::planning::{QuestionStore, ReadinessResult},
    };
    let root = std::env::temp_dir().join(format!("planner-stall-{}", uuid::Uuid::new_v4()));
    let store = PlanningStore::new(uuid::Uuid::new_v4(), &root, StoreMode::ManagedLocal);
    let (feature_uid, spec_revision) = install_feature(&store);

    let mut outcomes = Vec::new();
    for _ in 0..4 {
        outcomes.push(
            persist(
                &store,
                &feature_uid,
                1,
                &spec_revision,
                planner_output(&feature_uid),
            )
            .unwrap(),
        );
    }

    assert!(matches!(
        outcomes.last(),
        Some(ReadinessResult::PlanningStalled { .. })
    ));
    let questions = QuestionStore::new(&store);
    let (checklist, _) = questions.load_readiness(&feature_uid).unwrap().unwrap();
    assert!(checklist.stalled);
    assert_eq!(checklist.no_progress_count, 3);
    assert_eq!(questions.list().unwrap().len(), 1);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
// Production boundary: a validated, evidence-backed technical resolution must
// retire the attention card and persist its rationale in the question record.
// Unit tests cannot prove it commits with the planner readiness transaction.
// Dependencies: temporary managed-local planning store; no services.
// Setup cost: low, with synthetic feature and repository evidence.
fn planner_records_evidence_resolution_without_claiming_a_user_answer() {
    use crate::{
        artifacts::planning_store::{PlanningStore, StoreMode},
        core::planning::{QuestionStatus, QuestionStore},
        harness::prompt_catalog::TechnicalResolutionV1,
    };
    let root = std::env::temp_dir().join(format!("planner-resolution-{}", uuid::Uuid::new_v4()));
    let store = PlanningStore::new(uuid::Uuid::new_v4(), &root, StoreMode::ManagedLocal);
    let (feature_uid, spec_revision) = install_feature(&store);
    let mut first_output = planner_output(&feature_uid);
    first_output.proposed_questions[0].category =
        crate::harness::prompt_catalog::ClarificationCategory::Api;
    first_output.proposed_questions[0].decision_subject = "Retention endpoint contract".into();
    first_output.proposed_questions[0].decision_domain = "API response format".into();
    first_output.proposed_questions[0].question =
        "Which response shape should the retention endpoint use?".into();
    persist(&store, &feature_uid, 1, &spec_revision, first_output).unwrap();
    let questions = QuestionStore::new(&store);
    let question = questions.list().unwrap().remove(0);
    let mut output = planner_output(&feature_uid);
    output.proposed_questions.clear();
    output.resolved_questions = vec![TechnicalResolutionV1 {
        question_uid: question.uid.clone(),
        decision: "Keep the existing JSON response envelope".into(),
        rationale: "The repository README and shallow manifest evidence show this as the established API boundary.".into(),
        selected_option_id: None,
        evidence_refs: vec![format!("repository-overview:{feature_uid}")],
    }];
    persist(&store, &feature_uid, 1, &spec_revision, output).unwrap();
    let (resolved, _) = questions.load(&question.uid).unwrap();
    assert_eq!(resolved.status, QuestionStatus::EvidenceResolved);
    assert_eq!(
        resolved.answered_by.as_deref(),
        Some("Man.ager (repository evidence)")
    );
    assert!(
        resolved
            .answer_text
            .as_deref()
            .unwrap()
            .contains("Rationale:")
    );
    assert!(
        resolved
            .evidence_refs
            .iter()
            .any(|item| item == &format!("repository-overview:{feature_uid}"))
    );
    let _ = std::fs::remove_dir_all(root);
}
