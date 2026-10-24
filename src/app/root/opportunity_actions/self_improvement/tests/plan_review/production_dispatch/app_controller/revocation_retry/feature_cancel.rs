use crate::app::root::Screen;

#[path = "feature_cancel/retry.rs"]
mod retry;

fn set_implementation_state(
    project: &mut crate::app::session::Project,
    ticket: &str,
    ticket_text: &str,
    status: crate::core::implementation::ImplementationStatus,
) {
    let state = serde_json::from_value(serde_json::json!({
        "ticket": ticket,
        "ticket_text": ticket_text,
        "branch": "koolade/fixture",
        "base": "main",
        "base_commit": "fixture",
        "task_repository": "/tmp/koolade-evolve-fixture",
        "status": status,
        "detail": "",
        "pr_url": null,
        "verified_head": null
    }))
    .expect("test implementation state is valid");
    project
        .implementation_states
        .insert(ticket.to_owned(), state);
}

/// Feature-card cancellation must revoke Evolve after every child is complete,
/// while the run still awaits publication. This exercises the Board command
/// and persisted run; setup uses a fake reviewer and needs no Docker/provider.
#[test]
fn cancelling_feature_revokes_evolve_when_every_child_is_complete() {
    let temp = tempfile::tempdir_in("/tmp").unwrap();
    let test_tmp = temp.path().join("tmp");
    std::fs::create_dir_all(&test_tmp).unwrap();
    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    environment.set("TMPDIR", &test_tmp);
    environment.set("KOOLADE_HOME", temp.path().join("operator-home"));
    let gh = super::super::fake_gh(temp.path());
    let mut ready = super::super::super::setup::ready_task(&gh);
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
            let path = project
                .task_documents
                .iter()
                .find(|document| {
                    document
                        .metadata
                        .as_ref()
                        .is_some_and(|metadata| metadata.uid == planning_item_uid)
                })
                .expect("generated Evolve task is present on the Board")
                .path
                .clone();
            let document = project
                .task_documents
                .iter()
                .find(|document| document.path == path)
                .expect("generated Evolve task document is available");
            let ticket = document.path.clone();
            let ticket_text = document.text.clone();
            set_implementation_state(
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

    ready
        ._app
        .dispatch_ui_command(crate::ui::ApplicationCommand::CancelWork { key: feature_key });

    assert_eq!(
        crate::core::self_improvement::SelfImprovementStore::new(&planning)
            .load(&run_uid)
            .unwrap()
            .phase,
        crate::core::self_improvement::SelfImprovementPhaseV1::Cancelled
    );
    let Screen::Connected(project) = &ready._app.screen else {
        panic!("project disconnected")
    };
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
            crate::core::implementation::ImplementationStatus::Completed
        );
        assert!(!project.task_cancelled(&document.path));
    }
}

#[test]
fn successful_child_fallback_clears_feature_revocation_warning() {
    let temp = tempfile::tempdir_in("/tmp").unwrap();
    let test_tmp = temp.path().join("tmp");
    std::fs::create_dir_all(&test_tmp).unwrap();
    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    environment.set("TMPDIR", &test_tmp);
    environment.set("KOOLADE_HOME", temp.path().join("operator-home"));
    let gh = super::super::fake_gh(temp.path());
    let mut ready = super::super::super::setup::ready_task(&gh);
    let run_uid = ready.run.run_uid.clone();
    let feature_uid = ready.run.feature_uid.clone();
    let planning = ready.planning.clone();
    let (feature_key, feature_id) = {
        let Screen::Connected(project) = &mut ready._app.screen else {
            panic!("project disconnected")
        };
        let card = crate::core::planning_work::cards(&project.state, &project.planning_work)
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
            .expect("Evolve feature has a Board card");
        let feature_id = card.feature_id.expect("feature card has a legacy ID");
        let mut child_count = 0;
        let children = project
            .task_documents
            .iter()
            .filter(|document| {
                document
                    .text
                    .lines()
                    .any(|line| line.strip_prefix("Feature ID: ") == Some(feature_id.as_str()))
            })
            .map(|document| (document.path.clone(), document.text.clone()))
            .collect::<Vec<_>>();
        for (ticket, ticket_text) in children {
            set_implementation_state(
                project,
                &ticket,
                &ticket_text,
                crate::core::implementation::ImplementationStatus::Implementing,
            );
            child_count += 1;
        }
        assert!(child_count > 0, "feature cancellation has a child fallback");
        (card.key, feature_id)
    };

    let namespace = uuid::Uuid::parse_str(&run_uid).unwrap();
    let event_uid = uuid::Uuid::new_v5(
        &namespace,
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
        .dispatch_ui_command(crate::ui::ApplicationCommand::CancelWork { key: feature_key });

    assert_eq!(
        crate::core::self_improvement::SelfImprovementStore::new(&planning)
            .load(&run_uid)
            .unwrap()
            .phase,
        crate::core::self_improvement::SelfImprovementPhaseV1::Cancelled,
        "an unfinished child revokes the run after direct feature revocation conflicts"
    );
    let Screen::Connected(project) = &ready._app.screen else {
        panic!("project disconnected")
    };
    let warning_prefix = format!(
        "Work is cancelled, but self-improvement authorization for feature {} could not be revoked:",
        ready.run.feature_uid
    );
    assert!(
        project
            .activity
            .pending
            .iter()
            .all(|warning| !warning.starts_with(&warning_prefix)),
        "successful child fallback clears the stale feature-level warning"
    );
    assert_eq!(ready.run.feature_uid, feature_uid);
    assert!(!feature_id.is_empty());
}
