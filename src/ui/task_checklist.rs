//! Compact task-specific checklist derived from a story's acceptance criteria.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub label: String,
    pub complete: bool,
    pub criterion: String,
}

pub fn from_story(markdown: &str) -> Vec<Item> {
    let mut in_criteria = false;
    let mut items = Vec::new();
    for line in markdown.lines() {
        let line = line.trim();
        if line.starts_with("## ") {
            in_criteria = line
                .trim_start_matches('#')
                .trim()
                .eq_ignore_ascii_case("acceptance criteria");
            continue;
        }
        if !in_criteria {
            continue;
        }
        let Some(text) = line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")) else {
            continue;
        };
        let (text, complete) = if let Some(text) = text
            .strip_prefix("[x] ")
            .or_else(|| text.strip_prefix("[X] "))
        {
            (text, true)
        } else {
            (text.strip_prefix("[ ] ").unwrap_or(text), false)
        };
        let label = simplify(text);
        if !label.is_empty() {
            items.push(Item {
                label,
                complete,
                criterion: text.trim().to_owned(),
            });
        }
    }
    items
}

/// A verified implementation has satisfied its recorded acceptance criteria even
/// while publication/review is pending. Never carry that result into a new attempt.
pub fn from_task(
    markdown: &str,
    state: Option<&crate::core::implementation::Implementation>,
) -> Vec<Item> {
    use crate::core::implementation::ImplementationStatus as Status;
    let mut items = from_story(markdown);
    if let Some(state) = state {
        let verified = state.verified_head.is_some()
            && matches!(
                state.status,
                Status::ReadyToPublish
                    | Status::Publishing
                    | Status::WaitingForIndependentChecks
                    | Status::AwaitingReview
                    | Status::WaitingToMerge
                    | Status::Completed
            );
        if verified {
            let recorded = from_story(&state.ticket_text);
            for item in &mut items {
                item.complete |= recorded.iter().any(|old| old.criterion == item.criterion);
            }
        }
    }
    items
}

fn simplify(text: &str) -> String {
    let text = text.trim().trim_matches('`').trim_end_matches('.');
    let words: Vec<_> = text.split_whitespace().take(7).collect();
    let label = words.join(" ");
    if label.chars().count() > 52 {
        let mut short: String = label.chars().take(49).collect();
        short.push('…');
        short
    } else {
        label
    }
}

pub fn paint(
    ui: &mut egui::Ui,
    items: &[Item],
    complete: bool,
    remaining_only: bool,
    _active: bool,
    _elapsed: Option<&str>,
) {
    if items.is_empty() {
        return;
    }
    let checked = if complete {
        items.len()
    } else {
        items.iter().filter(|item| item.complete).count()
    };
    ui.label(
        egui::RichText::new(if checked == 0 {
            format!("{} criteria · awaiting verification", items.len())
        } else {
            format!("{checked} of {} complete", items.len())
        })
        .size(12.0)
        .weak(),
    );
    let limit = if remaining_only { 3 } else { items.len() };
    for item in items
        .iter()
        .filter(|item| !remaining_only || !(complete || item.complete))
        .take(limit)
    {
        let done = complete || item.complete;
        ui.horizontal(|ui| {
            let (box_rect, _) = ui.allocate_exact_size(egui::vec2(9.0, 9.0), egui::Sense::hover());
            ui.painter().rect_stroke(
                box_rect,
                2,
                egui::Stroke::new(1.0, crate::ui::theme::TEXT_MUTED),
                egui::StrokeKind::Inside,
            );
            if done {
                ui.painter()
                    .rect_filled(box_rect.shrink(2.0), 1, crate::ui::theme::SUCCESS);
            }
            ui.add(
                egui::Label::new(
                    egui::RichText::new(&item.label)
                        .size(12.0)
                        .color(crate::ui::theme::TEXT_DIM),
                )
                .wrap_mode(if remaining_only {
                    egui::TextWrapMode::Truncate
                } else {
                    egui::TextWrapMode::Wrap
                }),
            )
            .on_hover_text(&item.criterion);
        });
    }
    if remaining_only && !complete && items.len() - checked > limit {
        ui.label(
            egui::RichText::new(format!(
                "+ {} more in task details",
                items.len() - checked - limit
            ))
            .size(12.0)
            .color(crate::ui::theme::TEXT_DIM),
        );
    }
}
