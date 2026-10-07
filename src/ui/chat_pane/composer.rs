use super::Intent;
use crate::domain::chatlog::ChatMessage;
use crate::ui::theme;
use egui::{Frame, Layout, RichText, TextEdit};
pub(super) struct ComposeCopy<'a> {
    /// `id_salt` giving the pane's composer editor a surface-distinct
    /// widget id (Main Chat and task tabs never share a TextEdit id).
    pub(super) composer_id: &'static str,
    /// Placeholder shown while the draft is empty.
    pub(super) hint: &'static str,
    pub(super) context: Option<&'a str>,
    pub(super) actions: &'a [crate::ui::feature_approval::Action],
    pub(super) busy: bool,
    pub(super) progress: Option<&'a crate::harness::LiveProgress>,
    pub(super) offer: Option<&'a crate::core::workflow::InterviewBrief>,
    pub(super) implementation_offer: bool,
}

pub(super) fn paint_with_hint(
    ui: &mut egui::Ui,
    messages: &[ChatMessage],
    draft: &mut String,
    compose: &ComposeCopy,
) -> Intent {
    let ComposeCopy {
        composer_id,
        hint,
        context,
        actions,
        busy,
        progress,
        offer,
        implementation_offer,
    } = *compose;
    let mut cancel = false;
    let mut generate_tasks = false;
    let mut implement_tasks = false;
    let mut approve_feature = None;
    // Flipped once per frame by a claimed chip tap so the composer can pin
    // the caret and re-grab focus right behind the inserted text.
    let mut chip_fired = false;

    // ---------- message scroll ----------
    // RESERVE composer + (optionally) working-strip height UP FRONT:
    // with the messages list unconstrained, an empty conversation would eat
    // the whole pane and push the composer below the fold (invisible box).
    let reserve: f32 = 130.0 + if busy { 52.0 } else { 0.0 };
    ui.scope(|ui| {
        ui.set_max_height((ui.available_height() - reserve).max(48.0));
        egui::ScrollArea::vertical()
            .id_salt("conversation_scroll")
            .stick_to_bottom(true)
            .auto_shrink(egui::Vec2b::new(false, false))
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                let max_w = ui.available_width();
                if let Some(context) = context {
                    ui.collapsing("Task context", |ui| {
                        ui.label(context);
                    });
                }
                // One lift per pane at most: the freshest stored agent reply
                // still owed an answer (a later user message retires it).
                let lift_at = crate::ui::reply_tail::open_ask_index(messages);
                for (index, m) in messages.iter().enumerate() {
                    super::message::paint_message(
                        ui,
                        m,
                        max_w,
                        Some(index) == lift_at,
                        draft,
                        &mut chip_fired,
                        !busy,
                    );
                    ui.add_space(8.0);
                }
                approve_feature = super::feature_approval::paint(ui, actions, busy).map(|(id, _)| id);
                if actions.iter().any(|action| action.prepare_tasks) {
                    // Feature actions already provide the applicable next step.
                } else if implementation_offer {
                    theme::card_frame().show(ui, |ui| {
                        ui.label(RichText::new("Ready to implement").strong());
                        ui.label("Approve the feature associated with the next eligible task and start implementation. Auto Build continues approved tasks; Auto Publish is controlled separately.");
                        implement_tasks = ui.add_enabled(!busy, egui::Button::new("Implement tasks")).clicked();
                    });
                } else if let Some(brief) = offer {
                    theme::card_frame().show(ui, |ui| {
                        ui.label(RichText::new("Ready for task stories").strong());
                        ui.label(RichText::new(&brief.feature_name).size(13.0));
                        ui.label(RichText::new("Turn the agreed scope into a detailed implementation plan, or keep refining it below.").size(12.0).weak());
                        generate_tasks = ui.button("Generate task stories").clicked();
                    });
                }
                if let Some(progress) = progress {
                    super::progress::paint_progress(ui, progress);
                }
            });
    });

    // ---------- working strip (only while a turn runs) ----------
    if busy {
        Frame::NONE
            .fill(theme::PANEL_ALT)
            .corner_radius(6.0)
            .stroke(egui::Stroke::new(1.0, theme::ACCENT_SOFT))
            .inner_margin(egui::Margin::symmetric(10, 8))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    theme::operation_indicator(ui);
                    ui.label(RichText::new("agent working…").weak());
                    ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button(RichText::new("Cancel").weak()).clicked() {
                            cancel = true;
                        }
                    });
                });
            });
        ui.add_space(8.0);
    }

    // Keep the editor and its actions in one rounded surface.
    let editable = !busy;
    let mut send = false;
    Frame::NONE
        .fill(theme::PANEL_ALT)
        .corner_radius(22.0)
        .inner_margin(egui::Margin::symmetric(16, 12))
        .show(ui, |ui| {
            let editor = egui::ScrollArea::vertical()
                .id_salt((composer_id, "draft_scroll"))
                .max_height(44.0)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    TextEdit::multiline(draft)
                        .hint_text(hint)
                        .id_salt(composer_id)
                        .desired_width(f32::INFINITY)
                        .desired_rows(2)
                        .frame(egui::Frame::NONE)
                        .interactive(editable)
                        .show(ui)
                })
                .inner;
            if chip_fired {
                // A chip tap landed its option this frame: park the caret at
                // the end of the fresh text and hand the box back (typing
                // continues behind the inserted option).
                crate::ui::reply_tail::pin_caret_to_end(&editor, ui.ctx(), draft);
            }
            ui.horizontal(|ui| {
                ui.label(RichText::new("Ctrl + Enter to send").size(10.5).weak());
                ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                    let enabled = editable && !draft.trim().is_empty();
                    if composer_id == "task_tab_composer" {
                        send = ui
                            .add_enabled(enabled, egui::Button::new("Send reply"))
                            .on_hover_text("Send to this task's conversation")
                            .clicked();
                        return;
                    }
                    let (rect, response) =
                        ui.allocate_exact_size(egui::vec2(36.0, 36.0), egui::Sense::click());
                    let center = rect.center();
                    ui.painter().circle_filled(
                        center,
                        18.0,
                        if enabled {
                            theme::TEXT
                        } else {
                            theme::TEXT_DIM
                        },
                    );
                    let stroke = egui::Stroke::new(2.0, theme::BG);
                    ui.painter().line_segment(
                        [center + egui::vec2(0.0, 7.0), center - egui::vec2(0.0, 7.0)],
                        stroke,
                    );
                    ui.painter().line_segment(
                        [
                            center + egui::vec2(-6.0, -1.0),
                            center - egui::vec2(0.0, 7.0),
                        ],
                        stroke,
                    );
                    ui.painter().line_segment(
                        [
                            center + egui::vec2(6.0, -1.0),
                            center - egui::vec2(0.0, 7.0),
                        ],
                        stroke,
                    );
                    send = response.on_hover_text("Send message").clicked() && enabled;
                });
            });
            if editable
                && editor.response.has_focus()
                && !draft.trim().is_empty()
                && ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Enter))
            {
                send = true;
            }
        });
    Intent {
        send,
        cancel,
        generate_tasks,
        implement_tasks,
        approve_feature,
    }
}
