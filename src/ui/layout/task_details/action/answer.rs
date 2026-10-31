//! Explicit responses use a separate draft from read-only clarification chat.
use super::*;

pub(super) fn paint(
    ui: &mut egui::Ui,
    surface: &mut dyn Surface,
    state: state::Presentation<'_>,
    requested_changes: bool,
) {
    let id = egui::Id::new(("task_answer", state.ticket));
    let mut answer = ui
        .ctx()
        .data_mut(|data| data.get_temp::<String>(id))
        .unwrap_or_default();
    ui.label(if requested_changes {
        "Requested changes"
    } else {
        "Your response"
    });
    ui.add(
        egui::TextEdit::multiline(&mut answer)
            .desired_width(f32::INFINITY)
            .hint_text(if requested_changes {
                "Describe the changes…"
            } else {
                "Enter your answer…"
            }),
    );
    ui.ctx()
        .data_mut(|data| data.insert_temp(id, answer.clone()));
    if ui
        .add_enabled(
            !state.view.conversation_active && !answer.trim().is_empty(),
            egui::Button::new(if requested_changes {
                "Submit changes"
            } else {
                "Submit answer"
            })
            .fill(theme::BLUE),
        )
        .clicked()
    {
        let payload = if requested_changes {
            answer
        } else {
            format!("My answer: {answer}")
        };
        let before = surface.task_messages(state.ticket).len();
        submit(
            surface,
            state.ticket,
            payload.clone(),
            !requested_changes && state.view.answer_context.is_some(),
        );
        let messages = surface.task_messages(state.ticket);
        if messages.len() > before
            && messages[before..].iter().any(|message| {
                message.role == crate::domain::ChatRole::User
                    && message.text == payload
                    && !message.clarification
            })
        {
            ui.ctx().data_mut(|data| data.remove::<String>(id));
        }
    }
}
