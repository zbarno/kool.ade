use super::*;

#[path = "task_details/proactive_attention.rs"]
mod proactive_attention;

#[test]
fn task_details_show_full_state_and_inline_reply() {
    let mut app = fixture();
    let key = ".koolade-packet/planning/tasks/fixture/001-task.md";
    if let Screen::Connected(p) = &mut app.screen {
        p.queue.blocked.insert(p.task_documents[0].path.clone(),
            crate::core::implementation::Failure::new(
                crate::core::implementation::FailureKind::ExternalPrerequisite,
                crate::core::implementation::RecoveryDisposition::UserAction,
                "## Waiting for user action\n\nThe published history conflicts with the gate.\n\n### Next action(s)\n\n- Adjudicator: approve the corrected footprint.\n- Operator: record the display demonstration.\n\nFull report: saved-report.json",
            ));
    }
    app.attention_fixture.insert(
        key.into(),
        crate::core::attention::Brief {
            problem: "The published history conflicts with the required file list.".into(),
            recommendation: None,
            options: Vec::new(),
            steps: vec![crate::core::attention::HumanStep {
                owner: "Operator".into(),
                action: "Record the display demonstration.".into(),
            }],
            after: "Resume once the required review is complete.".into(),
        },
    );
    let ctx = egui::Context::default();
    frame(&mut app, &ctx, vec![]);
    let output = click_text(&mut app, &ctx, "First task");
    for label in [
        "Task conversation",
        "Task details & state",
        "CURRENT STATE",
        "YOUR NEXT STEP",
        "Activity",
        "Resume after action",
        "Reply to this task",
        "Send response",
    ] {
        assert!(text_position(&output, label).is_some(), "missing {label}");
    }
    let conversation = text_position(&output, "Task conversation").unwrap();
    let state = text_position(&output, "Task details & state").unwrap();
    assert!(
        conversation.x < state.x,
        "conversation should be the left pane"
    );
    assert!(
        text_position(
            &output,
            "The published history conflicts with the required file list."
        )
        .is_some()
    );
    assert!(text_position(&output, "Operator: Record the display demonstration.").is_some());
    assert!(text_position(&output, "Full blocker report").is_some());
    let output = click_text(&mut app, &ctx, "Full blocker report");
    assert!(text_position(&output, "Full report: saved-report.json").is_some());
    assert!(text_position(&output, "Copy full report").is_some());
    assert!(
        output.shapes.iter().any(|shape| matches!(&shape.shape,
        egui::Shape::Text(text) if text.galley.text().contains("Full report: saved-report.json"))),
        "the complete failure text must be rendered, not shortened to a summary"
    );
}

#[test]
fn task_details_show_independent_check_result_separately_from_koolade_verification() {
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
        project.task_documents[0].metadata = Some(metadata);
        project.implementation_states.insert(
            key.into(),
            crate::core::implementation::Implementation {
                ticket: key.into(),
                task_uid: None,
                ticket_text: "# First task".into(),
                approved_specification: None,
                approved_product_context: None,
                completed_dependency_context: None,
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
    let output = click_text(&mut app, &ctx, "First task");
    for expected in [
        "Done",
        "GitHub Actions · Passed",
        "commit 0123456789ab",
        "All project workflows passed for this exact commit.",
    ] {
        assert!(
            text_position(&output, expected).is_some(),
            "missing {expected}"
        );
    }
    click_text(&mut app, &ctx, "Technical details");
    let output = click_text(&mut app, &ctx, "Branch intent");
    assert!(text_position(&output, "Source branch: release/2.1").is_some());
    assert!(text_position(&output, "Destination branch: integration").is_some());
}

#[test]
fn task_details_offer_open_options_in_the_reply_box() {
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
        "Choose an option",
        "Yes, use the existing adapter.",
        "No, replace the adapter.",
        "Send response",
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
                consequence: "There is no account change, but the task remains paused until tomorrow.".into(), source_evidence: None },
            crate::core::attention::OptionBrief { id: "b".into(), label: "Request higher quota".into(),
                meaning: "Ask the provider to raise the account limit.".into(),
                consequence: "This may require account approval or added cost; the task remains paused until capacity is granted.".into(), source_evidence: None },
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
        "Kool.ad/e recommends",
        "Wait for reset: Waiting avoids account changes and extra charges; the report says capacity returns tomorrow.",
        "Account owner: Choose how to get more capacity.",
        "Wait for reset",
        "Request higher quota",
        "If chosen: There is no account change, but the task remains paused until tomorrow.",
        "If chosen: This may require account approval or added cost; the task remains paused until capacity is granted.",
        "Kool.ad/e can retry once capacity is available.",
        "Send decision",
    ] {
        assert!(text_position(&output, label).is_some(), "missing {label}");
    }
    click_text(&mut app, &ctx, "Request higher quota");
    if let Screen::Connected(p) = &app.screen {
        assert_eq!(
            p.task_chats.drafts.get(key).map(String::as_str),
            Some("I choose option (b): Request higher quota.")
        );
    }
    let output = click_text(&mut app, &ctx, "Send decision");
    assert!(text_position(&output, "Decision saved for Kool.ad/e.").is_some());
    assert!(text_position(&output, "Change decision").is_some());
    if let Screen::Connected(p) = &app.screen {
        assert!(
            p.task_turns.is_empty(),
            "decision should not start a planner turn"
        );
        assert!(p.task_chats.drafts.get(key).is_none_or(String::is_empty));
        assert!(
            p.task_chats.messages[key]
                .last()
                .is_some_and(|m| m.role == ChatRole::User && m.text.contains("option (b)"))
        );
        let mut saved = crate::persistence::task_chats::TaskChats::default();
        saved.ensure_loaded(&p.chat_slug);
        assert!(
            saved.messages[key]
                .last()
                .is_some_and(|m| m.text.contains("option (b)"))
        );
    }
}
