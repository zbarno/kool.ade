use super::*;
#[path = "task_details/simplified.rs"]
mod simplified;

#[path = "task_details/proactive_attention.rs"]
mod proactive_attention;

#[path = "task_details/full_state.rs"]
mod full_state;

#[test]
fn task_details_show_independent_check_result_separately_from_koolade_verification() {
    let _environment = crate::test_support::lock_process_environment();
    let mut app = fixture();
    let key = ".koolade-packet/planning/tasks/fixture/001-task.md";
    if let Screen::Connected(project) = &mut app.screen {
        let mut identity = crate::domain::ArtifactIdentity::new("TASK-1", "First task");
        identity.parent_uid = Some(uuid::Uuid::new_v4().to_string());
        let targets = crate::core::workflow::BranchTargets {
            source: "release/2.1".into(),
            destination: "integration".into(),
        };
        let metadata = crate::artifacts::task_docs::TaskMetadata::new(&identity, "root", vec![])
            .unwrap()
            .with_branch_targets(Some(&targets))
            .unwrap();
        project.task_documents[0].metadata = Some(metadata.clone());
        project.task_documents[0].task_state = Some(crate::artifacts::task_docs::TaskState {
            batch_uid: metadata.batch_uid,
            repository_id: "root".into(),
            dependency_uids: Vec::new(),
            status: crate::core::planning_work::WorkStatus::InReview,
            execution_status: Some("awaiting_review".into()),
            source_branch: None,
            source_sha: None,
            destination_branch: None,
            implementation_branch: None,
            publication_status: crate::artifacts::task_docs::ExecutionPublicationStatus::Planned,
            pull_requests: Vec::new(),
            legacy_linkage: true,
            revision: 4,
        });
        project.implementation_states.insert(
            key.into(),
            crate::core::implementation::Implementation {
                ticket: key.into(),
                task_uid: None,
                workspace_uid: None,
                ticket_text: "# First task".into(),
                approved_specification: None,
                approved_product_context: None,
                completed_dependency_context: None,
                manager_repair_context: None,
                branch: "koolade/fixture".into(),
                source_branch: Some("release/2.1".into()),
                destination_branch: Some("integration".into()),
                source_ref: Some("release/2.1".into()),
                source_commit: Some("fixture-base".into()),
                repository_id: Some("root".into()),
                project_id: None,
                repository_identity: None,
                push_repository: None,
                repository_cache: None,
                task_repository_allocation_key: None,
                base: "main".into(),
                base_commit: "fixture-base".into(),
                task_repository: std::path::PathBuf::from("/tmp/koolade-fixture"),
                task_repository_kind:
                    crate::core::implementation::TaskRepositoryKind::LegacyWorktree,
                task_repository_ready: false,
                task_repositories: vec![std::path::PathBuf::from("/tmp/koolade-fixture")],
                task_repository_commits: std::collections::BTreeMap::new(),
                status: crate::core::implementation::ImplementationStatus::Completed,
                detail: "Locally verified.".into(),
                pr_url: None,
                verified_head: Some("0123456789abcdef".into()),
                auto_merge: false,
                merged_commit: Some("0123456789abcdef".into()),
                pr_state: None,
                pr_details: None,
                pr_checked_at: None,
                pr_check_attempted_at: None,
                pr_check_error: None,
                independent_check: Some(crate::core::implementation::IndependentCheck {
                    provider: "GitHub Actions".into(),
                    commit: "0123456789abcdef".into(),
                    candidate_ref: "refs/heads/koolade/checks/task/0123456789abcdef".into(),
                    status: crate::core::implementation::IndependentCheckStatus::Passed,
                    checked_at: None,
                    detail: Some("All project workflows passed for this exact commit.".into()),
                }),
                cleanup: Default::default(),
            },
        );
    }
    let ctx = egui::Context::default();
    frame(&mut app, &ctx, vec![]);
    click_text(&mut app, &ctx, "First task");
    let output = click_text(&mut app, &ctx, "Why am I seeing this?");
    for expected in [
        "In review",
        "GitHub Actions · Passed",
        "commit 0123456789ab",
        "All project workflows passed for this exact commit.",
    ] {
        assert!(
            text_position(&output, expected).is_some(),
            "missing {expected}"
        );
    }
    let current_state = text_position(&output, "CURRENT STATE").unwrap();
    assert!(
        output.shapes.iter().any(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.text() == "In review" => {
                let position = text.pos + text.galley.mesh_bounds.center().to_vec2();
                position.x >= current_state.x
                    && position.y > current_state.y
                    && position.y < current_state.y + 90.0
            }
            _ => false,
        }),
        "the task detail modal should show the normalized task status"
    );
    let size = egui::vec2(1800.0, 1400.0);
    for _ in 0..3 {
        frame_at(&mut app, &ctx, vec![], size);
    }
    for section in ["Technical details", "Branch intent"] {
        click_text_at(&mut app, &ctx, section, size);
        frame_at(
            &mut app,
            &ctx,
            vec![
                egui::Event::PointerMoved(egui::pos2(400.0, 700.0)),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, -600.0),
                    phase: egui::TouchPhase::Move,
                    modifiers: Default::default(),
                },
            ],
            size,
        );
        for _ in 0..3 {
            frame_at(&mut app, &ctx, vec![], size);
        }
    }
    let output = frame_at(&mut app, &ctx, vec![], size);
    assert!(
        text_position(&output, "Source branch: release/2.1").is_some(),
        "visible: {:?}",
        output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Text(text) => Some(text.galley.text()),
                _ => None,
            })
            .collect::<Vec<_>>()
    );
    assert!(text_position(&output, "Destination branch: integration").is_some());
}

#[test]
fn task_details_offer_open_options_in_the_action_pane() {
    let _environment = crate::test_support::lock_process_environment();
    let mut app = fixture();
    let key = ".koolade-packet/planning/tasks/fixture/001-task.md";
    if let Screen::Connected(p) = &mut app.screen {
        p.task_chats.messages.insert(key.into(), vec![ChatMessage::new(
            ChatRole::Agent,
            "Ready.\n\n---\n- Which approach?\n- Yes, use the existing adapter.\n- No, replace the adapter.",
            Some(key.into()),
        )]);
    }
    let ctx = egui::Context::default();
    frame(&mut app, &ctx, vec![]);
    let output = click_text(&mut app, &ctx, "First task");
    for label in [
        "Which approach?",
        "Choose one",
        "Yes, use the existing adapter.",
        "No, replace the adapter.",
        "Send",
    ] {
        assert!(text_position(&output, label).is_some(), "missing {label}");
    }
    click_text(&mut app, &ctx, "Yes, use the existing adapter.");
    if let Screen::Connected(p) = &app.screen {
        assert_eq!(
            p.task_chats.drafts.get(key).map(String::as_str),
            Some("Yes, use the existing adapter.")
        );
    }
}

#[test]
fn generated_attention_brief_explains_an_unseen_blocker_and_sends_its_choice() {
    let _environment = crate::test_support::lock_process_environment();
    let mut app = fixture();
    let key = ".koolade-packet/planning/tasks/fixture/001-task.md";
    if let Screen::Connected(p) = &mut app.screen {
        p.queue.blocked.insert(key.into(), crate::core::implementation::Failure::other("## Waiting for user action\n\nA provider quota stopped the job.\n\n### Next action(s)\n\n- Account owner: choose (a) wait or (b) request more capacity.\n\nFull report: report.json"));
    }
    app.attention_fixture.insert(key.into(), crate::core::attention::Brief {
        problem: "The provider has reached its daily request limit, so the job cannot continue today.".into(),
        recommendation: Some(crate::core::attention::Recommendation {
            option_id: "a".into(),
            rationale: "Waiting avoids account changes and extra charges; the report says capacity returns tomorrow.".into(),
        }),
        options: vec![
            crate::core::attention::OptionBrief { id: "a".into(), label: "Wait for reset".into(),
                meaning: "Use the existing quota after it refreshes.".into(),
                consequence: "There is no account change, but the task remains paused until tomorrow.".into(), clarification: false, source_evidence: None },
            crate::core::attention::OptionBrief { id: "b".into(), label: "Request higher quota".into(),
                meaning: "Ask the provider to raise the account limit.".into(),
                consequence: "This may require account approval or added cost; the task remains paused until capacity is granted.".into(), clarification: false, source_evidence: None },
        ],
        steps: vec![crate::core::attention::HumanStep {
            owner: "Account owner".into(), action: "Choose how to get more capacity.".into(),
        }],
        after: "Kool.ad/e can retry once capacity is available.".into(),
    });
    let ctx = egui::Context::default();
    frame(&mut app, &ctx, vec![]);
    let output = click_text(&mut app, &ctx, "First task");
    for label in [
        "The provider has reached its daily request limit, so the job cannot continue today.",
        "Wait for reset",
        "Request higher quota",
        "I'm not sure",
        "Kool.ad/e can retry once capacity is available.",
    ] {
        assert!(text_position(&output, label).is_some(), "missing {label}");
    }
    assert!(
        text_position(&output, "Send decision").is_none(),
        "cards submit directly"
    );
    if let Screen::Connected(project) = &mut app.screen {
        project
            .task_chats
            .drafts
            .insert(key.into(), "What happens next?".into());
    }
    click_text(&mut app, &ctx, "Request higher quota");
    if let Screen::Connected(project) = &app.screen {
        assert_eq!(
            project.task_chats.drafts.get(key).map(String::as_str),
            Some("What happens next?")
        );
    }
    if let Screen::Connected(p) = &app.screen {
        assert!(
            p.task_turns.is_empty(),
            "decision should not start a planner turn"
        );
        assert_eq!(
            p.task_chats.drafts.get(key).map(String::as_str),
            Some("What happens next?")
        );
        assert!(
            p.task_chats.messages[key]
                .iter()
                .any(|m| m.role == ChatRole::User && m.text.contains("option (b)"))
        );
        let mut saved = crate::persistence::task_chats::TaskChats::default();
        saved.ensure_loaded(&p.chat_slug);
        assert!(
            saved.messages[key]
                .iter()
                .any(|m| m.text.contains("option (b)"))
        );
    }
}
