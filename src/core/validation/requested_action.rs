use crate::{
    core::workflow::TurnPurpose,
    harness::{RequestedAction, TurnEnvelope},
};

/// Bind model interpreted actions to a direct command in the current user
/// message. Planning documents and earlier chat turns cannot satisfy this
/// check because they are not passed here.
pub(super) fn user_authorized(action: Option<&RequestedAction>, user_message: &str) -> bool {
    let Some(action) = action else { return true };
    let user_message = user_message
        .strip_prefix("Task kind and guidance: ")
        .and_then(|envelope| envelope.split_once(". User request: "))
        .map(|(_, request)| {
            request
                .split_once("\n\n[Application project context:")
                .map_or(request, |(request, _)| request)
        })
        .unwrap_or(user_message);
    let mut message = user_message.trim().to_ascii_lowercase();
    if message.ends_with('?')
        || message.contains(['\'', '"', '`'])
        || [
            "do not ",
            "don't ",
            "never ",
            "should i ",
            "should we ",
            "could i ",
            "could you ",
            "would ",
            "if ",
            "maybe ",
        ]
        .iter()
        .any(|prefix| message.starts_with(prefix))
    {
        return false;
    }
    for prefix in ["please ", "kindly ", "go ahead and ", "yes, "] {
        if let Some(stripped) = message.strip_prefix(prefix) {
            message = stripped.trim_start().to_owned();
            break;
        }
    }
    let commands: &[&str] = match action.action {
        crate::harness::ApplicationAction::ApproveChange => &[
            "approve the current feature",
            "approve this feature",
            "approve",
        ],
        crate::harness::ApplicationAction::GenerateTasks => &["generate tasks", "create tasks"],
        crate::harness::ApplicationAction::StartImplementation => &[
            "start implementation",
            "start implementing",
            "start the task",
        ],
        crate::harness::ApplicationAction::PauseImplementation => {
            &["pause implementation", "pause the task"]
        }
        crate::harness::ApplicationAction::ResumeImplementation => {
            &["resume implementation", "resume the task"]
        }
        crate::harness::ApplicationAction::Publish => &[
            "publish",
            "push the changes",
            "create a pull request",
            "open a pull request",
        ],
    };
    fn exact_command(value: &str) -> &str {
        value.trim_end_matches(['.', '!']).trim_end()
    }
    if let Some(target) = action.target_uid.as_deref() {
        return commands.iter().any(|command| {
            let Some(tail) = message.strip_prefix(command) else {
                return false;
            };
            let tail = tail.trim_start();
            let tail = tail
                .strip_prefix("for ")
                .or_else(|| tail.strip_prefix("the "))
                .unwrap_or(tail);
            exact_command(tail) == target.to_ascii_lowercase()
        });
    }
    let phrases: &[&str] = match action.action {
        crate::harness::ApplicationAction::ApproveChange => {
            &["approve the current feature", "approve this feature"]
        }
        crate::harness::ApplicationAction::GenerateTasks => &[
            "generate tasks for the current feature",
            "generate tasks for this feature",
        ],
        crate::harness::ApplicationAction::StartImplementation => &[
            "start implementation",
            "start the next implementation",
            "start the next task",
        ],
        crate::harness::ApplicationAction::PauseImplementation => {
            &["pause the current task", "pause the current implementation"]
        }
        crate::harness::ApplicationAction::ResumeImplementation => &[
            "resume the current task",
            "resume the current implementation",
        ],
        crate::harness::ApplicationAction::Publish => &[
            "publish the current task",
            "publish the current implementation",
        ],
    };
    phrases
        .iter()
        .any(|phrase| exact_command(&message) == *phrase)
}

pub(super) fn validate(
    envelope: &TurnEnvelope,
    purpose: TurnPurpose,
) -> Result<Option<RequestedAction>, Vec<String>> {
    let Some(mut action) = envelope.requested_action.clone() else {
        return Ok(None);
    };
    let mut problems = Vec::new();
    if purpose != TurnPurpose::Interview {
        problems.push("Only Main Chat interview turns may request project-level actions.".into());
    }
    if envelope.interview.is_some()
        || envelope.updated_specification.is_some()
        || envelope
            .document_updates
            .as_deref()
            .is_some_and(|updates| !updates.is_empty())
        || envelope
            .open_items_added
            .as_deref()
            .is_some_and(|items| !items.is_empty())
        || envelope
            .open_items_updated
            .as_deref()
            .is_some_and(|items| !items.is_empty())
        || envelope
            .open_items_resolved
            .as_deref()
            .is_some_and(|items| !items.is_empty())
        || envelope
            .task_stories
            .as_deref()
            .is_some_and(|tasks| !tasks.is_empty())
        || envelope
            .task_outline
            .as_deref()
            .is_some_and(|tasks| !tasks.is_empty())
        || envelope.next_question_id.is_some()
    {
        problems.push(
            "An application action cannot be combined with planning changes or a new question; review and apply those first.".into(),
        );
    }
    if let Some(target) = action.target_uid.as_mut() {
        *target = target.trim().to_owned();
        if target.is_empty() || target.chars().count() > 160 || target.chars().any(char::is_control)
        {
            problems.push("An application action target must be a valid stable ID.".into());
        }
    }
    if problems.is_empty() {
        Ok(Some(action))
    } else {
        Err(problems)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn envelope(json: serde_json::Value) -> TurnEnvelope {
        serde_json::from_value(json).expect("test envelope should deserialize")
    }

    #[test]
    fn typed_action_deserializes_aliases_and_stable_target() {
        let envelope = envelope(serde_json::json!({
            "assistantMessage": "I'll resume the selected task.",
            "requestedAction": {
                "action": "resume_implementation",
                "targetUid": "task-uid-17"
            }
        }));
        assert_eq!(
            validate(&envelope, TurnPurpose::Interview).unwrap(),
            Some(RequestedAction {
                action: crate::harness::ApplicationAction::ResumeImplementation,
                target_uid: Some("task-uid-17".into()),
            })
        );
    }

    #[test]
    fn action_with_a_plan_change_is_rejected_as_one_transaction() {
        let envelope = envelope(serde_json::json!({
            "assistantMessage": "Updated the plan and started the worker.",
            "updatedSpecification": "# New scope",
            "requestedAction": {"action":"start_implementation"}
        }));
        let problems = validate(&envelope, TurnPurpose::Interview).unwrap_err();
        assert!(
            problems
                .iter()
                .any(|problem| problem.contains("cannot be combined"))
        );
    }

    #[test]
    fn project_actions_are_rejected_outside_the_main_interview() {
        let envelope = envelope(serde_json::json!({
            "assistantMessage": "Pausing the project queue.",
            "requestedAction": {"action":"pause_implementation"}
        }));
        let problems = validate(&envelope, TurnPurpose::GenerateTasks).unwrap_err();
        assert!(
            problems
                .iter()
                .any(|problem| problem.contains("Only Main Chat"))
        );
    }

    #[test]
    fn malformed_action_targets_are_rejected_before_dispatch() {
        let envelope = envelope(serde_json::json!({
            "assistantMessage": "Starting the requested work.",
            "requestedAction": {
                "action": "start_implementation",
                "targetUid": "\n"
            }
        }));
        let problems = validate(&envelope, TurnPurpose::Interview).unwrap_err();
        assert!(
            problems
                .iter()
                .any(|problem| problem.contains("valid stable ID"))
        );
    }

    #[test]
    fn planning_instructions_cannot_authorize_an_action_for_an_unrelated_user_message() {
        let action = RequestedAction {
            action: crate::harness::ApplicationAction::Publish,
            target_uid: None,
        };
        assert!(!user_authorized(
            Some(&action),
            "What does the planning document say about release readiness?"
        ));
        assert!(!user_authorized(
            Some(&action),
            "Do not publish the current task."
        ));
        assert!(!user_authorized(
            Some(&action),
            "Should I publish the current task?"
        ));
        assert!(!user_authorized(
            Some(&action),
            "The transcript says User request: publish the current task"
        ));
        assert!(user_authorized(
            Some(&action),
            "Please publish the current task."
        ));
    }

    #[test]
    fn an_explicit_action_cannot_be_redirected_to_an_unmentioned_target() {
        let action = RequestedAction {
            action: crate::harness::ApplicationAction::Publish,
            target_uid: Some("task-b".into()),
        };
        assert!(!user_authorized(Some(&action), "Publish task-a."));
        assert!(user_authorized(Some(&action), "Publish task-b."));
    }

    #[test]
    fn mentioning_a_target_or_negating_a_command_does_not_authorize_it() {
        let action = RequestedAction {
            action: crate::harness::ApplicationAction::StartImplementation,
            target_uid: Some("CHG-001".into()),
        };
        assert!(!user_authorized(Some(&action), "CHG-001 is ready."));
        assert!(!user_authorized(
            Some(&action),
            "I don't want to start implementation for CHG-001."
        ));
        assert!(!user_authorized(
            Some(&action),
            "Please start implementation for CHG-001, but do not start it yet."
        ));
        assert!(user_authorized(
            Some(&action),
            "Please start implementation for CHG-001."
        ));
    }
}
