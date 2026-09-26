use super::*;

struct CannedHarness(String);

impl crate::harness::AiHarness for CannedHarness {
    fn label(&self) -> String {
        "typed-action fixture".into()
    }

    fn check_available(&self) -> Result<String, crate::error::AppError> {
        Ok(self.label())
    }

    fn execute(
        &self,
        _request: &crate::harness::PlanningRequest,
    ) -> Result<crate::harness::HarnessOutcome, crate::error::AppError> {
        Ok(crate::harness::HarnessOutcome {
            final_text: self.0.clone(),
            envelope: None,
            stderr_tail: String::new(),
        })
    }
}

fn fixture() -> (PacketApp, std::path::PathBuf) {
    let root = std::env::temp_dir().join(format!(
        "packet-typed-action-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).unwrap();
    assert!(
        std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(&root)
            .status()
            .unwrap()
            .success()
    );
    let mut app = super::super::board_tests::fixture();
    if let Screen::Connected(project) = &mut app.screen {
        project.state = crate::core::state::PlannerState::load(&root).unwrap();
        project.state.bootstrap_missing().unwrap();
        project.chat_slug = root.join("runtime").to_string_lossy().into_owned();
        project.task_documents.clear();
    }
    (app, root)
}

fn run_to_idle(app: &mut PacketApp) {
    let ctx = egui::Context::default();
    let deadline = Instant::now() + Duration::from_secs(5);
    while app.conversation_busy() {
        app.tick(0.016, &ctx);
        assert!(Instant::now() < deadline, "scripted turn did not finish");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn typed_action_returned_by_the_planner_reaches_the_live_application_gate() {
    let (mut app, root) = fixture();
    app.task_harness = Some(Box::new(CannedHarness(
        serde_json::json!({
            "assistant_message": "I'll check whether implementation can start.",
            "requested_action": {"action":"start_implementation"}
        })
        .to_string(),
    )));

    app.start_turn("Could you start the implementation?");
    run_to_idle(&mut app);

    assert!(matches!(&app.screen, Screen::Connected(project)
        if project.active_implementations.is_empty() && project.active_turn.is_none()));
    assert!(
        app.chat_messages()
            .iter()
            .any(|message| message.text.contains("There is no task batch")),
        "unexpected chat: {:?}",
        app.chat_messages()
            .iter()
            .map(|message| message.text.as_str())
            .collect::<Vec<_>>()
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn generate_request_cannot_approve_a_feature_as_a_side_effect() {
    let (mut app, root) = fixture();
    let feature_id = "CHG-017";
    let specification = format!(
        "# {feature_id}: Export run history\n\n**Status:** Ready\n\n## Intent\n\nExport the selected run history.\n"
    );
    if let Screen::Connected(project) = &mut app.screen {
        project.state.active_feature = Some((feature_id.into(), specification.clone()));
        project
            .state
            .active_features
            .push((feature_id.into(), specification.clone()));
        project.state.workflow.brief = Some(crate::core::workflow::InterviewBrief {
            feature_name: feature_id.into(),
            problem: "Operators cannot export run history.".into(),
            goal: "Save run history for review.".into(),
            target_users: "Operators".into(),
            intended_outcome: "A local export is available.".into(),
            success_criteria: vec!["The export opens.".into()],
            in_scope: vec!["Export selected run history.".into()],
            out_of_scope: vec!["Automatic uploads.".into()],
            constraints: vec!["Keep the export local.".into()],
            ready_for_tasks: true,
        });
        project.state.workflow.reviewed_specification = Some(specification);
    }

    dispatch(
        &mut app,
        RequestedAction {
            action: ApplicationAction::GenerateTasks,
            target_uid: None,
        },
    );

    assert!(matches!(&app.screen, Screen::Connected(project)
    if project.active_turn.is_none()
        && !crate::core::workflow::feature_approved(
            &project.state.repo_root,
            &project.state.workflow,
            feature_id
        )));
    assert!(
        app.chat_messages()
            .iter()
            .any(|message| { message.text.contains("Use its Approve action first") })
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn former_trigger_phrases_are_sent_to_the_model_and_do_nothing_without_a_typed_action() {
    for text in [
        "start implementing",
        "why won't you start implementing?",
        "No empieces todavía.",
    ] {
        let (mut app, root) = fixture();
        app.task_harness = Some(Box::new(CannedHarness(
            serde_json::json!({
                "assistant_message": "I understand; no application action was requested.",
                "requested_action": null
            })
            .to_string(),
        )));

        app.start_turn(text);
        assert!(
            app.conversation_busy(),
            "the planner should interpret: {text}"
        );
        run_to_idle(&mut app);
        assert!(matches!(&app.screen, Screen::Connected(project)
            if project.active_implementations.is_empty() && project.active_turn.is_none()));
        assert!(
            app.chat_messages()
                .iter()
                .any(|message| message.text.contains("no application action")),
            "unexpected chat for {text}: {:?}",
            app.chat_messages()
                .iter()
                .map(|message| message.text.as_str())
                .collect::<Vec<_>>()
        );
        let _ = std::fs::remove_dir_all(root);
    }
}
