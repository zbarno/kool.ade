use crate::app::root::KooladeApp;

#[path = "tests/task_fixture.rs"]
mod task_fixture;

impl KooladeApp {
    pub(crate) fn seed_trace_navigation_fixture(
        &mut self,
    ) -> (
        String,
        crate::core::lifecycle::SpecDeltaRecordV1,
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        String,
    ) {
        let fixture = task_fixture::seed(self);
        (
            fixture.feature_uid,
            fixture.delta,
            fixture.task_uid,
            fixture.decision_uid,
            fixture.test_ref,
            fixture.evidence_ref,
            fixture.review_uid,
            fixture.qa_uid,
            fixture.pr_ref,
            fixture.event_uid,
            fixture.followup_uid,
            fixture.operator_uid,
        )
    }
}

#[test]
fn navigation_dispatch_selects_the_resolved_persisted_task() {
    let mut app = crate::app::root::board_tests::fixture();
    let (feature_uid, delta, task_uid, _, _, _, _, _, _, _, _, _) =
        app.seed_trace_navigation_fixture();
    app.dispatch_ui_command(crate::ui::ApplicationCommand::OpenSpecificationTrace {
        feature_uid: feature_uid.clone(),
        kind: crate::core::lifecycle::SpecTraceReferenceKindV1::Task,
        reference: task_uid.clone(),
    });

    let selected = crate::ui::Surface::specification_trace_navigation(&app)
        .expect("dispatcher opens the selected trace record");
    assert_eq!(
        selected.target_kind,
        crate::core::lifecycle::SpecTraceTargetKindV1::Task
    );
    assert_eq!(selected.target_uid, task_uid);
    assert_eq!(selected.feature_uid, feature_uid);
    assert!(selected.detail.contains("Phase Ready"));
    assert_eq!(
        selected.requirement_traces[0].requirement_id,
        delta.requirement_id
    );
}

#[test]
fn persisted_trace_reference_categories_open_their_typed_targets() {
    use crate::core::lifecycle::{
        SpecTraceReferenceKindV1 as Ref, SpecTraceTargetKindV1 as Target,
    };
    let mut app = crate::app::root::board_tests::fixture();
    let (
        feature_uid,
        _,
        task_uid,
        decision_uid,
        test_ref,
        evidence_ref,
        review_uid,
        qa_uid,
        pr_ref,
        event_uid,
        followup_uid,
        operator_uid,
    ) = app.seed_trace_navigation_fixture();
    let cases = [
        (Ref::Task, task_uid.as_str(), Target::Task),
        (
            Ref::DecisionOrReceipt,
            decision_uid.as_str(),
            Target::Decision,
        ),
        (Ref::Test, test_ref.as_str(), Target::TestReference),
        (
            Ref::Evidence,
            evidence_ref.as_str(),
            Target::EvidenceReference,
        ),
        (
            Ref::DecisionOrReceipt,
            review_uid.as_str(),
            Target::CodeReview,
        ),
        (Ref::DecisionOrReceipt, qa_uid.as_str(), Target::RuntimeQa),
        (
            Ref::PullRequestState,
            pr_ref.as_str(),
            Target::PullRequestState,
        ),
        (
            Ref::LifecycleEvent,
            event_uid.as_str(),
            Target::LifecycleEvent,
        ),
        (Ref::FollowupTask, followup_uid.as_str(), Target::Task),
        (
            Ref::OperatorDecision,
            operator_uid.as_str(),
            Target::OperatorDecision,
        ),
        // Decision/receipt links in the selected trace pane use this shared kind.
        (
            Ref::DecisionOrReceipt,
            operator_uid.as_str(),
            Target::OperatorDecision,
        ),
    ];
    for (kind, reference, expected) in cases {
        app.dispatch_ui_command(crate::ui::ApplicationCommand::OpenSpecificationTrace {
            feature_uid: feature_uid.clone(),
            kind,
            reference: reference.to_owned(),
        });
        let selected = crate::ui::Surface::specification_trace_navigation(&app)
            .expect("typed resolver selects a persisted target");
        assert_eq!(selected.target_uid, reference, "target {reference}");
        assert_eq!(selected.target_kind, expected, "target {reference}");
        assert_eq!(selected.feature_uid, feature_uid);
    }
}
