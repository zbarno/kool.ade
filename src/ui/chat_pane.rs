//! Left pane: restored conversation (from ~/.packet), the working-status
//! strip while a turn runs, and the composer. Returns per-frame intents.
//! CHG-003 story 5 adds tappable option-chip rows beneath lifted digest
//! asks: a tap joins the full choice text into the draft (newline-joined),
//! pins the caret at its end, and never sends.

use egui::{Frame, Layout, RichText, TextEdit};

use crate::domain::chatlog::{ChatMessage, ChatRole};
use crate::ui::theme;

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
    paint_with_hint(
        ui,
        messages,
        draft,
        &ComposeCopy {
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
    paint_with_hint(
        ui,
        messages,
        draft,
        &ComposeCopy {
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

/// Surface-specific static composer copy. Kept as ONE private parameter
/// (rather than two scalars) so [`paint_with_hint`] does not accrue
/// additional parameters past the lint baseline it already carries.
struct ComposeCopy<'a> {
    /// `id_salt` giving the pane's composer editor a surface-distinct
    /// widget id (Main Chat and task tabs never share a TextEdit id).
    composer_id: &'static str,
    /// Placeholder shown while the draft is empty.
    hint: &'static str,
    context: Option<&'a str>,
    actions: &'a [super::feature_approval::Action],
    busy: bool,
    progress: Option<&'a crate::harness::LiveProgress>,
    offer: Option<&'a crate::core::workflow::InterviewBrief>,
    implementation_offer: bool,
}

fn paint_with_hint(
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
                    paint_message(
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
                    paint_progress(ui, progress);
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
                    ui.spinner();
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

fn paint_message(
    ui: &mut egui::Ui,
    m: &ChatMessage,
    max_w: f32,
    lift: bool,
    draft: &mut String,
    chip_fired: &mut bool,
    interactive: bool,
) {
    let mine = m.role == ChatRole::User;
    let indent = if mine { 32.0 } else { 0.0 };
    ui.horizontal(|ui| {
        ui.add_space(indent);
        ui.vertical(|ui| {
            ui.set_width((max_w - indent - 8.0).max(100.0));
            if !mine {
                ui.horizontal(|ui| {
                    if m.role != ChatRole::Agent {
                        ui.label(RichText::new("Update").size(11.0).weak());
                    }
                    ui.label(
                        RichText::new(m.ts.format("%H:%M").to_string())
                            .size(10.0)
                            .weak(),
                    );
                    if let Some(item) = &m.ref_item {
                        theme::badge(ui, item, theme::PANEL_ALT, theme::TEXT_DIM);
                    }
                });
                ui.add_space(2.0);
            }
            Frame::NONE
                .fill(if mine {
                    theme::PANEL_ALT
                } else {
                    egui::Color32::TRANSPARENT
                })
                .corner_radius(20.0)
                .inner_margin(if mine { 16 } else { 0 })
                .show(ui, |ui| {
                    if m.role == ChatRole::Agent {
                        // Agent prose renders as structured dark-theme
                        // Markdown. Card-tab messages arrive pre-shielded by
                        // layout.rs and `readable` is idempotent on
                        // already-shielded prose, so the re-shield composes.
                        let shielded = crate::ui::message_text::readable(m);
                        if lift {
                            // Lifted reply: the tail leaves the body (it no
                            // longer sits buried in the text) and paints as
                            // its own distinct block directly underneath.
                            let tail = crate::ui::reply_tail::parse_reply_tail(&shielded);
                            crate::ui::markdown::paint(ui, &tail.body, crate::ui::markdown::CHAT);
                            crate::ui::reply_tail::paint_open_ask(ui, &tail);
                            // CHG-003 story 5: tappable chips for digest
                            // bullets that carry recognisable options
                            // (2..=6 matches, else the tail declines; a tap
                            // joins the FULL choice text, never sends).
                            let choices = crate::ui::reply_tail::digest_choices(&tail);
                            if let Some(index) =
                                crate::ui::reply_tail::paint_chip_row(ui, &choices, interactive)
                            {
                                crate::ui::reply_tail::join_choice(draft, &choices[index], '\n');
                                *chip_fired = true;
                            }
                        } else {
                            crate::ui::markdown::paint(ui, &shielded, crate::ui::markdown::CHAT);
                        }
                    } else {
                        // User bubbles and System notices stay plain —
                        // only agent prose is Markdown-rendered.
                        ui.add(
                            egui::Label::new(
                                RichText::new(m.text.trim())
                                    .size(15.0)
                                    .line_height(Some(23.0))
                                    .color(theme::TEXT),
                            )
                            .wrap(),
                        );
                    }
                });
        });
    });
}

pub fn paint_progress(ui: &mut egui::Ui, progress: &crate::harness::LiveProgress) {
    ui.push_id("live_turn", |ui| {
        if !progress.posts.is_empty() {
            for post in &progress.posts {
                ui.push_id(post.id, |ui| {
                    if post.kind == "thinking" {
                        egui::CollapsingHeader::new(
                            RichText::new("Thinking").size(12.0).color(theme::TEXT_DIM),
                        )
                        .id_salt("thought_block")
                        .default_open(true)
                        .show(ui, |ui| {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(post.text.trim())
                                        .size(13.0)
                                        .line_height(Some(20.0))
                                        .color(theme::TEXT_DIM),
                                )
                                .wrap(),
                            );
                        });
                    } else if post.kind == "tool" {
                        egui::CollapsingHeader::new(format!(
                            "Tool output · {}",
                            post.text.lines().next().unwrap_or("tool")
                        ))
                        .id_salt("tool_output")
                        .show(ui, |ui| {
                            ui.add(
                                egui::Label::new(RichText::new(&post.text).monospace().size(12.0))
                                    .wrap(),
                            );
                        });
                    } else {
                        // Streaming reply prose: painted progressively,
                        // degrading harmlessly on in-flight prefixes (the
                        // harness projection withholds envelopes and
                        // unterminated opening fences upstream).
                        crate::ui::markdown::paint(ui, post.text.trim(), crate::ui::markdown::CHAT);
                    }
                    ui.add_space(4.0);
                });
            }
            if let Some(activity) = &progress.activity {
                ui.label(RichText::new(activity).size(11.0).weak());
            }
            return;
        }

        if !progress.thoughts.is_empty() {
            egui::CollapsingHeader::new(
                RichText::new("Thinking").size(12.0).color(theme::TEXT_DIM),
            )
            .default_open(true)
            .show(ui, |ui| {
                ui.add(
                    egui::Label::new(
                        RichText::new(progress.thoughts.trim())
                            .size(13.0)
                            .line_height(Some(20.0))
                            .color(theme::TEXT_DIM),
                    )
                    .wrap(),
                );
            });
            ui.add_space(4.0);
        }
        if let Some(activity) = &progress.activity {
            ui.add(
                egui::Label::new(RichText::new(activity).size(11.0).color(theme::TEXT_DIM)).wrap(),
            );
            ui.add_space(4.0);
        }
        if !progress.response.is_empty() {
            crate::ui::markdown::paint(ui, progress.response.trim(), crate::ui::markdown::CHAT);
            ui.add_space(4.0);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::{LivePost, LiveProgress};

    /// One Markdown body shared by all three roles so the negative
    /// controls (User/System) lock the role gate against the converted
    /// agent bubble.
    const SHARED_MD: &str = "# Heading\n\nLead with **boldlead** and finish plain.";

    fn galleys(output: &egui::FullOutput) -> Vec<&egui::Galley> {
        output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Text(shape) => Some(&*shape.galley),
                _ => None,
            })
            .collect()
    }

    fn galley_texts(output: &egui::FullOutput) -> Vec<String> {
        galleys(output)
            .iter()
            .map(|galley| galley.text().to_owned())
            .collect()
    }

    fn section_text<'a>(galley: &'a egui::Galley, section: &egui::text::LayoutSection) -> &'a str {
        &galley.job.text[section.byte_range.start.0..section.byte_range.end.0]
    }

    fn paint_ctx() -> egui::Context {
        let ctx = egui::Context::default();
        ctx.set_visuals(theme::packet_visuals());
        ctx
    }

    #[test]
    fn agent_replies_paint_markdown_while_user_and_system_bubbles_stay_plain() {
        let ctx = paint_ctx();
        let messages = vec![
            ChatMessage::new(ChatRole::Agent, SHARED_MD, None),
            ChatMessage::new(ChatRole::User, SHARED_MD, None),
            ChatMessage::new(ChatRole::System, SHARED_MD, None),
        ];
        let mut draft = String::new();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            paint(ui, &messages, &mut draft, false, None, None, false);
        });
        let texts = galley_texts(&output);
        let bearing = texts.iter().filter(|t| t.contains("boldlead"));
        assert_eq!(
            bearing.count(),
            3,
            "each role paints the body once: {texts:?}"
        );
        let scrubbed = texts
            .iter()
            .filter(|t| t.contains("boldlead") && !t.contains('*'));
        assert_eq!(
            scrubbed.count(),
            1,
            "exactly the agent bubble scrubs markers: {texts:?}"
        );
        // Negative controls: User and System galleys keep every raw marker.
        for text in &texts {
            if text.contains("boldlead") && text.contains('*') {
                assert!(
                    text.contains('#') && text.contains('\n'),
                    "control stays verbatim: {text}"
                );
            }
        }
        // The agent bold run carries a distinct TextFormat from its sibling.
        let agent_body = galleys(&output)
            .into_iter()
            .find(|g| g.text().contains("boldlead") && !g.text().contains('*'))
            .expect("agent paragraph galley");
        let bold_section = agent_body
            .job
            .sections
            .iter()
            .find(|s| section_text(agent_body, s) == "boldlead")
            .expect("styled bold section");
        let plain_section = agent_body
            .job
            .sections
            .iter()
            .find(|s| section_text(agent_body, s) == "Lead with ")
            .expect("plain lead section");
        assert_ne!(
            bold_section.format, plain_section.format,
            "bold differs from plain"
        );
        assert!(bold_section.format.extra_letter_spacing.abs() > 1e-9);
        assert!(plain_section.format.extra_letter_spacing.abs() < f32::EPSILON);
        assert_eq!(bold_section.format.color, theme::TEXT);
        // The heading renders at CHAT.h1 in theme ink.
        let heading = galleys(&output)
            .into_iter()
            .find(|g| g.text() == "Heading")
            .expect("heading galley");
        for section in &heading.job.sections {
            assert!(
                (section.format.font_id.size - crate::ui::markdown::CHAT.h1).abs() < 1e-6,
                "heading size {}",
                section.format.font_id.size
            );
            assert_eq!(section.format.color, theme::TEXT);
        }
        output.textures_delta.clear();
    }

    #[test]
    fn card_tab_entry_paints_the_same_agent_markdown_after_double_shield() {
        // layout.rs pre-swaps card messages' text with `readable` output
        // before paint_task re-shields; this pins that the round trip is
        // stable and the reply still renders styled and marker-free.
        let prose = "Shaped reply.\n\n- ship it\n- then **verify**";
        let raw_envelope = serde_json::json!({
            "assistant_message": prose,
            "schema_version": 1,
        })
        .to_string();
        let original = ChatMessage::new(ChatRole::Agent, raw_envelope, None);
        let pre_shielded = crate::ui::message_text::readable(&original);
        assert_eq!(pre_shielded.as_ref(), prose);
        let swapped = ChatMessage::new(ChatRole::Agent, pre_shielded.into_owned(), None);
        let re_shielded = crate::ui::message_text::readable(&swapped);
        assert_eq!(re_shielded.as_ref(), prose, "double shield must round-trip");
        let messages = vec![swapped];
        let ctx = paint_ctx();
        let mut draft = String::new();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            paint_task(ui, &messages, &mut draft, false);
        });
        let texts = galley_texts(&output);
        assert!(
            texts
                .iter()
                .any(|t| t.contains("verify") && !t.contains('*')),
            "card-tab agent reply must be marker-free: {texts:?}"
        );
        assert!(
            !texts.iter().any(|t| t.contains('{')),
            "no envelope braces: {texts:?}"
        );
        output.textures_delta.clear();
    }

    #[test]
    fn raw_envelopes_never_reach_the_reply_galley() {
        let ctx = paint_ctx();
        let envelope =
            "{\"assistant_message\":\"Hi **you**\",\"schema_version\":1,\"open_items_updated\":[]}";
        let messages = vec![ChatMessage::new(ChatRole::Agent, envelope, None)];
        let mut draft = String::new();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            paint(ui, &messages, &mut draft, false, None, None, false);
        });
        let texts = galley_texts(&output);
        assert!(
            texts.iter().any(|t| t.contains("you") && !t.contains('*')),
            "readable prose must render styled: {texts:?}"
        );
        assert!(
            !texts
                .iter()
                .any(|t| t.contains("assistant_message") || t.contains('{')),
            "envelope leaked into a chat galley: {texts:?}"
        );
        output.textures_delta.clear();
    }

    #[test]
    fn degenerate_agent_bodies_leave_the_pane_intact() {
        for body in ["", "   \n\t  ", "---", "solo", "```rust\nlet value = 1;"] {
            let ctx = paint_ctx();
            let messages = vec![ChatMessage::new(ChatRole::Agent, body, None)];
            let mut draft = String::new();
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                paint(ui, &messages, &mut draft, false, None, None, false);
            });
            let texts = galley_texts(&output);
            // Surrounding chrome (composer hint, send affordance) survives.
            assert!(
                texts.iter().any(|t| t.contains("Ctrl + Enter to send")),
                "composer hint missing for body {body:?}"
            );
            // Painter never leaks fence glyphs, not even on an unclosed one.
            assert!(
                texts.iter().all(|t| !t.contains("```")),
                "fence glyphs leaked for body {body:?}: {texts:?}"
            );
            output.textures_delta.clear();
        }
        // The unclosed fence still lands as a monospace line in the bubble.
        let ctx = paint_ctx();
        let messages = vec![ChatMessage::new(
            ChatRole::Agent,
            "```rust\nlet value = 1;",
            None,
        )];
        let mut draft = String::new();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            paint(ui, &messages, &mut draft, false, None, None, false);
        });
        let code_line = galleys(&output)
            .into_iter()
            .find(|g| g.text() == "let value = 1;")
            .expect("unclosed fence paints its code line");
        assert!(
            code_line
                .job
                .sections
                .iter()
                .all(|s| s.format.font_id.family == egui::FontFamily::Monospace)
        );
        output.textures_delta.clear();
    }

    #[test]
    fn live_stream_keeps_diagnostic_regimes_and_formats_prose_chunks() {
        let streaming_md = "Streamed **boldchunk** and a half fence\n```rust\nlet n = 2;";
        let progress = LiveProgress {
            posts: vec![
                LivePost {
                    id: (1, 0),
                    kind: "thinking".to_string(),
                    text: "reasoning **still plain**".to_string(),
                },
                LivePost {
                    id: (2, 0),
                    kind: "tool".to_string(),
                    text: "cargo build\nCompiling packet v0.1.0".to_string(),
                },
                LivePost {
                    id: (3, 0),
                    kind: "text".to_string(),
                    text: streaming_md.to_string(),
                },
            ],
            ..Default::default()
        };
        let ctx = paint_ctx();
        // Frame 1: the thinking diagnostic is open by default and stays
        // plainly labeled; the streamed prose renders through the painter.
        let mut first = ctx.run_ui(egui::RawInput::default(), |ui| {
            paint_progress(ui, &progress);
        });
        let texts = galley_texts(&first);
        assert!(
            texts
                .iter()
                .any(|t| t.contains("boldchunk") && !t.contains('*')),
            "streamed prose must be marker-free: {texts:?}"
        );
        assert!(
            texts
                .iter()
                .any(|t| t.contains("reasoning **still plain**")),
            "plain thinking body must show its literal markers: {texts:?}"
        );
        // Locate the tool diagnostics header and click it open.
        let header_center = first
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Text(shape) => Some(shape),
                _ => None,
            })
            .find(|shape| (*shape.galley).text().starts_with("Tool output"))
            .map(|shape| shape.pos + (*shape.galley).size() * 0.5)
            .expect("tool diagnostics header");
        let click = egui::RawInput {
            events: vec![
                egui::Event::PointerMoved(header_center),
                egui::Event::PointerButton {
                    pos: header_center,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
                egui::Event::PointerButton {
                    pos: header_center,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: Default::default(),
                },
            ],
            ..Default::default()
        };
        // The collapsible fades open over a short wall-clock animation, so
        // keep painting frames until the tool body is fully disclosed.
        let mut saw_tool_mono = false;
        for _ in 0..120 {
            let input = if !saw_tool_mono {
                click.clone()
            } else {
                egui::RawInput::default()
            };
            let mut out = ctx.run_ui(input, |ui| paint_progress(ui, &progress));
            for galley in galleys(&out) {
                if galley.text() == "cargo build\nCompiling packet v0.1.0" {
                    saw_tool_mono = galley
                        .job
                        .sections
                        .iter()
                        .all(|sect| sect.format.font_id.family == egui::FontFamily::Monospace);
                    break;
                }
            }
            out.textures_delta.clear();
            if saw_tool_mono {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert!(saw_tool_mono, "disclosed tool output must stay monospace");
        first.textures_delta.clear();

        // Posts empty: the response fallback renders through the painter.
        let response_only = LiveProgress {
            response: "Final **word** lands.".to_string(),
            ..Default::default()
        };
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            paint_progress(ui, &response_only);
        });
        let texts = galley_texts(&output);
        assert!(
            texts.iter().any(|t| t.contains("word") && !t.contains('*')),
            "fallback response must be marker-free: {texts:?}"
        );
        output.textures_delta.clear();
    }

    // ------------------------------------------------------------------
    // CHG-003 story 4: the lifted at-a-glance digest block.
    // ------------------------------------------------------------------

    const DIGEST_FIXTURE: &str = "Draft ready.\n\n---\n\
- Which vendor shall we bind?\n\
- Recommended: Aurora, effective Monday.\n\
- Pointer: CLR-021 impact notes.";
    const DIGEST_ROWS: [&str; 3] = [
        "Which vendor shall we bind?",
        "Recommended: Aurora, effective Monday.",
        "Pointer: CLR-021 impact notes.",
    ];

    /// Rectangles filled exactly [`theme::DIGEST_BG`] — the lifted backdrop.
    fn digest_backdrops(output: &egui::FullOutput) -> usize {
        output
            .shapes
            .iter()
            .filter(|clipped| match &clipped.shape {
                egui::Shape::Rect(rect) => rect.fill == theme::DIGEST_BG,
                _ => false,
            })
            .count()
    }

    /// Line segments stroked in [`theme::BORDER`]: pane chrome (0 by design),
    /// the Markdown `---` separator when the body keeps its rule, and the
    /// one lift hairline.
    fn border_lines(output: &egui::FullOutput) -> usize {
        output
            .shapes
            .iter()
            .filter(|clipped| match &clipped.shape {
                egui::Shape::LineSegment { stroke, .. } => stroke.color == theme::BORDER,
                _ => false,
            })
            .count()
    }

    /// Indices (walk order) of the galleys whose full text is `needle`.
    fn galley_positions(output: &egui::FullOutput, needle: &str) -> Vec<usize> {
        galleys(output)
            .into_iter()
            .enumerate()
            .filter_map(|(i, galley)| (galley.text() == needle).then_some(i))
            .collect()
    }

    /// First non-empty section format of a galley.
    fn lead_section_format(galley: &egui::Galley) -> egui::text::TextFormat {
        galley
            .job
            .sections
            .iter()
            .find(|s| s.byte_range.end.0 > s.byte_range.start.0)
            .map(|s| s.format.clone())
            .expect("galley carries at least one non-empty section")
    }

    /// The main pane's own chrome, painted with inert empty messages: the
    /// zero-point against which lift hairlines are counted.
    fn main_chrome_border_lines() -> usize {
        let messages = vec![
            ChatMessage::new(ChatRole::User, "", None),
            ChatMessage::new(ChatRole::Agent, "", None),
            ChatMessage::new(ChatRole::System, "", None),
        ];
        let ctx = paint_ctx();
        let mut draft = String::new();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            paint(ui, &messages, &mut draft, false, None, None, false);
        });
        let count = border_lines(&output);
        output.textures_delta.clear();
        count
    }

    /// The same zero-point for the card-tab surface.
    fn card_tab_chrome_border_lines() -> usize {
        let messages = vec![
            ChatMessage::new(ChatRole::User, "", None),
            ChatMessage::new(ChatRole::Agent, "", None),
            ChatMessage::new(ChatRole::System, "", None),
        ];
        let ctx = paint_ctx();
        let mut draft = String::new();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            paint_task(ui, &messages, &mut draft, false);
        });
        let count = border_lines(&output);
        output.textures_delta.clear();
        count
    }

    /// BORDER lines the reply bodies themselves draw (Markdown `---`
    /// separators). Mirrors the pane's own body choice per message: the
    /// lifted reply paints `tail.body`, every other message paints its full
    /// readable prose.
    fn body_border_lines(messages: &[ChatMessage], lifted: Option<usize>) -> usize {
        let mut total = 0;
        for (i, m) in messages.iter().enumerate() {
            if m.role != ChatRole::Agent {
                continue;
            }
            let readable = crate::ui::message_text::readable(m);
            let body = if Some(i) == lifted {
                crate::ui::reply_tail::parse_reply_tail(&readable).body
            } else {
                readable.to_string()
            };
            let ctx = paint_ctx();
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    crate::ui::markdown::paint(ui, &body, crate::ui::markdown::CHAT);
                });
            });
            total += border_lines(&output);
            output.textures_delta.clear();
        }
        total
    }

    /// Payload-order scan: every row of `rows` appears in exactly one galley,
    /// and the rows ascend in walk order.
    fn assert_rows_in_payload_order(output: &egui::FullOutput, rows: &[&str]) {
        let mut previous = None;
        for row in rows {
            let spots = galley_positions(output, row);
            assert_eq!(
                spots.len(),
                1,
                "row {row:?} must appear exactly once: {spots:?}"
            );
            if let Some(prev) = previous {
                assert!(prev < spots[0], "payload order broke before {row:?}");
            }
            previous = Some(spots[0]);
        }
    }

    /// AC 1: a stored three-bullet digest floats its unlabeled block under
    /// the Main Chat body — one DIGEST_BG backdrop, one lift hairline, the
    /// three rows verbatim and ordered, lead row stronger, no markers or
    /// labels anywhere.
    #[test]
    fn digest_reply_floats_an_unlabeled_lift_directly_under_the_main_chat_body() {
        let messages = vec![
            ChatMessage::new(ChatRole::User, "Bind the vendor please.", None),
            ChatMessage::new(ChatRole::Agent, DIGEST_FIXTURE, None),
        ];
        assert_eq!(
            crate::ui::reply_tail::open_ask_index(&messages),
            Some(1),
            "the digest reply must select as the open ask"
        );
        let ctx = paint_ctx();
        let mut draft = String::new();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            paint(ui, &messages, &mut draft, false, None, None, false);
        });
        // (a) Exactly one lifted backdrop.
        assert_eq!(
            digest_backdrops(&output),
            1,
            "exactly one DIGEST_BG backdrop expected"
        );
        // (b) Hairline bookkeeping: pane chrome + body rules + the ONE lift
        // rule. The lifted body no longer carries its '---', so the body
        // reference contributes zero here.
        let expected_lines = main_chrome_border_lines() + body_border_lines(&messages, Some(1)) + 1;
        assert_eq!(
            border_lines(&output),
            expected_lines,
            "lift must add exactly one BORDER hairline"
        );
        let texts = galley_texts(&output);
        // The Markdown body renders only the pre-tail prose.
        assert!(
            texts.iter().any(|t| t == "Draft ready."),
            "trimmed body must render: {texts:?}"
        );
        // (c) Every row floats exactly once, in payload order.
        assert_rows_in_payload_order(&output, &DIGEST_ROWS);
        // (d) The lead row carries a stronger TextFormat than the second.
        let lead = galleys(&output)
            .into_iter()
            .find(|g| g.text() == DIGEST_ROWS[0])
            .expect("lead row galley");
        let second = galleys(&output)
            .into_iter()
            .find(|g| g.text() == DIGEST_ROWS[1])
            .expect("second row galley");
        let (lead_fmt, second_fmt) = (lead_section_format(lead), lead_section_format(second));
        assert_ne!(lead_fmt, second_fmt, "lead row must be stronger");
        assert!(
            lead_fmt.extra_letter_spacing.abs() > 1e-9,
            "lead row uses the tracked-bold convention (got {})",
            lead_fmt.extra_letter_spacing
        );
        assert!(
            second_fmt.extra_letter_spacing.abs() < f32::EPSILON,
            "second row stays regular"
        );
        // (e) Purity: no rule glyphs, no bare list markers, no invented label
        // words. (The rows are asserted byte-equal to the parsed payload
        // above, which is the machine check that the painter printed nothing
        // besides the ask/recommendation/pointer text itself.)
        assert!(
            !texts.iter().any(|t| t.contains("---")),
            "rule glyphs leaked: {texts:?}"
        );
        assert!(
            !texts.iter().any(|t| t.trim_start().starts_with("- ")),
            "bare list markers leaked: {texts:?}"
        );
        assert!(
            !texts
                .iter()
                .any(|t| t.starts_with("Ask:") || t.starts_with("Recommendation:")),
            "the lift must stay unlabeled: {texts:?}"
        );
        output.textures_delta.clear();
    }

    /// AC 2: the instant a user answer follows the digest, zero backdrops and
    /// no lift hairline — only the body-intrinsic rule of the still-printed
    /// reply text may remain.
    #[test]
    fn a_user_answer_immediately_retires_the_lift() {
        let messages = vec![
            ChatMessage::new(ChatRole::User, "Bind the vendor please.", None),
            ChatMessage::new(ChatRole::Agent, DIGEST_FIXTURE, None),
            ChatMessage::new(ChatRole::User, "Go with Aurora.", None),
        ];
        assert_eq!(
            crate::ui::reply_tail::open_ask_index(&messages),
            None,
            "an answered ask must deselect"
        );
        let ctx = paint_ctx();
        let mut draft = String::new();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            paint(ui, &messages, &mut draft, false, None, None, false);
        });
        assert_eq!(
            digest_backdrops(&output),
            0,
            "user answer must retire the backdrop"
        );
        let expected_lines = main_chrome_border_lines() + body_border_lines(&messages, None);
        assert_eq!(
            border_lines(&output),
            expected_lines,
            "no lift hairline may survive an answer (allowed: {})",
            expected_lines
        );
        output.textures_delta.clear();
    }

    /// AC 3: the legacy 'Your next step:' reply lifts on Main Chat for the
    /// first time — the question floats strongly and verbatim, the marker is
    /// gone from every galley, and the body still renders.
    #[test]
    fn legacy_your_next_step_lifts_on_main_chat_with_no_marker_leak() {
        const LEGACY: &str = "SSO is recorded.\nYour next step: Should guests use SSO too?";
        const QUESTION: &str = "Should guests use SSO too?";
        let messages = vec![
            ChatMessage::new(ChatRole::User, "Set up SSO.", None),
            ChatMessage::new(ChatRole::Agent, LEGACY, None),
        ];
        assert_eq!(crate::ui::reply_tail::open_ask_index(&messages), Some(1));
        let ctx = paint_ctx();
        let mut draft = String::new();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            paint(ui, &messages, &mut draft, false, None, None, false);
        });
        assert_eq!(digest_backdrops(&output), 1, "legacy ask must float");
        let texts = galley_texts(&output);
        assert_eq!(
            galley_positions(&output, QUESTION).len(),
            1,
            "question lifts verbatim exactly once: {texts:?}"
        );
        let lifted = galleys(&output)
            .into_iter()
            .find(|g| g.text() == QUESTION)
            .expect("lifted question galley");
        assert!(
            lead_section_format(lifted).extra_letter_spacing.abs() > 1e-9,
            "lifted legacy ask must print strongly"
        );
        assert!(
            !texts.iter().any(|t| t.contains("Your next step:")),
            "marker leaked: {texts:?}"
        );
        assert!(
            texts.iter().any(|t| t.contains("SSO is recorded.")),
            "Markdown body must still render: {texts:?}"
        );
        assert_eq!(
            border_lines(&output),
            main_chrome_border_lines() + body_border_lines(&messages, Some(1)) + 1,
            "one lift hairline for the legacy ask"
        );
        output.textures_delta.clear();
    }

    /// AC 4 negative battery: NoReply finals, marker-free plain finals,
    /// answered digests and six-bullet overloads all paint the plain body
    /// with ABSOLUTELY no tail chrome.
    #[test]
    fn non_asking_finals_paint_the_plain_body_with_zero_tail_chrome() {
        const OVERLOAD: &str = "Deck.\n---\n- One\n- Two\n- Three\n- Four\n- Five\n- Six";
        let u = |text: &str| ChatMessage::new(ChatRole::User, text, None);
        let a = |text: &str| ChatMessage::new(ChatRole::Agent, text, None);
        let cases: Vec<(&str, Vec<ChatMessage>)> = vec![
            (
                "NoReply-final",
                vec![
                    u("Confirm SSO."),
                    a("SSO and MFA are confirmed.\nNo reply needed."),
                ],
            ),
            (
                "marker-free plain final",
                vec![u("Status?"), a("All green, nothing blocked.")],
            ),
            (
                "digest-then-user",
                vec![
                    u("Bind the vendor please."),
                    a(DIGEST_FIXTURE),
                    u("Go with Aurora."),
                ],
            ),
            (
                "six-bullet overload degrades to Plain",
                vec![u("Deal breaker?"), a(OVERLOAD)],
            ),
        ];
        let chrome = main_chrome_border_lines();
        for (name, messages) in cases {
            assert_eq!(
                crate::ui::reply_tail::open_ask_index(&messages),
                None,
                "{name}: must not select"
            );
            let ctx = paint_ctx();
            let mut draft = String::new();
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                paint(ui, &messages, &mut draft, false, None, None, false);
            });
            assert_eq!(
                digest_backdrops(&output),
                0,
                "{name}: zero backdrops expected"
            );
            let allowed = chrome + body_border_lines(&messages, None);
            assert_eq!(
                border_lines(&output),
                allowed,
                "{name}: no lift hairline (allowed body/chrome lines: {allowed})"
            );
            output.textures_delta.clear();
        }
    }

    /// AC 6: an intentionally broken envelope shields to plain prose and can
    /// never feed the lift — no braces or envelope keys may reach a galley.
    #[test]
    fn broken_envelopes_shield_to_prose_and_can_never_feed_the_lift() {
        const BROKEN: &str = "{\"assistant_message\":\"Draft ready.\\n\\n---\\n- Which vendor shal";
        let messages = vec![
            ChatMessage::new(ChatRole::User, "Vendor question pending.", None),
            ChatMessage::new(ChatRole::Agent, BROKEN, None),
        ];
        let ctx = paint_ctx();
        let mut draft = String::new();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            paint(ui, &messages, &mut draft, false, None, None, false);
        });
        assert_eq!(digest_backdrops(&output), 0, "shield note must not lift");
        assert_eq!(
            border_lines(&output),
            main_chrome_border_lines(),
            "no hairline of any kind may come from the shield path"
        );
        let texts = galley_texts(&output);
        assert!(
            texts.iter().any(|t| t.contains("unreadable reply")),
            "shield note must render as plain prose: {texts:?}"
        );
        assert!(
            !texts.iter().any(|t| t.contains('{') || t.contains('}')),
            "braces leaked into a galley: {texts:?}"
        );
        assert!(
            !texts.iter().any(|t| t.contains("assistant_message")),
            "envelope key leaked: {texts:?}"
        );
        output.textures_delta.clear();
    }

    /// AC 5: the card tab paints the identical lift at Main Chat parity —
    /// same backdrop, same hairline, same rows in the same order.
    #[test]
    fn card_tabs_paint_the_identical_lift_as_main_chat() {
        let messages = vec![
            ChatMessage::new(ChatRole::User, "Bind the vendor please.", None),
            ChatMessage::new(ChatRole::Agent, DIGEST_FIXTURE, None),
        ];
        assert_eq!(crate::ui::reply_tail::open_ask_index(&messages), Some(1));
        let ctx = paint_ctx();
        let mut draft = String::new();
        let mut main_out = ctx.run_ui(egui::RawInput::default(), |ui| {
            paint(ui, &messages, &mut draft, false, None, None, false);
        });
        let ctx = paint_ctx();
        let mut draft = String::new();
        let mut tab_out = ctx.run_ui(egui::RawInput::default(), |ui| {
            paint_task(ui, &messages, &mut draft, false);
        });
        // Identical findings on both surfaces.
        assert_eq!(
            digest_backdrops(&tab_out),
            1,
            "card tab must float the lift"
        );
        assert_eq!(
            digest_backdrops(&tab_out),
            digest_backdrops(&main_out),
            "backdrop parity"
        );
        assert_rows_in_payload_order(&tab_out, &DIGEST_ROWS);
        assert_rows_in_payload_order(&main_out, &DIGEST_ROWS);
        // Tab hairline bookkeeping: tab chrome + lifted body rules + ONE rule.
        assert_eq!(
            border_lines(&tab_out),
            card_tab_chrome_border_lines() + body_border_lines(&messages, Some(1)) + 1,
            "card tab must add exactly one lift hairline"
        );
        assert!(border_lines(&main_out) >= 1, "main chat hairline present");
        main_out.textures_delta.clear();
        tab_out.textures_delta.clear();
    }

    // ------------------------------------------------------------------
    // CHG-003 story 5: tappable option chips in the Main Chat pane.
    // ------------------------------------------------------------------

    const CHOICE_DIGEST: &str = "Ready.\n\n---\n\
- Which vendor shall we bind?\n\
- Yes, bind Aurora effective Monday.\n\
- No, keep Postman.";

    /// (rect, stroke colour) pairs for every chip-styled cell: a RoundRect
    /// filled exactly [`theme::CHIP_FILL`] with corner radius 10.0.
    fn chip_cells(output: &egui::FullOutput) -> Vec<(egui::Rect, egui::Color32)> {
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

    /// Centre point of the galley whose ENTIRE text equals `needle`.
    fn galley_point(output: &egui::FullOutput, needle: &str) -> Option<egui::Pos2> {
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

    /// Multi-frame driver holding the pane's state (messages, draft, busyness)
    /// across simulated input frames.
    struct ChipTapHarness {
        ctx: egui::Context,
        messages: Vec<ChatMessage>,
        draft: String,
        busy: bool,
    }

    impl ChipTapHarness {
        fn new(messages: Vec<ChatMessage>, busy: bool) -> Self {
            Self {
                ctx: paint_ctx(),
                messages,
                draft: String::new(),
                busy,
            }
        }

        /// One frame of the main pane; returns (frame output, intent).
        fn frame(&mut self, events: Vec<egui::Event>) -> (egui::FullOutput, Intent) {
            let mut intent = Intent::default();
            let output = self.ctx.run_ui(
                egui::RawInput {
                    events,
                    ..Default::default()
                },
                |ui| {
                    intent = paint(
                        ui,
                        &self.messages,
                        &mut self.draft,
                        self.busy,
                        None,
                        None,
                        false,
                    );
                },
            );
            (output, intent)
        }

        /// Pointer moved → press → (frame) → release; returns the release frame.
        fn click_at(&mut self, pos: egui::Pos2) -> (egui::FullOutput, Intent) {
            let (mut press_out, press_intent) = self.frame(vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
            ]);
            assert!(!press_intent.send, "a PRESS must never send");
            press_out.textures_delta.clear();
            self.frame(vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            }])
        }
    }

    /// AC1/AC2 end-to-end: tapping a digest chip joins the FULL bullet text
    /// into the draft (newline-joined, exact), never sends, grabs the
    /// composer, and parks the caret at the inserted end — a later keystroke
    /// lands BEHIND the option text. The chips sit directly under the
    /// lifted ask row.
    #[test]
    fn tapping_a_digest_chip_types_the_full_option_into_the_main_draft_without_sending() {
        let messages = vec![
            ChatMessage::new(ChatRole::User, "Pick a vendor.", None),
            ChatMessage::new(ChatRole::Agent, CHOICE_DIGEST, None),
        ];
        assert_eq!(crate::ui::reply_tail::open_ask_index(&messages), Some(1));
        let choices = crate::ui::reply_tail::digest_choices(
            &crate::ui::reply_tail::parse_reply_tail(CHOICE_DIGEST),
        );
        assert_eq!(choices.len(), 2, "the two bullets offer options");
        let yes_label = crate::ui::reply_tail::choice_label(&choices[0]);
        let no_label = crate::ui::reply_tail::choice_label(&choices[1]);

        let mut run = ChipTapHarness::new(messages, false);
        let (mut output, first_intent) = run.frame(Vec::new());
        assert!(!first_intent.send, "idle frame sends nothing");
        let cells = chip_cells(&output);
        assert_eq!(cells.len(), 2, "two chips under the lifted digest");
        assert!(
            cells.iter().all(|(_, color)| *color == theme::CHIP_BORDER),
            "idle chip strokes rest on CHIP_BORDER"
        );
        // Placement: the chips follow the lifted ask row in walk order.
        let ask_spots = galley_positions(&output, "Which vendor shall we bind?");
        let yes_spots = galley_positions(&output, &yes_label);
        assert_eq!(ask_spots.len(), 1, "ask row visible");
        assert_eq!(yes_spots.len(), 1, "chip one visible");
        assert!(
            ask_spots[0] < yes_spots[0],
            "chip row rides BELOW the lifted ask"
        );
        let yes_point = galley_point(&output, &yes_label).expect("chip one centre");
        assert!(
            galley_point(&output, &no_label).is_some(),
            "chip two visible"
        );
        output.textures_delta.clear();

        assert!(
            run.ctx.memory(|mem| mem.focused()).is_none(),
            "no focus before the tap"
        );
        let (mut release_out, intent) = run.click_at(yes_point);
        release_out.textures_delta.clear();
        assert!(!intent.send, "a chip tap must NOT fire the composer");
        assert!(!intent.cancel && !intent.generate_tasks && !intent.implement_tasks);
        assert_eq!(
            run.draft, choices[0].text,
            "the FULL bullet text joins the draft, byte for byte"
        );

        // The focus request settles on the next frame; typing afterwards
        // must land behind the inserted option (caret pinned at the end).
        let (mut settle_out, _) = run.frame(Vec::new());
        settle_out.textures_delta.clear();
        assert!(
            run.ctx.memory(|mem| mem.focused()).is_some(),
            "the composer holds focus after the tap"
        );
        let (mut typed_out, typed_intent) = run.frame(vec![egui::Event::Text("x".to_string())]);
        typed_out.textures_delta.clear();
        assert!(!typed_intent.send);
        assert_eq!(
            run.draft,
            format!("{}x", choices[0].text),
            "the keystroke landed at the pinned END of the draft"
        );
    }

    /// AC1 negative: while the lane is busy the chips still render (dimmed,
    /// stroke resting) but taps are fully inert — no draft mutation, no send.
    #[test]
    fn busy_panels_render_chips_but_swallow_every_tap() {
        let messages = vec![
            ChatMessage::new(ChatRole::User, "Pick a vendor.", None),
            ChatMessage::new(ChatRole::Agent, CHOICE_DIGEST, None),
        ];
        let choices = crate::ui::reply_tail::digest_choices(
            &crate::ui::reply_tail::parse_reply_tail(CHOICE_DIGEST),
        );
        let yes_label = crate::ui::reply_tail::choice_label(&choices[0]);

        let mut run = ChipTapHarness::new(messages, true);
        let (mut output, _) = run.frame(Vec::new());
        let cells = chip_cells(&output);
        assert_eq!(
            cells.len(),
            2,
            "business dims the chips, it does not erase them"
        );
        assert!(
            cells.iter().all(|(_, color)| *color == theme::CHIP_BORDER),
            "disabled chips keep the resting border (no hover-promotion affordance)"
        );
        output.textures_delta.clear();
        let yes_point = galley_point(&output, &yes_label).expect("dimmed chip one centre");
        let (mut release_out, intent) = run.click_at(yes_point);
        release_out.textures_delta.clear();
        assert!(!intent.send, "busy tap must not send");
        assert!(run.draft.is_empty(), "busy tap must not mutate the draft");
    }

    /// Absence battery (AC1/AC3 discipline): answered digests, single-choice
    /// digests, legacy next-step lifts and plain finals render NO chips —
    /// while the still-valid lifts keep floating (backdrop survives the chip
    /// gate declining). The control: an open two-choice digest renders both.
    #[test]
    fn chip_rows_appear_only_for_open_two_to_six_choice_digests() {
        const ONE_CHOICE: &str = "Deal?\n---\n- Yes, take it.";
        const LEGACY: &str = "SSO is recorded.\nYour next step: Should guests use SSO too?";
        let u = |text: &str| ChatMessage::new(ChatRole::User, text, None);
        let a = |text: &str| ChatMessage::new(ChatRole::Agent, text, None);
        let cases: Vec<(&str, Vec<ChatMessage>, usize)> = vec![
            (
                "answered digest",
                vec![u("Pick a vendor."), a(CHOICE_DIGEST), u("Aurora, Monday.")],
                0, // no lift at all
            ),
            (
                "single-choice digest (gate declined, lift survives)",
                vec![u("Deal?"), a(ONE_CHOICE)],
                1, // one DIGEST_BG backdrop, zero chips
            ),
            (
                "legacy next-step (lift survives, no chips)",
                vec![u("Set up SSO."), a(LEGACY)],
                1,
            ),
            (
                "marker-free plain final",
                vec![u("Status?"), a("All green, nothing blocked.")],
                0,
            ),
        ];
        for (name, messages, backdrops_expected) in cases {
            let mut run = ChipTapHarness::new(messages, false);
            let (mut output, _) = run.frame(Vec::new());
            assert_eq!(chip_cells(&output).len(), 0, "{name}: no chips at all");
            assert_eq!(
                digest_backdrops(&output),
                backdrops_expected,
                "{name}: lift backdrop expectations held"
            );
            output.textures_delta.clear();
        }
        // Control: the open two-choice digest DOES render its row.
        let mut run = ChipTapHarness::new(
            vec![
                ChatMessage::new(ChatRole::User, "Pick a vendor.", None),
                ChatMessage::new(ChatRole::Agent, CHOICE_DIGEST, None),
            ],
            false,
        );
        let (mut output, _) = run.frame(Vec::new());
        assert_eq!(
            chip_cells(&output).len(),
            2,
            "open digest renders both chips"
        );
        output.textures_delta.clear();
    }
}
