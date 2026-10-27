use super::*;

pub(super) fn fake_route() -> crate::harness::HarnessRouteProvenance {
    crate::harness::HarnessRouteProvenance {
        provider_id: "synthetic-provider".into(),
        role: crate::execution::contract::HarnessRole::PlanningAnalyst,
        work_category: Some(crate::persistence::harness_settings::PLANNING.into()),
        selected_model: Some("synthetic-model".into()),
        requested_model: Some("synthetic-model".into()),
        capability_probe_id: Some("synthetic-probe".into()),
        capability_generation: Some(1),
    }
}

pub(super) struct FakeTurnHarness {
    pub(super) route: crate::harness::HarnessRouteProvenance,
    pub(super) response: String,
    pub(super) entered: Option<std::sync::Arc<std::sync::Barrier>>,
    pub(super) release: Option<std::sync::Arc<std::sync::Barrier>>,
}

impl crate::harness::AiHarness for FakeTurnHarness {
    fn label(&self) -> String {
        "synthetic setup provider".into()
    }

    fn route_provenance(
        &self,
        _request: &crate::harness::PlanningRequest,
    ) -> Option<crate::harness::HarnessRouteProvenance> {
        Some(self.route.clone())
    }

    fn check_available(&self) -> Result<String, crate::error::AppError> {
        Ok("synthetic".into())
    }

    fn execute(
        &self,
        _request: &crate::harness::PlanningRequest,
    ) -> Result<crate::harness::HarnessOutcome, crate::error::AppError> {
        if let Some(entered) = &self.entered {
            entered.wait();
        }
        if let Some(release) = &self.release {
            release.wait();
        }
        Ok(crate::harness::HarnessOutcome {
            final_text: self.response.clone(),
            envelope: None,
            stderr_tail: String::new(),
        })
    }
}

pub(super) fn fake_harness() -> Box<dyn crate::harness::AiHarness> {
    Box::new(FakeTurnHarness {
        route: fake_route(),
        response: r#"{"schemaVersion":1,"reply":"explain_observation"}"#.into(),
        entered: None,
        release: None,
    })
}

pub(super) fn complete_turns(controller: &mut Controller) -> Vec<super::controller::CompletedTurn> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    let mut completed_turns = Vec::new();
    loop {
        completed_turns.extend(controller.take_completed());
        if controller.active_requests().is_empty() {
            return completed_turns;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "fake provider did not finish"
        );
        std::thread::yield_now();
    }
}

#[test]
fn different_setup_tasks_keep_parallel_machine_local_conversations() {
    let directory = tempfile::tempdir().unwrap();
    crate::persistence::bootstrap_conversations::BootstrapConversationStore::with_test_directory(
        directory.path(),
        || {
            let host = crate::app::setup_attention::catalog::HostKind::current();
            let first = task(
                "setup:docker:rootless",
                crate::persistence::setup_tasks::State::Failed,
            );
            let second = task(
                "setup:cli:authenticate",
                crate::persistence::setup_tasks::State::Detected,
            );
            let mut controller = Controller::default();
            *controller.draft_mut(&first.id).unwrap() = "Why is Docker blocked?".into();
            *controller.draft_mut(&second.id).unwrap() = "How do I check the CLI route?".into();
            controller.start(&first, host, fake_harness()).unwrap();
            controller.start(&second, host, fake_harness()).unwrap();
            assert!(controller.view(&first.id).busy);
            assert!(controller.view(&second.id).busy);
            assert_eq!(controller.active_requests().len(), 2);

            for completed in complete_turns(&mut controller) {
                let current = if completed.task_id() == first.id {
                    &first
                } else {
                    &second
                };
                controller.finish(completed, Some(current), Some(fake_route()));
            }
            let first_history = controller.view(&first.id).messages;
            let second_history = controller.view(&second.id).messages;
            assert_eq!(first_history.len(), 2);
            assert_eq!(second_history.len(), 2);
            assert!(first_history[0].text.contains("Docker"));
            assert!(second_history[0].text.contains("CLI"));
            assert_eq!(first_history[1].role, crate::domain::ChatRole::Agent);
            assert_eq!(second_history[1].role, crate::domain::ChatRole::Agent);
            assert_eq!(
                first_history[1].text,
                "The setup task's current observation is: Synthetic setup observation."
            );
        },
    );
}

#[test]
fn cancelling_an_in_flight_turn_discards_its_late_provider_response() {
    let directory = tempfile::tempdir().unwrap();
    crate::persistence::bootstrap_conversations::BootstrapConversationStore::with_test_directory(
        directory.path(),
        || {
            let current = task(
                "setup:docker:rootless",
                crate::persistence::setup_tasks::State::Failed,
            );
            let entered = std::sync::Arc::new(std::sync::Barrier::new(2));
            let release = std::sync::Arc::new(std::sync::Barrier::new(2));
            let mut controller = Controller::default();
            *controller.draft_mut(&current.id).unwrap() = "Explain the current blocker.".into();
            controller
                .start(
                    &current,
                    crate::app::setup_attention::catalog::HostKind::current(),
                    Box::new(FakeTurnHarness {
                        route: fake_route(),
                        response: r#"{"schemaVersion":1,"reply":"explain_observation"}"#.into(),
                        entered: Some(entered.clone()),
                        release: Some(release.clone()),
                    }),
                )
                .unwrap();
            entered.wait();
            controller.cancel(&current.id);
            release.wait();
            let completed = complete_turns(&mut controller).pop().unwrap();
            controller.finish(completed, Some(&current), Some(fake_route()));
            let view = controller.view(&current.id);
            assert!(
                !view
                    .messages
                    .iter()
                    .any(|message| message.role == crate::domain::ChatRole::Agent)
            );
            assert!(view.messages.last().unwrap().text.contains("cancelled"));
        },
    );
}

#[test]
fn route_changes_and_stale_task_facts_discard_provider_responses() {
    let directory = tempfile::tempdir().unwrap();
    crate::persistence::bootstrap_conversations::BootstrapConversationStore::with_test_directory(
        directory.path(),
        || {
            let host = crate::app::setup_attention::catalog::HostKind::current();
            let original = task(
                "setup:docker:rootless",
                crate::persistence::setup_tasks::State::Failed,
            );
            let entered = std::sync::Arc::new(std::sync::Barrier::new(2));
            let release = std::sync::Arc::new(std::sync::Barrier::new(2));
            let mut controller = Controller::default();
            *controller.draft_mut(&original.id).unwrap() = "Explain this setup task.".into();
            controller
                .start(
                    &original,
                    host,
                    Box::new(FakeTurnHarness {
                        route: fake_route(),
                        response: r#"{"schemaVersion":1,"reply":"repeat_next_step"}"#.into(),
                        entered: Some(entered.clone()),
                        release: Some(release.clone()),
                    }),
                )
                .unwrap();
            entered.wait();
            let mut changed_task = original.clone();
            changed_task.title.push_str(" changed");
            changed_task.scan_generation += 1;
            release.wait();
            let completed = complete_turns(&mut controller).pop().unwrap();
            controller.finish(completed, Some(&changed_task), Some(fake_route()));
            let stale_view = controller.view(&original.id);
            assert!(
                !stale_view
                    .messages
                    .iter()
                    .any(|message| message.role == crate::domain::ChatRole::Agent)
            );
            assert!(
                stale_view
                    .error
                    .as_deref()
                    .unwrap()
                    .contains("facts changed")
            );

            let route_task = task(
                "setup:cli:authenticate",
                crate::persistence::setup_tasks::State::Detected,
            );
            let route_entered = std::sync::Arc::new(std::sync::Barrier::new(2));
            let route_release = std::sync::Arc::new(std::sync::Barrier::new(2));
            *controller.draft_mut(&route_task.id).unwrap() = "Explain the CLI setup step.".into();
            controller
                .start(
                    &route_task,
                    host,
                    Box::new(FakeTurnHarness {
                        route: fake_route(),
                        response: r#"{"schemaVersion":1,"reply":"explain_observation"}"#.into(),
                        entered: Some(route_entered.clone()),
                        release: Some(route_release.clone()),
                    }),
                )
                .unwrap();
            route_entered.wait();
            let mut switched_route = fake_route();
            switched_route.selected_model = Some("different-model".into());
            assert!(!controller.active_route_matches(&route_task.id, Some(&switched_route)));
            controller.cancel(&route_task.id);
            route_release.wait();
            let completed = complete_turns(&mut controller).pop().unwrap();
            controller.finish(completed, Some(&route_task), Some(switched_route));
            let route_view = controller.view(&route_task.id);
            assert!(
                !route_view
                    .messages
                    .iter()
                    .any(|message| message.role == crate::domain::ChatRole::Agent)
            );
            assert!(
                route_view
                    .error
                    .as_deref()
                    .unwrap()
                    .contains("assistant or verified model changed")
            );
        },
    );
}

#[test]
fn restart_load_adds_a_notice_without_resuming_the_previous_turn() {
    let directory = tempfile::tempdir().unwrap();
    crate::persistence::bootstrap_conversations::BootstrapConversationStore::with_test_directory(
        directory.path(),
        || {
            let task_id = "setup:docker:rootless";
            crate::persistence::bootstrap_conversations::BootstrapConversationStore::append(
                task_id,
                &[crate::domain::ChatMessage::new(
                    crate::domain::ChatRole::User,
                    "Explain the setup blocker.",
                    None,
                )],
            )
            .unwrap();
            let mut controller = Controller::default();
            let view = controller.view(task_id);
            assert!(!view.busy);
            assert_eq!(view.messages.len(), 2);
            assert_eq!(
                view.messages.last().unwrap().role,
                crate::domain::ChatRole::System
            );
            assert!(
                view.messages
                    .last()
                    .unwrap()
                    .text
                    .contains("will not resume")
            );
        },
    );
}
