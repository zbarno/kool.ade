use super::*;

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

#[test]
fn persists_idempotent_questions_and_blocked_readiness() {
    use crate::{
        artifacts::planning_store::{PlanningStore, StoreMode},
        core::planning::QuestionStore,
    };
    let root = std::env::temp_dir().join(format!("planner-pass-{}", uuid::Uuid::new_v4()));
    let feature_uid = uuid::Uuid::new_v4().hyphenated().to_string();
    let store = PlanningStore::new(uuid::Uuid::new_v4(), &root, StoreMode::ManagedLocal);
    let spec_revision = "a".repeat(64);
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
    let _ = std::fs::remove_dir_all(root);
}
