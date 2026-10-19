use crate::{app::session::Project, domain::chatlog::ChatMessage};

pub(super) fn remember_turn_messages(
    project: &mut Project,
    planning_work_key: Option<&str>,
    messages: Vec<ChatMessage>,
) {
    if let Some(key) = planning_work_key {
        project.remember_planning_work_chat(key, messages);
    } else {
        project.remember_turn_chat(messages);
    }
}
