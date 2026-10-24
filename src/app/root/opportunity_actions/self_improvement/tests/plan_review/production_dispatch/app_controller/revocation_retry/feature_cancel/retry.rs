use crate::app::root::Screen;

#[test]
fn duplicate_feature_cancel_clears_warning_after_child_fallback() {
    let temp = tempfile::tempdir_in("/tmp").unwrap();
    let test_tmp = temp.path().join("tmp");
    std::fs::create_dir_all(&test_tmp).unwrap();
    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    environment.set("TMPDIR", &test_tmp);
    environment.set("KOOLADE_HOME", temp.path().join("operator-home"));
    let gh = super::super::super::fake_gh(temp.path());
    let mut ready = super::super::super::super::setup::ready_task(&gh);
    let run_uid = ready.run.run_uid.clone();
    let feature_uid = ready.run.feature_uid.clone();
    let task_uids = ready.run.task_uids.clone();
    let planning = ready.planning.clone();
    let feature_key = {
        let Screen::Connected(project) = &mut ready._app.screen else {
            panic!("project disconnected")
        };
        let lifecycle = crate::core::lifecycle::LifecycleStore::new(&planning);
        for task_uid in &task_uids {
            let task = lifecycle.load_task(task_uid).unwrap();
            let planning_item_uid = task
                .planning_item_uid
                .as_deref()
                .expect("generated Evolve task has a planning item");
            let document = project
                .task_documents
                .iter()
                .find(|document| {
                    document
                        .metadata
                        .as_ref()
                        .is_some_and(|metadata| metadata.uid == planning_item_uid)
                })
                .expect("generated task is present on the Board");
            let ticket = document.path.clone();
            let ticket_text = document.text.clone();
            super::set_implementation_state(
                project,
                &ticket,
                &ticket_text,
                crate::core::implementation::ImplementationStatus::Completed,
            );
        }
        project.state.baseline_planning_revision = planning.revision().unwrap();
        crate::core::planning_work::cards(&project.state, &project.planning_work)
            .into_iter()
            .find(|work| {
                work.feature_uid.as_deref() == Some(feature_uid.as_str())
                    && work.feature_id.is_some()
                    && !matches!(
                        work.kind,
                        crate::core::planning_work::WorkKind::TaskGeneration
                            | crate::core::planning_work::WorkKind::PlanReviewDisposition
                            | crate::core::planning_work::WorkKind::CodeReviewDisposition
                    )
            })
            .map(|work| work.key)
            .expect("Evolve feature has a Board card")
    };

    let event_uid = uuid::Uuid::new_v5(
        &uuid::Uuid::parse_str(&run_uid).unwrap(),
        format!("user-cancelled-feature:{feature_uid}").as_bytes(),
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
                event_path,
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
            key: feature_key.clone(),
        });
    assert_eq!(
        crate::core::self_improvement::SelfImprovementStore::new(&planning)
            .load(&run_uid)
            .unwrap()
            .phase,
        crate::core::self_improvement::SelfImprovementPhaseV1::Ready
    );
    let warning_prefix = format!(
        "Work is cancelled, but self-improvement authorization for feature {feature_uid} could not be revoked:"
    );
    let Screen::Connected(project) = &ready._app.screen else {
        panic!("project disconnected")
    };
    assert!(
        project
            .activity
            .pending
            .iter()
            .any(|warning| warning.starts_with(&warning_prefix)),
        "failed feature revocation leaves a retryable warning"
    );

    ready
        ._app
        .dispatch_ui_command(crate::ui::ApplicationCommand::CancelWork { key: feature_key });

    assert_eq!(
        crate::core::self_improvement::SelfImprovementStore::new(&planning)
            .load(&run_uid)
            .unwrap()
            .phase,
        crate::core::self_improvement::SelfImprovementPhaseV1::Cancelled,
        "duplicate cancellation can revoke through a child task after the feature event conflicts"
    );
    let Screen::Connected(project) = &ready._app.screen else {
        panic!("project disconnected")
    };
    assert!(
        project
            .activity
            .pending
            .iter()
            .all(|warning| !warning.starts_with(&warning_prefix)),
        "successful child fallback clears the feature-level warning"
    );
    let lifecycle = crate::core::lifecycle::LifecycleStore::new(&planning);
    for task_uid in task_uids {
        let task = lifecycle.load_task(&task_uid).unwrap();
        let planning_item_uid = task.planning_item_uid.as_deref().unwrap();
        let document = project
            .task_documents
            .iter()
            .find(|document| {
                document
                    .metadata
                    .as_ref()
                    .is_some_and(|metadata| metadata.uid == planning_item_uid)
            })
            .unwrap();
        assert_eq!(
            project.implementation_states[&document.path].status,
            crate::core::implementation::ImplementationStatus::Completed,
            "retrying feature cancellation does not recancel completed children"
        );
    }
}
