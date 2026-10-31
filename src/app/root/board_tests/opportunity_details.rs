use super::*;
use crate::core::opportunity::{
    EvidenceConfidentialityV1, OpportunityEvidenceRefV1, OpportunityKindV1, OpportunityOriginV1,
    OpportunityScopeV1, OpportunityStatusV1, OpportunityTargetKindV1, OpportunityTargetV1,
    OpportunityV1,
};

fn proposal() -> OpportunityV1 {
    OpportunityV1 {
        schema_version: 1,
        opportunity_uid: uuid::Uuid::new_v4().to_string(),
        workspace_uid: uuid::Uuid::new_v4().to_string(),
        origin: OpportunityOriginV1::QaFinding,
        kind: OpportunityKindV1::Quality,
        target: OpportunityTargetV1 {
            kind: OpportunityTargetKindV1::ActiveUserProject,
            repository_uid: uuid::Uuid::new_v4().to_string(),
            canonical_remote: "https://github.com/fixture/repo".into(),
            source_sha: "a".repeat(40),
        },
        related_feature_uids: Vec::new(),
        related_task_uids: Vec::new(),
        discovered_at_ms: 1,
        observation_revision: 1,
        evidence_refs: vec![OpportunityEvidenceRefV1 {
            evidence_uid: uuid::Uuid::new_v4().to_string(),
            sha256: "b".repeat(64),
            confidentiality: EvidenceConfidentialityV1::UserLocal,
        }],
        problem_or_opportunity: "Improve planning reliability".into(),
        user_value: "Plans remain consistent".into(),
        expected_outcome: "A reviewed feature is available".into(),
        possible_approaches: vec!["Use current planning records".into()],
        recommended_approach: "Use current planning records".into(),
        alternatives: vec!["Defer the change".into()],
        estimated_scope: OpportunityScopeV1::Small,
        impact: "Clearer planning".into(),
        cost_or_risk: "Ordinary product risk".into(),
        affected_systems: vec!["planning".into()],
        current_capability_state: "The existing flow lacks a review summary".into(),
        generic_workaround: Some("Review the source records manually".into()),
        proposed_approach: "Create a reviewed feature".into(),
        dedup_fingerprint: "c".repeat(64),
        status: OpportunityStatusV1::AwaitingDecision,
        revision: 1,
    }
}

#[test]
fn proposal_opens_the_universal_details_modal_with_its_own_discussion() {
    let mut app = fixture();
    let proposal = proposal();
    let uid = proposal.opportunity_uid.clone();
    if let Screen::Connected(project) = &mut app.screen {
        crate::core::opportunity::OpportunityStore::new(&project.state.planning_store)
            .save_advisory(proposal)
            .unwrap();
    }

    let ctx = egui::Context::default();
    let output = click_text(&mut app, &ctx, "Improvement proposals (1)");
    assert!(text_position(&output, "Improve planning reliability").is_some());
    assert!(text_position(&output, "Pursue").is_none());
    assert!(text_position(&output, "A reviewed feature is available").is_none());

    click_text(&mut app, &ctx, "Improve planning reliability");
    let output = click_text(&mut app, &ctx, "Target and evidence");
    for expected in [
        "Ask a question",
        "Task details & state",
        "Grounded trigger",
        "Expected benefit",
        "Expected outcome",
        "Scope, approach, and alternatives",
        "Pursue",
        "Not now · 7 days",
        "Decline",
        "Discuss",
        "User-local evidence reference; source content remains in this workspace",
        "Discussion is advisory",
    ] {
        assert!(
            text_contains(&output, expected),
            "missing {expected:?} in opportunity details"
        );
    }
    let conversation = text_position(&output, "Ask a question").unwrap();
    let details = text_position(&output, "Task details & state").unwrap();
    assert!(
        conversation.x < details.x,
        "conversation should be the left pane"
    );
    assert_eq!(
        ctx.data_mut(|data| data.get_temp::<String>(egui::Id::new("koolade_selected_planning")))
            .as_deref(),
        Some(format!("opportunity:{uid}").as_str())
    );

    frame(
        &mut app,
        &ctx,
        vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Default::default(),
        }],
    );
    frame(&mut app, &ctx, vec![]);
    assert!(
        ctx.data_mut(|data| data.get_temp::<String>(egui::Id::new("koolade_selected_planning")))
            .is_none()
    );
}
