use crate::domain::{ChatMessage, ChatRole};
use crate::ui::theme;
use crate::ui::{ApplicationCommand, Surface, task_chat::paint};
// ------------------------------------------------------------------
// CHG-003 story 5: tappable option chips on the answer-needed card.
// ------------------------------------------------------------------

use crate::domain::item::{ItemKind, OpenItem, Priority};
use crate::domain::user::CurrentUser;
use crate::ui::ToastQueue;

pub(super) const CARD_DIGEST: &str = "Ready.\n\n---\n\
- Which vendor shall we bind?\n\
- Yes, bind Aurora effective Monday.\n\
- No, keep Postman.";

/// Minimal Surface probe: an eligible Human-question item `CLR-001`
/// (category General → broadcast-eligible to the seated operator) with
/// controllable task transcript, draft, busyness and send accounting.
#[derive(Default)]
pub(super) struct CardProbe {
    pub(super) items: Vec<OpenItem>,
    pub(super) messages: Vec<ChatMessage>,
    pub(super) main: Vec<ChatMessage>,
    pub(super) draft: String,
    pub(super) draft_present: bool,
    pub(super) busy: bool,
    pub(super) sent: u32,
    pub(super) user: CurrentUser,
    pub(super) stakes: crate::domain::Stakeholders,
    pub(super) toasts: ToastQueue,
}

impl CardProbe {
    /// The standard open answer card: an eligible question whose latest
    /// agent reply ends in a two-choice digest.
    pub(super) fn answer_card(question: &str, digest: &str) -> Self {
        Self {
            items: vec![OpenItem::new(
                "CLR-001".to_string(),
                Priority::High,
                ItemKind::Question,
                "General".to_string(),
                None,
                question.to_string(),
                "Vendor selection".to_string(),
            )],
            messages: vec![
                ChatMessage::new(ChatRole::User, "Pick a vendor.", None),
                ChatMessage::new(ChatRole::Agent, digest, None),
            ],
            user: CurrentUser::new("Operator", vec![]),
            draft_present: true,
            ..Self::default()
        }
    }
}

impl Surface for CardProbe {
    fn session_title(&self) -> &str {
        "card probe"
    }
    fn is_git_repo(&self) -> bool {
        false
    }
    fn git_branch(&self) -> &str {
        ""
    }
    fn git_head(&self) -> &str {
        ""
    }
    fn git_dirty(&self) -> bool {
        false
    }
    fn chat_messages(&self) -> &[ChatMessage] {
        &self.main
    }
    fn chat_draft(&mut self) -> &mut String {
        &mut self.draft
    }
    fn is_busy(&self) -> bool {
        self.busy
    }
    fn conversation_busy(&self) -> bool {
        self.busy
    }
    fn task_chat_active(&self, _key: &str) -> bool {
        self.busy
    }
    fn task_progress(&self, _ticket: &str) -> Option<&crate::harness::LiveProgress> {
        None
    }
    fn task_offer(&self) -> Option<&crate::core::workflow::InterviewBrief> {
        None
    }
    fn implementation_state(
        &self,
        _ticket: &str,
    ) -> Option<&crate::core::implementation::Implementation> {
        None
    }
    fn task_detail_view(&mut self, _ticket: &str) -> Option<crate::ui::task_detail::ViewModel> {
        None
    }
    fn implementation_active(&self, _ticket: &str) -> bool {
        false
    }
    fn implementation_elapsed(&self, _ticket: &str) -> Option<String> {
        None
    }
    fn auto_plan(&self) -> bool {
        false
    }
    fn auto_build(&self) -> bool {
        false
    }
    fn auto_implement(&self) -> bool {
        false
    }
    fn auto_publish(&self) -> bool {
        false
    }
    fn require_independent_checks(&self) -> bool {
        false
    }
    fn queue_status(&self) -> &str {
        ""
    }
    fn live_progress(&self) -> Option<&crate::harness::LiveProgress> {
        None
    }
    fn planning_board(&self) -> crate::ui::planning_board::ViewModel {
        crate::ui::planning_board::ViewModel {
            planning_items: self.items.clone(),
            eligible_item_ids: crate::core::routing::eligible_items(
                &self.items,
                &self.user,
                &self.stakes,
            )
            .iter()
            .map(|item| item.id.clone())
            .collect(),
            ..Default::default()
        }
    }
    fn next_question_id(&self) -> Option<&str> {
        None
    }
    fn spec_text(&self) -> &str {
        ""
    }
    fn toasts(&mut self) -> &mut ToastQueue {
        &mut self.toasts
    }
    fn dispatch(&mut self, command: ApplicationCommand) {
        if matches!(command, ApplicationCommand::SendTaskReply { .. }) {
            self.sent += 1;
        }
    }
    // The card-under-test hooks.
    fn task_messages(&self, _key: &str) -> &[ChatMessage] {
        &self.messages
    }
    fn task_draft(&mut self, key: &str) -> Option<&mut String> {
        (key == "CLR-001" && self.draft_present).then_some(&mut self.draft)
    }
}

/// (rect, stroke colour) pairs for every chip-styled cell on the frame.
pub(super) fn card_chip_cells(output: &egui::FullOutput) -> Vec<(egui::Rect, egui::Color32)> {
    output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Rect(rect)
                if rect.fill == theme::CHIP_FILL
                    && rect.corner_radius == egui::CornerRadius::same(10_u8) =>
            {
                Some((rect.rect, rect.stroke.color))
            }
            _ => None,
        })
        .collect()
}

/// Centre of the text shape whose full text equals `needle`.
pub(super) fn card_point(output: &egui::FullOutput, needle: &str) -> Option<egui::Pos2> {
    output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Text(shape) => Some(shape),
            _ => None,
        })
        .find(|shape| shape.galley.text() == needle)
        .map(|shape| shape.pos + shape.galley.mesh_bounds.center().to_vec2())
}

pub(super) fn card_has_text(output: &egui::FullOutput, needle: &str) -> bool {
    output.shapes.iter().any(|clipped| match &clipped.shape {
        egui::Shape::Text(shape) => shape.galley.text().contains(needle),
        _ => false,
    })
}

/// Walk-order position among TEXT galleys, for nesting assertions.
pub(super) fn card_walk_index(output: &egui::FullOutput, needle: &str) -> Option<usize> {
    output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Text(shape) => Some(&**shape.galley),
            _ => None,
        })
        .position(|text| text == needle)
}

/// Runs one card frame with the probe; returns the frame output.
pub(super) fn card_frame(
    ctx: &egui::Context,
    probe: &mut CardProbe,
    expanded: bool,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    ctx.run_ui(
        egui::RawInput {
            events,
            ..Default::default()
        },
        |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                let _ = paint(ui, probe, "CLR-001", expanded);
            });
        },
    )
}

pub(super) fn press_events(pos: egui::Pos2) -> Vec<egui::Event> {
    vec![
        egui::Event::PointerMoved(pos),
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: Default::default(),
        },
    ]
}

pub(super) fn release_events(pos: egui::Pos2) -> Vec<egui::Event> {
    vec![egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Default::default(),
    }]
}
