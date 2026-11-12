use super::*;

#[path = "tests/process_kill.rs"]
mod process_kill;
#[path = "tests/timeout_replay.rs"]
mod timeout_replay;

#[test]
fn completed_manager_result_is_persisted_as_activity_not_chat() {
    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    let home = std::env::temp_dir().join(format!("manager-result-home-{}", std::process::id()));
    environment.set("KOOLADE_HOME", &home);
    let mut app = crate::app::root::board_tests::fixture();
    let crate::app::root::Screen::Connected(project) = &mut app.screen else {
        unreachable!()
    };
    let legacy = ChatMessage::new(ChatRole::Agent, "Earlier Manager patrol", None);
    project.chat.push(legacy.clone());
    let mut specification = ChatMessage::new(ChatRole::Agent, "Current answer", None);
    specification.specification_context = Some(crate::domain::chatlog::SpecificationChatContext {
        scope_key: "product".into(),
        intent: "ask".into(),
        published_reference: "refs/heads/main".into(),
        published_commit_sha: "a".repeat(40),
        source_documents: Vec::new(),
        effective_document_sha256: "b".repeat(64),
        reviewed_repository_evidence: Vec::new(),
    });
    project.chat.push(specification.clone());
    project.activity.pending.clear();
    let chat_before = project.chat.clone();
    let planning_revision = project.state.baseline_planning_revision.clone();
    let output = crate::app::manager::ManagerOutput {
        summary: "Board checked; one blocked task needs attention.".into(),
        uncertainty_reported: false,
        proposed_actions: Vec::new(),
        opportunities: Vec::new(),
        evidence_hashes: Default::default(),
        evidence_confidentialities: Default::default(),
        planning_revision,
        code_review_targets: Default::default(),
        verified_repository_evidence: Vec::new(),
        baseline_review_id: None,
    };
    project.activity.manager = Some(crate::app::manager::Manager::completed_for_test(Ok(output)));
    advance(project, &mut None);
    assert_eq!(
        project.chat, chat_before,
        "Manager must not append to Specification chat"
    );
    assert_eq!(
        project.activity.manager_history.last().unwrap().text,
        "Board checked; one blocked task needs attention."
    );
    assert_eq!(
        crate::persistence::manager_activity::load(&project.chat_slug)
            .last()
            .unwrap()
            .text,
        "Board checked; one blocked task needs attention."
    );
    let (prompt_history, _) = project.recent_specification_chat("product", 12);
    assert_eq!(prompt_history.len(), 1);
    assert_eq!(prompt_history[0].1, "Current answer");
    assert!(
        project.chat.contains(&legacy),
        "legacy history remains accessible"
    );
}

#[test]
fn repeated_manager_failure_does_not_duplicate_activity() {
    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    let home = std::env::temp_dir().join(format!("manager-failure-home-{}", std::process::id()));
    environment.set("KOOLADE_HOME", &home);
    let mut app = crate::app::root::board_tests::fixture();
    let crate::app::root::Screen::Connected(project) = &mut app.screen else {
        unreachable!()
    };
    project.activity.last_update = Some(Instant::now());
    let error = "provider is unavailable";
    project.activity.manager = Some(crate::app::manager::Manager::completed_for_test(Err(
        error.into()
    )));
    advance(project, &mut None);
    let history_len = project.activity.manager_history.len();
    project.activity.manager = Some(crate::app::manager::Manager::completed_for_test(Err(
        error.into()
    )));
    advance(project, &mut None);
    assert_eq!(project.activity.manager_history.len(), history_len);
    assert_eq!(project.activity.last_manager_error.as_deref(), Some(error));
}

#[path = "tests/reordered_outcomes.rs"]
mod reordered_outcomes;
#[path = "tests/sequence_gap.rs"]
mod sequence_gap;
