//! Left pane: restored conversation (from ~/.koolade), the working-status
//! strip while a turn runs, and the composer. Returns per-frame intents.
//! CHG-003 story 5 adds tappable option-chip rows beneath lifted digest
//! asks: a tap joins the full choice text into the draft (newline-joined),
//! pins the caret at its end, and never sends.

use super::feature_approval;
use crate::domain::chatlog::ChatMessage;
mod composer;
mod message;
mod progress;
#[cfg(test)]
mod tests;
pub use progress::paint_progress;

/// Per-frame intents produced by painting the pane.
#[derive(Default, Debug)]
pub struct Intent {
    pub send: bool,
    pub cancel: bool,
    pub generate_tasks: bool,
    pub implement_tasks: bool,
    pub approve_feature: Option<String>,
}

pub struct Actions<'a> {
    pub task_offer: Option<&'a crate::core::workflow::InterviewBrief>,
    pub implementation_offer: bool,
    pub features: &'a [super::feature_approval::Action],
}

pub fn paint(
    ui: &mut egui::Ui,
    messages: &[ChatMessage],
    draft: &mut String,
    busy: bool,
    progress: Option<&crate::harness::LiveProgress>,
    offer: Option<&crate::core::workflow::InterviewBrief>,
    implementation_offer: bool,
) -> Intent {
    paint_with_actions(
        ui,
        messages,
        draft,
        busy,
        progress,
        Actions {
            task_offer: offer,
            implementation_offer,
            features: &[],
        },
    )
}

pub fn paint_with_actions(
    ui: &mut egui::Ui,
    messages: &[ChatMessage],
    draft: &mut String,
    busy: bool,
    progress: Option<&crate::harness::LiveProgress>,
    actions: Actions<'_>,
) -> Intent {
    composer::paint_with_hint(
        ui,
        messages,
        draft,
        &composer::ComposeCopy {
            composer_id: "main_chat_composer",
            hint: "What are you building?",
            context: None,
            actions: actions.features,
            busy,
            progress,
            offer: actions.task_offer,
            implementation_offer: actions.implementation_offer,
        },
    )
}

pub fn paint_specification(
    ui: &mut egui::Ui,
    messages: &[ChatMessage],
    draft: &mut String,
    busy: bool,
    progress: Option<&crate::harness::LiveProgress>,
) -> Intent {
    composer::paint_with_hint(
        ui,
        messages,
        draft,
        &composer::ComposeCopy {
            composer_id: "main_chat_composer",
            hint: "What should change in this specification?",
            context: None,
            actions: &[],
            busy,
            progress,
            offer: None,
            implementation_offer: false,
        },
    )
}

pub fn paint_task(
    ui: &mut egui::Ui,
    messages: &[ChatMessage],
    draft: &mut String,
    busy: bool,
) -> Intent {
    paint_task_with_context(ui, messages, draft, busy, None)
}

pub fn paint_task_with_context(
    ui: &mut egui::Ui,
    messages: &[ChatMessage],
    draft: &mut String,
    busy: bool,
    context: Option<&str>,
) -> Intent {
    paint_task_with_actions(ui, messages, draft, busy, context, &[])
}

pub fn paint_task_with_actions(
    ui: &mut egui::Ui,
    messages: &[ChatMessage],
    draft: &mut String,
    busy: bool,
    context: Option<&str>,
    actions: &[super::feature_approval::Action],
) -> Intent {
    composer::paint_with_hint(
        ui,
        messages,
        draft,
        &composer::ComposeCopy {
            composer_id: "task_tab_composer",
            hint: "Reply about this task…",
            context,
            actions,
            busy,
            progress: None,
            offer: None,
            implementation_offer: false,
        },
    )
}
