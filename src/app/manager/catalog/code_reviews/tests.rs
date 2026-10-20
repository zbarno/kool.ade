use super::*;

fn target() -> (String, FindingTarget) {
    let evidence_id = "code-review-finding:abc123".to_owned();
    let target = FindingTarget {
        task_uid: "00000000-0000-4000-8000-000000000001".into(),
        feature_uid: "00000000-0000-4000-8000-000000000002".into(),
        subject_hash: "a".repeat(64),
        finding_id: "finding-1".into(),
        finding_fingerprint: "c".repeat(64),
        scope: crate::harness::prompt_catalog::FindingScopeV1::Blocking,
    };
    (evidence_id, target)
}

fn action(
    evidence_id: &str,
    target: &FindingTarget,
) -> crate::harness::prompt_catalog::ManagerActionV1 {
    crate::harness::prompt_catalog::ManagerActionV1 {
        kind: crate::harness::prompt_catalog::ManagerActionKind::RepairFinding,
        target: format!("{}#{}", target.task_uid, target.finding_id),
        reason: "The reviewed change misses the required behavior.".into(),
        evidence: vec![evidence_id.into()],
        expected_revision: target.subject_hash.clone(),
        contradiction: None,
    }
}

#[test]
fn code_review_dispositions_bind_one_action_to_the_current_subject_and_finding() {
    let (evidence_id, target) = target();
    let targets = [(evidence_id.clone(), target.clone())].into();
    let evidence = [evidence_id.clone()].into();
    assert!(validate_dispositions(&[action(&evidence_id, &target)], &targets, &evidence).is_ok());
    assert!(validate_dispositions(&[], &targets, &evidence).is_err());

    let mut stale = action(&evidence_id, &target);
    stale.expected_revision = "b".repeat(64);
    assert!(validate_dispositions(&[stale], &targets, &evidence).is_err());

    let duplicate = action(&evidence_id, &target);
    assert!(validate_dispositions(&[duplicate.clone(), duplicate], &targets, &evidence).is_err());
}

#[test]
fn concurrent_task_findings_remain_bound_to_their_own_dispositions() {
    let (first_evidence, first) = target();
    let second_evidence = "code-review-finding:def456".to_owned();
    let mut second = first.clone();
    second.task_uid = "00000000-0000-4000-8000-000000000003".into();
    second.feature_uid = "00000000-0000-4000-8000-000000000004".into();
    second.subject_hash = "b".repeat(64);
    second.finding_id = "finding-2".into();
    let targets = [
        (first_evidence.clone(), first.clone()),
        (second_evidence.clone(), second.clone()),
    ]
    .into();
    let available_evidence = [first_evidence.clone(), second_evidence.clone()].into();
    let actions = [
        action(&first_evidence, &first),
        action(&second_evidence, &second),
    ];

    assert!(validate_dispositions(&actions, &targets, &available_evidence).is_ok());
    let mut cross_bound = actions;
    cross_bound[0].target = format!("{}#{}", second.task_uid, second.finding_id);
    cross_bound[0].expected_revision = second.subject_hash;
    assert!(validate_dispositions(&cross_bound, &targets, &available_evidence).is_err());
}

#[test]
fn only_nonblocking_findings_can_be_promoted_as_follow_ups() {
    let (evidence_id, mut target) = target();
    let targets = [(evidence_id.clone(), target.clone())].into();
    let evidence = [evidence_id.clone()].into();
    let mut promote = action(&evidence_id, &target);
    promote.kind = crate::harness::prompt_catalog::ManagerActionKind::PromoteFollowUp;
    assert!(validate_dispositions(&[promote.clone()], &targets, &evidence).is_err());

    target.scope = crate::harness::prompt_catalog::FindingScopeV1::NonBlocking;
    let targets = [(evidence_id.clone(), target.clone())].into();
    promote.target = format!("{}#{}", target.task_uid, target.finding_id);
    promote.expected_revision = target.subject_hash.clone();
    assert!(validate_dispositions(&[promote], &targets, &evidence).is_ok());

    for kind in [
        crate::harness::prompt_catalog::ManagerActionKind::RepairFinding,
        crate::harness::prompt_catalog::ManagerActionKind::AskUser,
    ] {
        let mut invalid = action(&evidence_id, &target);
        invalid.kind = kind;
        assert!(validate_dispositions(&[invalid], &targets, &evidence).is_err());
    }
}

#[test]
fn manager_can_request_user_risk_decision_only_for_nonblocking_findings() {
    let (evidence_id, mut target) = target();
    target.scope = crate::harness::prompt_catalog::FindingScopeV1::NonBlocking;
    let targets = [(evidence_id.clone(), target.clone())].into();
    let evidence = [evidence_id.clone()].into();
    let mut request = action(&evidence_id, &target);
    request.kind = crate::harness::prompt_catalog::ManagerActionKind::AcceptRisk;
    assert!(validate_dispositions(&[request.clone()], &targets, &evidence).is_ok());
    target.scope = crate::harness::prompt_catalog::FindingScopeV1::Blocking;
    let targets = [(evidence_id.clone(), target)].into();
    assert!(validate_dispositions(&[request], &targets, &evidence).is_err());
}
