use super::*;

#[test]
fn proposal_must_match_an_allowed_repository_identity_and_target_kind() {
    let proposal = proposal();
    assert!(
        validate_output_with_context(PromptRole::Manager, &manager_output(&proposal), &context())
            .is_ok()
    );

    let mut wrong_identity = proposal.clone();
    wrong_identity.target_repository = "https://github.com/example/other".into();
    assert!(
        validate_output_with_context(
            PromptRole::Manager,
            &manager_output(&wrong_identity),
            &context()
        )
        .is_err()
    );

    let mut wrong_kind = proposal;
    wrong_kind.target = OpportunityTarget::OtherLinkedRepository;
    assert!(
        validate_output_with_context(
            PromptRole::Manager,
            &manager_output(&wrong_kind),
            &context()
        )
        .is_err()
    );
}

#[test]
fn event_origin_requires_a_typed_application_evidence_reference() {
    let mut proposal = proposal();
    proposal.origin = OpportunityOrigin::QaFinding;
    assert!(
        validate_output_with_context(PromptRole::Manager, &manager_output(&proposal), &context())
            .is_err()
    );

    proposal
        .evidence_refs
        .push("lifecycle-event:event-123".into());
    let mut context = context();
    context
        .evidence_ids
        .insert("lifecycle-event:event-123".into());
    assert!(
        validate_output_with_context(PromptRole::Manager, &manager_output(&proposal), &context)
            .is_ok()
    );
}

#[test]
fn every_improvement_kind_and_event_origin_can_be_validated_with_grounded_evidence() {
    use super::super::schemas::{OpportunityKind, OpportunityOrigin};

    for kind in [
        OpportunityKind::NewFeature,
        OpportunityKind::ExistingFeatureExtension,
        OpportunityKind::BugFix,
        OpportunityKind::Integration,
        OpportunityKind::TechStackSupport,
        OpportunityKind::Quality,
        OpportunityKind::Security,
        OpportunityKind::Performance,
        OpportunityKind::DeveloperExperience,
        OpportunityKind::Documentation,
        OpportunityKind::Infrastructure,
    ] {
        let mut proposal = proposal();
        proposal.kind = kind;
        assert!(
            validate_output_with_context(
                PromptRole::Manager,
                &manager_output(&proposal),
                &context()
            )
            .is_ok(),
            "supported improvement kind must remain advisory and valid"
        );
    }

    let cases = [
        (OpportunityOrigin::UserGoal, None),
        (
            OpportunityOrigin::RunFailure,
            Some("implementation-failure:run-1"),
        ),
        (OpportunityOrigin::QaFinding, Some("lifecycle-event:qa-1")),
        (
            OpportunityOrigin::ReviewFinding,
            Some("lifecycle-event:review-1"),
        ),
        (OpportunityOrigin::SpecDrift, Some("lifecycle-event:spec-1")),
        (OpportunityOrigin::CodeInspection, None),
        (OpportunityOrigin::UserFeedback, None),
        (OpportunityOrigin::Roadmap, None),
        (OpportunityOrigin::ScheduledExploration, None),
    ];
    for (origin, event_reference) in cases {
        let mut proposal = proposal();
        proposal.origin = origin;
        let mut context = context();
        if let Some(reference) = event_reference {
            proposal.evidence_refs.push(reference.into());
            context.evidence_ids.insert(reference.into());
        }
        assert!(
            validate_output_with_context(PromptRole::Manager, &manager_output(&proposal), &context)
                .is_ok(),
            "supported proposal origin must validate with its required typed evidence"
        );
    }
}

#[test]
fn planner_opportunity_context_requires_user_value_and_target_repo() {
    let mut input = base(
        PromptRole::Planner,
        RolePayloadV1::PlannerOpportunity {
            open_questions: vec![],
            previous_answers: vec![],
            research_evidence: vec![],
            requested_scope: "plan the approved feature".into(),
            opportunity_origin: Some("repeated build failure".into()),
            existing_support_evidence: vec!["generic CLI recipe exists".into()],
            workaround: Some("run the recipe manually".into()),
            user_value: Some("reduce repeated setup".into()),
            confidentiality_class: EvidenceConfidentiality::WorkspacePrivate,
            concrete_target_repo: Some("project".into()),
        },
    );
    assert!(render(&input).is_ok());
    if let RolePayloadV1::PlannerOpportunity {
        concrete_target_repo,
        ..
    } = &mut input.payload
    {
        *concrete_target_repo = None;
    }
    assert!(render(&input).is_err());
}
