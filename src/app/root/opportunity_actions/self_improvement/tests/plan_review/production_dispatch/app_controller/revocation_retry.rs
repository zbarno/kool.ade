use crate::app::root::Screen;

#[path = "revocation_retry/feature_cancel.rs"]
mod feature_cancel;

/// User cancellation must persist its failed revocation warning and retry it
/// before a stale worker authorization can be reused.
#[test]
fn duplicate_cancel_retries_failed_self_improvement_revocation() {
    let temp = tempfile::tempdir_in("/tmp").unwrap();
    let test_tmp = temp.path().join("tmp");
    std::fs::create_dir_all(&test_tmp).unwrap();
    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    environment.set("TMPDIR", &test_tmp);
    environment.set("KOOLADE_HOME", temp.path().join("operator-home"));
    let gh = super::fake_gh(temp.path());
    let mut ready = super::super::setup::ready_task(&gh);
    let ticket = ready.ticket.clone();
    let run_uid = ready.run.run_uid.clone();
    let feature_uid = ready.run.feature_uid.clone();
    let planning = ready.planning.clone();
    let lifecycle = crate::core::lifecycle::LifecycleStore::new(&planning);
    let task = lifecycle
        .task_for_planning_item(&ready.metadata.uid)
        .unwrap()
        .expect("fixture task is linked to the generated planning item");
    assert_eq!(task.feature_uid, feature_uid);

    // Simulate a preexisting event id collision so the first cancel fails only
    // at the Evolve revocation write, after the app's cancellation is saved.
    let event_uid = uuid::Uuid::new_v5(
        &uuid::Uuid::parse_str(&run_uid).unwrap(),
        format!("user-cancelled-task:{}", task.task_uid).as_bytes(),
    )
    .hyphenated()
    .to_string();
    let event_path = format!("state/self_improvement_events/{run_uid}/{event_uid}.json");
    let conflicting_event = crate::core::self_improvement::SelfImprovementTransitionV1 {
        schema_version: 1,
        uid: event_uid,
        run_uid: run_uid.clone(),
        created_at_ms: 1,
        updated_at_ms: 1,
        revision: 1,
        from_phase: crate::core::self_improvement::SelfImprovementPhaseV1::ApprovedToPlan,
        to_phase: crate::core::self_improvement::SelfImprovementPhaseV1::Planning,
        event: Some(crate::core::self_improvement::SelfImprovementEventV1::BeginPlanning),
        opportunity_decision_uid: None,
    };
    planning
        .transaction(
            &[(
                event_path.clone(),
                serde_json::to_vec_pretty(&conflicting_event).unwrap(),
            )],
            None,
        )
        .unwrap();
    if let Screen::Connected(project) = &mut ready._app.screen {
        project.state.baseline_planning_revision = planning.revision().unwrap();
    }

    ready
        ._app
        .dispatch_ui_command(crate::ui::ApplicationCommand::CancelWork {
            key: ticket.clone(),
        });

    let runs = crate::core::self_improvement::SelfImprovementStore::new(&planning);
    assert_eq!(
        runs.load(&run_uid).unwrap().phase,
        crate::core::self_improvement::SelfImprovementPhaseV1::Ready,
        "conflicting event identity leaves authorization active on the first attempt"
    );
    let warning_prefix = format!(
        "Work is cancelled, but self-improvement authorization for {ticket} could not be revoked:"
    );
    {
        let Screen::Connected(project) = &ready._app.screen else {
            panic!("project disconnected")
        };
        assert!(project.task_cancelled(&ticket));
        assert!(
            project
                .activity
                .pending
                .iter()
                .any(|warning| warning.starts_with(&warning_prefix)),
            "failed authorization revocation must leave a retryable warning"
        );
    }

    planning
        .transaction_with_removals(&[], &[event_path], None)
        .unwrap();
    ready
        ._app
        .dispatch_ui_command(crate::ui::ApplicationCommand::CancelWork {
            key: ticket.clone(),
        });
    assert_eq!(
        runs.load(&run_uid).unwrap().phase,
        crate::core::self_improvement::SelfImprovementPhaseV1::Cancelled,
        "duplicate cancellation retries the pending authorization revocation"
    );
    let Screen::Connected(project) = &ready._app.screen else {
        panic!("project disconnected")
    };
    assert!(project.task_cancelled(&ticket));
    assert_eq!(
        project.state.baseline_planning_revision,
        planning.revision().unwrap(),
        "successful Evolve retry advances the app planning baseline"
    );
    assert!(
        project
            .activity
            .pending
            .iter()
            .all(|warning| !warning.starts_with(&warning_prefix)),
        "successful retry removes the stale revocation warning"
    );
}
