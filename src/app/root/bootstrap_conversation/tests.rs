use super::*;

fn task(id: &str, state: crate::persistence::setup_tasks::State) -> LocalTask {
    LocalTask {
        schema_version: 1,
        id: id.into(),
        title: "Synthetic setup task".into(),
        explanation: "Synthetic setup observation.".into(),
        next_step: "Review this setup task.".into(),
        dependencies: Vec::new(),
        blocked_by: Vec::new(),
        state,
        created_at_unix_ms: 1,
        observed_at_unix_ms: 1,
        scan_generation: 1,
        verified_at_unix_ms: None,
        verified_result: None,
        attempts: Vec::new(),
        events: Vec::new(),
        legacy_source_id: None,
        superseded_by: Vec::new(),
    }
}

fn suggestion(
    operation: BootstrapIntentOperationV1,
    image_profile: Option<BootstrapImageProfileV1>,
) -> BootstrapIntentSuggestionV1 {
    BootstrapIntentSuggestionV1 {
        operation,
        image_profile,
    }
}

#[test]
fn only_current_nonmutating_or_reviewable_suggestions_are_accepted() {
    use crate::persistence::setup_tasks::State;
    let recheck = suggestion(BootstrapIntentOperationV1::RecheckPrerequisites, None);
    let open_tools = suggestion(BootstrapIntentOperationV1::OpenCodingTools, None);
    assert!(suggest_remediation_intent(
        &task("setup:docker:rootless", State::Failed),
        &recheck
    ));
    assert!(!suggest_remediation_intent(
        &task("setup:docker:rootless", State::Verified),
        &recheck
    ));
    assert!(suggest_remediation_intent(
        &task("setup:cli:authenticate", State::Detected),
        &open_tools
    ));
    assert!(!suggest_remediation_intent(
        &task("setup:docker:rootless", State::Failed),
        &open_tools
    ));
}

#[test]
fn image_suggestions_must_match_the_exact_unblocked_task_profile_and_state() {
    use crate::persistence::setup_tasks::State;
    let image =
        "setup:images:rust:amd64:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let rust = suggestion(
        BootstrapIntentOperationV1::PrepareLockedImage,
        Some(BootstrapImageProfileV1::Rust),
    );
    let general = suggestion(
        BootstrapIntentOperationV1::PrepareLockedImage,
        Some(BootstrapImageProfileV1::General),
    );
    let mut current = task(image, State::ReadyToFix);
    assert!(suggest_remediation_intent(&current, &rust));
    assert!(!suggest_remediation_intent(&current, &general));
    current.blocked_by.push("setup:docker:engine".into());
    assert!(!suggest_remediation_intent(&current, &rust));
    current.blocked_by.clear();
    current.state = State::Unsupported;
    assert!(!suggest_remediation_intent(&current, &rust));
}

#[test]
fn task_snapshot_fingerprint_changes_when_scan_or_state_changes() {
    use crate::persistence::setup_tasks::State;
    let original = task("setup:docker:rootless", State::Failed);
    let host = crate::app::setup_attention::catalog::HostKind::Linux;
    let mut changed_generation = original.clone();
    changed_generation.scan_generation += 1;
    let mut changed_state = original.clone();
    changed_state.state = State::Verified;
    let mut changed_title = original.clone();
    changed_title.title.push_str(" changed");
    let mut changed_explanation = original.clone();
    changed_explanation.explanation.push_str(" changed");
    let mut changed_next_step = original.clone();
    changed_next_step.next_step.push_str(" changed");
    assert_ne!(
        task_fingerprint(&original, host),
        task_fingerprint(&changed_generation, host)
    );
    assert_ne!(
        task_fingerprint(&original, host),
        task_fingerprint(&changed_state, host)
    );
    assert_ne!(
        task_fingerprint(&original, host),
        task_fingerprint(&changed_title, host)
    );
    assert_ne!(
        task_fingerprint(&original, host),
        task_fingerprint(&changed_explanation, host)
    );
    assert_ne!(
        task_fingerprint(&original, host),
        task_fingerprint(&changed_next_step, host)
    );
    assert_ne!(
        task_fingerprint(&original, host),
        task_fingerprint(
            &original,
            crate::app::setup_attention::catalog::HostKind::Windows
        )
    );
}

#[test]
fn reply_kinds_render_only_fixed_questions_or_app_owned_task_facts() {
    let current = task(
        "setup:docker:rootless",
        crate::persistence::setup_tasks::State::Failed,
    );
    assert_eq!(
        response_message(BootstrapConversationReplyV1::ExplainObservation, &current),
        "The setup task's current observation is: Synthetic setup observation."
    );
    assert_eq!(
        response_message(BootstrapConversationReplyV1::RepeatNextStep, &current),
        "The setup task's suggested next step is: Review this setup task."
    );
    assert_eq!(
        response_message(BootstrapConversationReplyV1::AskWhatToExplain, &current),
        "Which part of this setup task should I explain?"
    );
    assert_eq!(
        response_message(BootstrapConversationReplyV1::AskWhatTaskShows, &current),
        "What does the setup task show after the latest check?"
    );
}

mod lifecycle;
mod suggestions;
