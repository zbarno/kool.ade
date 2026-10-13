use super::*;

#[path = "tests/store.rs"]
mod persistence;

fn proposal() -> OpportunityV1 {
    OpportunityV1 {
        schema_version: 1,
        opportunity_uid: uuid::Uuid::new_v4().to_string(),
        workspace_uid: uuid::Uuid::new_v4().to_string(),
        origin: OpportunityOriginV1::RunFailure,
        kind: OpportunityKindV1::BugFix,
        target: OpportunityTargetV1 {
            kind: OpportunityTargetKindV1::ActiveUserProject,
            repository_uid: uuid::Uuid::new_v4().to_string(),
            canonical_remote: "https://github.com/example/project".into(),
            source_sha: "a".repeat(64),
        },
        related_feature_uids: vec![],
        related_task_uids: vec![],
        discovered_at_ms: 1,
        observation_revision: 1,
        evidence_refs: vec![OpportunityEvidenceRefV1 {
            evidence_uid: uuid::Uuid::new_v4().to_string(),
            sha256: "b".repeat(64),
            confidentiality: EvidenceConfidentialityV1::UserLocal,
        }],
        problem_or_opportunity: "Repeated provider discovery failures".into(),
        user_value: "Avoid repeated setup delays".into(),
        expected_outcome: "Provider discovery recovers after transient failure".into(),
        possible_approaches: vec!["Bounded retry".into()],
        recommended_approach: "Use bounded retry with persisted status".into(),
        alternatives: vec!["Require manual restart".into()],
        estimated_scope: OpportunityScopeV1::Small,
        impact: "Reduces interruptions".into(),
        cost_or_risk: "Adds a bounded retry path".into(),
        affected_systems: vec!["provider discovery".into()],
        current_capability_state: "Failures are not retried".into(),
        generic_workaround: None,
        proposed_approach: "Retry transient failures once".into(),
        dedup_fingerprint: "c".repeat(64),
        status: OpportunityStatusV1::AwaitingDecision,
        revision: 1,
    }
}

#[test]
fn proposal_requires_concrete_target_and_evidence_hashes() {
    let mut value = proposal();
    assert!(value.validate().is_ok());
    value.target.source_sha = "a".repeat(40);
    assert!(value.validate().is_ok());
    value.target.source_sha = "b".repeat(64);
    assert!(value.validate().is_ok());
    value.target.source_sha = "unknown".into();
    assert!(value.validate().is_err());
}

#[test]
fn product_proposal_must_use_canonical_repository() {
    let mut value = proposal();
    value.target.kind = OpportunityTargetKindV1::KooladeProduct;
    assert!(value.validate().is_err());
    value.target.canonical_remote = "https://github.com/zbarno/kool.ade".into();
    assert!(value.validate().is_ok());
}

#[test]
fn decision_must_match_current_proposal_revision_and_wait_for_user() {
    let value = proposal();
    let decision = OpportunityDecisionV1 {
        schema_version: 1,
        uid: uuid::Uuid::new_v4().to_string(),
        opportunity_uid: value.opportunity_uid.clone(),
        opportunity_revision_hash: value.revision_hash().unwrap(),
        user_id: "synthetic-user".into(),
        decision: OpportunityDecisionKindV1::Pursue,
        occurred_at_ms: 2,
        snooze_until_ms: None,
        explanation: None,
    };
    assert!(decision.validate(&value).is_ok());
    let mut changed = value.clone();
    changed.revision += 1;
    assert!(decision.validate(&changed).is_err());
}

#[test]
fn deferred_decision_expires_at_its_persisted_snooze_deadline() {
    let mut value = proposal();
    value.status = OpportunityStatusV1::Deferred;
    let decision = OpportunityDecisionV1 {
        schema_version: 1,
        uid: uuid::Uuid::new_v4().hyphenated().to_string(),
        opportunity_uid: value.opportunity_uid.clone(),
        opportunity_revision_hash: value.revision_hash().unwrap(),
        user_id: "synthetic-user".into(),
        decision: OpportunityDecisionKindV1::NotNow,
        occurred_at_ms: 100,
        snooze_until_ms: Some(200),
        explanation: None,
    };
    assert!(decision.validate(&value).is_ok());
    assert!(!snooze_expired(&decision, 199));
    assert!(snooze_expired(&decision, 200));
    let mut invalid = decision;
    invalid.snooze_until_ms = Some(100);
    assert!(invalid.validate(&value).is_err());
}

#[test]
fn fingerprint_normalizes_whitespace_and_case_per_workspace_and_target() {
    let workspace = uuid::Uuid::new_v4().to_string();
    let target = uuid::Uuid::new_v4().to_string();
    assert_eq!(
        dedup_fingerprint(&workspace, &target, "  Provider   RETRY ").unwrap(),
        dedup_fingerprint(&workspace, &target, "provider retry").unwrap()
    );
    assert_ne!(
        dedup_fingerprint(
            &workspace,
            &uuid::Uuid::new_v4().to_string(),
            "provider retry"
        )
        .unwrap(),
        dedup_fingerprint(&workspace, &target, "provider retry").unwrap()
    );
}
