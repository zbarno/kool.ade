use koolade::core::lifecycle::GateReceipt;
use koolade::runtime_qa::QaApplicabilityAssessmentV1;
use sha2::Digest as _;

pub(super) fn assert_legacy_proposal_rejected(
    lifecycle: &koolade::core::lifecycle::LifecycleStore<'_>,
    task_uid: &str,
    task_revision: u64,
    assessment: QaApplicabilityAssessmentV1,
    manager_receipt: &mut GateReceipt,
) {
    let mut legacy_reviewer_receipt = super::super::role_receipt(
        koolade::harness::prompt_catalog::PromptRole::RuntimeQaReviewerV2,
        koolade::core::lifecycle::GateDecision::Approved,
        task_uid,
        &"a".repeat(64),
    );
    let proposal = applicability_proposal(
        task_revision,
        assessment,
        &mut legacy_reviewer_receipt,
        manager_receipt,
    );
    assert!(
        lifecycle
            .record_runtime_qa_applicability(
                task_uid,
                task_revision,
                &uuid::Uuid::new_v4().hyphenated().to_string(),
                proposal,
                manager_receipt.clone(),
            )
            .is_err()
    );
}

pub(super) fn applicability_proposal(
    task_revision: u64,
    assessment: QaApplicabilityAssessmentV1,
    reviewer_receipt: &mut GateReceipt,
    manager_receipt: &mut GateReceipt,
) -> koolade::runtime_qa::QaApplicabilityProposalV1 {
    let assessment_hash = format!(
        "{:x}",
        sha2::Sha256::digest(serde_json::to_vec(&assessment).unwrap())
    );
    reviewer_receipt.subject_uid = assessment.task_uid.clone();
    reviewer_receipt.subject_sha256 = assessment_hash.clone();
    reviewer_receipt.reviewer_run_id = Some(assessment.reviewer_execution_id.clone());
    reviewer_receipt.reviewer_session_id = Some(assessment.reviewer_session_id.clone());
    reviewer_receipt.author_run_id = Some(assessment.approved_by.clone());
    reviewer_receipt.author_session_id = Some(assessment.manager_session_id.clone());
    reviewer_receipt.evidence_refs = vec![format!(
        "runtime-qa-applicability-assessment:{assessment_hash}"
    )];
    let target_evidence_id = format!("headless-target:{}", assessment.target_id);
    let proposal = koolade::runtime_qa::QaApplicabilityProposalV1 {
        schema_version: 1,
        task_revision,
        assessment,
        reviewer_verdict: koolade::harness::prompt_catalog::ReviewVerdictV1 {
            subject_hash: assessment_hash.clone(),
            examined_files: Vec::new(),
            tested_evidence_ids: Vec::new(),
            unexamined_areas: Vec::new(),
            pass: true,
            outcome: Some(koolade::harness::prompt_catalog::ReviewOutcomeV1::Pass),
            coverage: vec![koolade::harness::prompt_catalog::CoverageV1 {
                requirement: "VISUAL_QA_APPLICABILITY".into(),
                status: koolade::harness::prompt_catalog::CoverageStatus::Pass,
                evidence: vec![target_evidence_id],
            }],
            findings: Vec::new(),
            untested: Vec::new(),
        },
        reviewer_receipt: reviewer_receipt.clone(),
    };
    let proposal_hash = proposal.proposal_sha256().unwrap();
    manager_receipt.evidence_refs = vec![
        format!("runtime-qa-applicability:{assessment_hash}"),
        format!("gate-receipt:{}", reviewer_receipt.receipt_uid),
        format!("runtime-qa-applicability-proposal:{proposal_hash}"),
    ];
    proposal
}
