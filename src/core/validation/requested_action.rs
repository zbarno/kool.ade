use crate::{
    core::workflow::TurnPurpose,
    harness::{RequestedAction, TurnEnvelope},
};

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
}
