use super::*;
use crate::harness::{ApplicationAction, RequestedAction};

mod feature;
mod implementation;
mod target;

#[cfg(test)]
#[path = "requested_action/tests.rs"]
mod tests;

pub(super) fn dispatch(app: &mut PacketApp, request: RequestedAction) {
    if matches!(&app.screen, Screen::Connected(project) if project.active_turn.is_some() || project.task_chats.active.is_some())
    {
        action_feedback(
            app,
            "Packet could not run that action because another conversation is still active. Try again when it finishes.",
        );
        return;
    }
    match request.action {
        ApplicationAction::ApproveChange => feature::approve(app, request.target_uid),
        ApplicationAction::GenerateTasks => feature::generate(app, request.target_uid),
        ApplicationAction::StartImplementation => {
            implementation::start(app, request.target_uid, false)
        }
        ApplicationAction::PauseImplementation => implementation::pause(app, request.target_uid),
        ApplicationAction::ResumeImplementation => {
            implementation::start(app, request.target_uid, true)
        }
        ApplicationAction::Publish => implementation::publish(app, request.target_uid),
    }
}

pub(super) fn start_next(app: &mut PacketApp) {
    implementation::start_from_button(app);
}

fn action_feedback(app: &mut PacketApp, message: &str) {
    if let Screen::Connected(project) = &mut app.screen {
        project.remember_chat(vec![ChatMessage::new(ChatRole::System, message, None)]);
    }
}
