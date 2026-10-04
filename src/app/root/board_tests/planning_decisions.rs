use super::*;

#[test]
fn planning_items_use_board_and_modal_even_before_tasks_exist() {
    let mut app = fixture();
    let item = OpenItem::new(
        "CLR-010".into(),
        crate::domain::item::Priority::High,
        crate::domain::item::ItemKind::Question,
        "General".into(),
        Some("All".into()),
        "Which users need access?".into(),
        "Determines the access model".into(),
    );
    if let Screen::Connected(project) = &mut app.screen {
        project.task_documents.clear();
        project.state.items = vec![item.clone()];
    }
    let ctx = egui::Context::default();
    frame(&mut app, &ctx, vec![]);
    let output = frame(&mut app, &ctx, vec![]);
    assert!(text_position(&output, "Needs attention · 1").is_some());
    assert!(text_position(&output, "Your answer needed").is_some());
    assert!(text_position(&output, "Determines the access model").is_none());
    let pos = text_position(&output, &item.question).unwrap();
    for pressed in [true, false] {
        frame(
            &mut app,
            &ctx,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                },
            ],
        );
    }
    let output = frame(&mut app, &ctx, vec![]);
    assert!(text_position(&output, "Determines the access model").is_some());
    assert!(text_position(&output, "Owner: All").is_none());
    assert!(text_position(&output, "Your answer needed").is_some());
    *app.task_draft("CLR-010").unwrap() = "Use corporate SSO".into();
    assert!(app.chat_draft().is_empty());
    assert!(
        ctx.data_mut(|d| d.get_temp::<String>(egui::Id::new("koolade_selected_planning")))
            .is_some()
    );
    if let Screen::Connected(project) = &mut app.screen {
        project.state.items.clear();
    }
    let output = frame(&mut app, &ctx, vec![]);
    assert!(text_position(&output, &item.question).is_none());
}

#[test]
fn human_decision_brief_shows_issue_specific_buttons_and_advisory_details() {
    let mut app = fixture();
    let mut item = OpenItem::new(
        "CLR-012".into(),
        crate::domain::Priority::High,
        crate::domain::ItemKind::Question,
        "Security".into(),
        Some("Security Owner".into()),
        "How long should people stay signed in before signing in again?".into(),
        "Shorter sign-ins help protect an account if a phone is lost, but may interrupt longer work.".into(),
    );
    item.decision_brief = Some(crate::domain::DecisionBrief {
        id: item.id.clone(),
        question: item.question.clone(),
        why_now: "Before launch, we need a clear rule for when people must sign in again.".into(),
        recommendation: Some(crate::domain::DecisionRecommendation {
            option_id: "short-session".into(),
            rationale: "This gives better protection if a phone is lost, while limiting how often people sign in again.".into(),
        }),
        confidence: Some(crate::domain::DecisionConfidence {
            level: crate::domain::ConfidenceLevel::Medium,
            explanation: "We know how sign-ins work today, but need your preference for the trade-off.".into(),
        }),
        options: vec![
            crate::domain::DecisionOption {
                id: "short-session".into(),
                label: "Ask people to sign in again after one hour".into(),
                summary: "People must sign in again after an hour.".into(),
                benefits: vec!["A lost phone is less likely to give someone lasting access.".into()],
                costs: vec!["People may need to sign in again during longer work.".into()],
                risks: vec!["If someone cannot sign in again, their work may be interrupted.".into()],
                consequences: vec!["The app must ask people to sign in again after an hour.".into()],
                reversibility: "We can choose a different sign-in period later.".into(),
            },
            crate::domain::DecisionOption {
                id: "persistent".into(),
                label: "Stay signed in until signing out".into(),
                summary: "People stay signed in until they sign out.".into(),
                benefits: vec!["People avoid signing in again during their work.".into()],
                costs: vec!["People must sign out themselves to end access.".into()],
                risks: vec!["Someone who gets access to a lost phone may stay signed in.".into()],
                consequences: vec!["People keep the current sign-in experience.".into()],
                reversibility: "We can add a sign-in limit later, with app changes.".into(),
            },
        ],
        benefits: vec![],
        costs: vec![],
        risks: vec![],
        ramifications: vec!["Every signed-in client follows the selected policy.".into()],
        reversibility: "We can revisit the sign-in period later.".into(),
        defer_consequence: "Without a rule, the app cannot finish its sign-in behavior for launch.".into(),
        evidence: vec!["src/auth/session.rs records the current behavior.".into()],
        adr_assessment: Some(crate::domain::AdrAssessment {
            create: false,
            title: String::new(),
            rationale: "This choice is not durable enough to need an ADR.".into(),
            revisit_when: vec![],
        }),
    });
    if let Screen::Connected(project) = &mut app.screen {
        project.task_documents.clear();
        project.state.items = vec![item.clone()];
    }
    app.cached_user = crate::domain::CurrentUser::new("Security Owner", Vec::new());
    let ctx = egui::Context::default();
    frame(&mut app, &ctx, vec![]);
    let output = frame(&mut app, &ctx, vec![]);
    for label in [
        "How long should people stay signed in before signing in again?",
        "Before launch, we need a clear rule for when people must sign in again.",
    ] {
        assert!(
            text_contains(&output, label),
            "missing {label} from the Needs Attention card"
        );
    }
    for label in [
        "Ask people to sign in again after one hour",
        "Stay signed in until signing out",
        "Your answer needed",
        "People must sign in again after an hour.",
        "If chosen: The app must ask people to sign in again after an hour.",
    ] {
        assert!(
            text_position(&output, label).is_some(),
            "missing {label}; visible text: {:?}",
            output
                .shapes
                .iter()
                .filter_map(|clipped| match &clipped.shape {
                    egui::Shape::Text(shape) => Some(shape.galley.text().to_owned()),
                    _ => None,
                })
                .collect::<Vec<_>>()
        );
    }
    click_text(&mut app, &ctx, "Ask people to sign in again after one hour");
    assert_eq!(
        app.task_draft(&item.id).map(|draft| draft.as_str()),
        Some("I choose option (short-session): Ask people to sign in again after one hour.")
    );
    if let Screen::Connected(project) = &app.screen {
        assert_eq!(
            project.state.items[0].status,
            crate::domain::ItemStatus::Open
        );
        assert!(project.active_turn.is_none());
    }

    let details = click_text(&mut app, &ctx, &item.question);
    assert!(text_position(&details, "Decision guidance").is_some());
    assert!(text_contains(
        &details,
        "Kool.ad/e recommends Ask people to sign in again after one hour"
    ));
    assert!(text_position(&details, "Decision details").is_some());
    let _ = click_text(&mut app, &ctx, "Decision details");
    let details = click_text(
        &mut app,
        &ctx,
        "Ask people to sign in again after one hour · People must sign in again after an hour.",
    );
    for label in [
        "Confidence: Medium",
        "A lost phone is less likely to give someone lasting access.",
        "If someone cannot sign in again, their work may be interrupted.",
        "Every signed-in client follows the selected policy.",
        "Without a rule, the app cannot finish its sign-in behavior for launch.",
        "src/auth/session.rs records the current behavior.",
    ] {
        assert!(
            text_contains(&details, label),
            "missing {label}; visible text: {:?}",
            details
                .shapes
                .iter()
                .filter_map(|clipped| match &clipped.shape {
                    egui::Shape::Text(shape) => Some(shape.galley.text().to_owned()),
                    _ => None,
                })
                .collect::<Vec<_>>()
        );
    }
    assert!(text_position(&details, "Approve provisional decision").is_none());
}
