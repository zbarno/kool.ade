use super::*;
pub(super) const SHARED_MD: &str = "# Heading\n\nLead with **boldlead** and finish plain.";

pub(super) fn galleys(output: &egui::FullOutput) -> Vec<&egui::Galley> {
    output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Text(shape) => Some(&*shape.galley),
            _ => None,
        })
        .collect()
}

pub(super) fn galley_texts(output: &egui::FullOutput) -> Vec<String> {
    galleys(output)
        .iter()
        .map(|galley| galley.text().to_owned())
        .collect()
}

pub(super) fn section_text<'a>(
    galley: &'a egui::Galley,
    section: &egui::text::LayoutSection,
) -> &'a str {
    &galley.job.text[section.byte_range.start.0..section.byte_range.end.0]
}

pub(super) fn paint_ctx() -> egui::Context {
    let ctx = egui::Context::default();
    ctx.set_visuals(theme::koolade_visuals());
    ctx
}

pub(super) const DIGEST_FIXTURE: &str = "Draft ready.\n\n---\n\
- Which vendor shall we bind?\n\
- Recommended: Aurora, effective Monday.\n\
- Pointer: CLR-021 impact notes.";
pub(super) const DIGEST_ROWS: [&str; 3] = [
    "Which vendor shall we bind?",
    "Recommended: Aurora, effective Monday.",
    "Pointer: CLR-021 impact notes.",
];

/// Rectangles filled exactly [`theme::DIGEST_BG`] — the lifted backdrop.
pub(super) fn digest_backdrops(output: &egui::FullOutput) -> usize {
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
pub(super) fn border_lines(output: &egui::FullOutput) -> usize {
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
pub(super) fn galley_positions(output: &egui::FullOutput, needle: &str) -> Vec<usize> {
    galleys(output)
        .into_iter()
        .enumerate()
        .filter_map(|(i, galley)| (galley.text() == needle).then_some(i))
        .collect()
}

/// First non-empty section format of a galley.
pub(super) fn lead_section_format(galley: &egui::Galley) -> egui::text::TextFormat {
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
pub(super) fn main_chrome_border_lines() -> usize {
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
pub(super) fn card_tab_chrome_border_lines() -> usize {
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
pub(super) fn body_border_lines(messages: &[ChatMessage], lifted: Option<usize>) -> usize {
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
pub(super) fn assert_rows_in_payload_order(output: &egui::FullOutput, rows: &[&str]) {
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

pub(super) const CHOICE_DIGEST: &str = "Ready.\n\n---\n\
- Which vendor shall we bind?\n\
- Yes, bind Aurora effective Monday.\n\
- No, keep Postman.";

/// (rect, stroke colour) pairs for every chip-styled cell: a RoundRect
/// filled exactly [`theme::CHIP_FILL`] with corner radius 10.0.
pub(super) fn chip_cells(output: &egui::FullOutput) -> Vec<(egui::Rect, egui::Color32)> {
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
pub(super) fn galley_point(output: &egui::FullOutput, needle: &str) -> Option<egui::Pos2> {
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
pub(super) struct ChipTapHarness {
    pub(super) ctx: egui::Context,
    pub(super) messages: Vec<ChatMessage>,
    pub(super) draft: String,
    pub(super) busy: bool,
}

impl ChipTapHarness {
    pub(super) fn new(messages: Vec<ChatMessage>, busy: bool) -> Self {
        Self {
            ctx: paint_ctx(),
            messages,
            draft: String::new(),
            busy,
        }
    }

    /// One frame of the main pane; returns (frame output, intent).
    pub(super) fn frame(&mut self, events: Vec<egui::Event>) -> (egui::FullOutput, Intent) {
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
    pub(super) fn click_at(&mut self, pos: egui::Pos2) -> (egui::FullOutput, Intent) {
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
