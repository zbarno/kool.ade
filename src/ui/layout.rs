//! Overall window chrome: product wordmark, repository context strip with
//! git snapshot, and the three resizable regions (chat / spec / items).

use egui::{CentralPanel, Color32, Frame, Layout, Panel, RichText};

use crate::domain::chatlog::ChatMessage;
use crate::ui::{theme, Surface};

pub enum HeaderAction {
    Refresh,
    Import,
    Stakeholders,
    CopySpec,
    Disconnect,
}

pub fn paint(ui: &mut egui::Ui, s: &mut dyn Surface) {
    let ctx = ui.ctx().clone();
    paint_header(ui, s);

    // Right: open items
    Panel::right("packet_items")
        .default_size(340.0)
        .min_size(260.0)
        .resizable(true)
        .show(ui, |ui| {
            pane_chrome(ui, "Open Items", s.items_len(), |ui| {
                crate::ui::items_pane::paint(
                    ui,
                    &crate::ui::items_pane::Args {
                        items: s.items(),
                        synthetic: s.synthetic_items(),
                        user: s.current_user(),
                        next_question_id: s.next_question_id(),
                    },
                );
            });
        })
        .response;

    // Left: chat
    Panel::left("packet_chat")
        .default_size(360.0)
        .min_size(260.0)
        .resizable(true)
        .show(ui, |ui| {
            pane_chrome(ui, "Conversation", usize::MAX, |ui| {
                let src_msgs: &[ChatMessage] = s.chat_messages();
                let msgs: Vec<ChatMessage> = src_msgs.to_vec();
                let preview: Option<String> = s.activity_preview().map(str::to_string);
                let busy = s.is_busy();
                let intent = crate::ui::chat_pane::paint(
                    ui,
                    &msgs,
                    s.chat_draft(),
                    busy,
                    preview.as_deref(),
                );
                if intent.send || intent.cancel {
                    s.on_intent(&intent);
                }
            });
        })
        .response;

    // Center: specification
    CentralPanel::default().frame(central_frame()).show(ui, |ui| {
        spec_toolbar(ui, s);
        egui::ScrollArea::both()
            .auto_shrink(egui::Vec2b::new(false, false))
            .show(ui, |ui| {
                ui.add_space(6.0);
                crate::ui::spec_viewer::render(ui, Some(s.spec_text()));
                ui.add_space(40.0);
            });
    });

    s.toasts().show(&ctx);
}

fn spec_toolbar(ui: &mut egui::Ui, s: &dyn Surface) {
    ui.horizontal(|ui| {
        ui.set_min_height(30.0);
        ui.label(RichText::new("Specification").strong().size(13.0));
        theme::badge(ui, "READ-ONLY", theme::PANEL_ALT, theme::TEXT_DIM);
        ui.label(
            RichText::new("edits happen exclusively through planning conversations")
                .weak()
                .size(11.0),
        );
        ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
            let words = s.spec_words();
            ui.label(RichText::new(format!("{words} words")).weak().size(11.0));
        });
    });
    ui.add_space(2.0);
    ui.separator();
    ui.add_space(4.0);
}

fn central_frame() -> Frame {
    Frame::NONE
        .fill(theme::BG)
        .inner_margin(egui::Margin::symmetric(18, 12))
}

fn pane_chrome<F>(ui: &mut egui::Ui, title: &str, count: usize, body: F)
where
    F: FnOnce(&mut egui::Ui),
{
    ui.horizontal(|ui| {
        ui.add_space(2.0);
        ui.label(RichText::new(title).strong().size(13.0).color(theme::TEXT));
        if count < usize::MAX && count > 0 {
            theme::badge(ui, &count.to_string(), theme::PANEL_ALT, theme::TEXT_DIM);
        }
    });
    ui.add_space(2.0);
    ui.separator();
    ui.add_space(4.0);
    body(ui);
}

fn paint_header(ui: &mut egui::Ui, s: &mut dyn Surface) {
    Panel::top("packet_header")
        .exact_size(46.0)
        .show(ui, |ui| {
            let height = 46.0;
            ui.horizontal(|ui| {
                ui.set_min_height(height);
                // Wordmark
                ui.label(
                    RichText::new("PACKET")
                        .strong()
                        .size(15.0)
                        .extra_letter_spacing(0.8)
                        .color(theme::TEXT),
                );
                ui.label(RichText::new("◆").size(14.0).color(theme::ACCENT));
                ui.add_space(6.0);
                // Product descriptor
                ui.label(RichText::new("git-native specification planner").weak().size(11.5));
                ui.separator();
                ui.add_space(4.0);
                // Repo + git status
                ui.label(RichText::new(s.session_title()).strong().size(13.0));
                git_pill(ui, s);
                // Actions
                ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                    for (label, act) in [
                        ("Refresh ⟳", HeaderAction::Refresh),
                        ("Import ⇪", HeaderAction::Import),
                        ("Stakeholders ✎", HeaderAction::Stakeholders),
                        ("Copy spec ⧉", HeaderAction::CopySpec),
                        ("Disconnect ⏻", HeaderAction::Disconnect),
                    ] {
                        if ui.button(RichText::new(label).size(12.0)).clicked() {
                            s.on_header_action(act);
                        }
                    }
                });
            });
        });
}

fn git_pill(ui: &mut egui::Ui, s: &mut dyn Surface) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        if !s.is_git_repo() {
            theme::badge(ui, "NOT A GIT REPO", Color32::from_rgb(64, 26, 26), theme::DANGER);
            return;
        }
        let dirty = s.git_dirty();
        ui.label(
            RichText::new(format!("git:{}", s.git_branch()))
                .monospace()
                .size(11.5)
                .color(if dirty { theme::WARNING } else { theme::SUCCESS }),
        );
        if !s.git_head().is_empty() {
            ui.label(RichText::new(s.git_head()).monospace().size(11.5).weak());
        }
        if dirty {
            theme::badge(ui, "dirty", theme::PANEL_ALT, theme::WARNING);
        }
    });
}
