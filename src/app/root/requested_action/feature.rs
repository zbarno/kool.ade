use super::*;

pub(super) fn approve(app: &mut PacketApp, target: Option<String>) {
    let id = match target::resolve_feature(app, target.as_deref()) {
        Ok(id) => id,
        Err(error) => {
            action_feedback(app, &error);
            return;
        }
    };
    let Screen::Connected(project) = &app.screen else {
        return;
    };
    if crate::core::workflow::feature_approved(
        &project.state.repo_root,
        &project.state.workflow,
        &id,
    ) {
        action_feedback(
            app,
            &format!("{id} is already approved for its current specification."),
        );
    } else if app
        .available_feature_actions(None)
        .iter()
        .any(|action| action.id == id)
    {
        app.approve_feature_only(&id);
    } else {
        action_feedback(
            app,
            &format!(
                "{id} is no longer ready for approval. Review its current specification and board state first."
            ),
        );
    }
}

pub(super) fn generate(app: &mut PacketApp, target: Option<String>) {
    let Screen::Connected(project) = &app.screen else {
        return;
    };
    if let Some(target) = target.as_deref() {
        let id = match target::resolve_feature(app, Some(target)) {
            Ok(id) => id,
            Err(error) => {
                action_feedback(app, &error);
                return;
            }
        };
        if project
            .state
            .active_feature
            .as_ref()
            .map(|feature| feature.0.as_str())
            != Some(id.as_str())
        {
            action_feedback(
                app,
                &format!(
                    "{id} is not the active planning focus. Make it active and review its current task plan before generating stories."
                ),
            );
            return;
        }
        if has_current_task_batch(project) {
            action_feedback(
                app,
                &format!(
                    "Task stories already exist for {id}. Review the current task batch instead of generating a duplicate."
                ),
            );
            return;
        }
        if !crate::core::workflow::feature_approved(
            &project.state.repo_root,
            &project.state.workflow,
            &id,
        ) {
            action_feedback(
                app,
                &format!(
                    "{id} has not been approved for its current specification. Use its Approve action first; this chat request will not bypass approval."
                ),
            );
            return;
        }
        if !project
            .state
            .workflow
            .ready(project.state.planning_contract())
        {
            action_feedback(
                app,
                &format!(
                    "{id} does not have a current reviewed task plan. Refresh the plan before generating task stories."
                ),
            );
            return;
        }
        if !project.active_implementations.is_empty() {
            action_feedback(
                app,
                &format!(
                    "{id} is approved, but task generation must wait until the current implementation workers finish."
                ),
            );
            return;
        }
        app.start_turn_with_purpose(
            &format!("Generate task stories for approved feature {id}."),
            crate::core::workflow::TurnPurpose::GenerateTasks,
        );
        return;
    }

    if let Some((id, _)) = &project.state.active_feature {
        let id = id.clone();
        generate(app, Some(id));
    } else if app.task_offer().is_some() {
        app.start_turn_with_purpose(
            "Generate task stories for the current reviewed plan.",
            crate::core::workflow::TurnPurpose::GenerateTasks,
        );
    } else {
        action_feedback(
            app,
            "There is no current, reviewed plan ready for task generation. Finish the interview and approve the change first.",
        );
    }
}

use super::action_feedback;
