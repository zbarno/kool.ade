use crate::domain::ItemKind;
use crate::ui::theme;
use egui::{RichText, Ui};

pub(in crate::ui::layout) fn human_title<'a>(title: &'a str, path: &str) -> &'a str {
    let stem = path
        .rsplit('/')
        .next()
        .unwrap_or(path)
        .trim_end_matches(".md");
    title
        .strip_prefix(stem)
        .and_then(|rest| rest.strip_prefix(" — ").or_else(|| rest.strip_prefix(": ")))
        .filter(|title| !title.trim().is_empty())
        .unwrap_or(title)
}

pub(in crate::ui::layout) fn legend(ui: &mut Ui) {
    let entries = [
        (None, "Task"),
        (Some(ItemKind::Question), "Question"),
        (Some(ItemKind::Ambiguity), "Ambiguity"),
        (Some(ItemKind::Assumption), "Assumption"),
        (Some(ItemKind::Ownership), "Ownership"),
    ];
    let per_row = if ui.available_width() < 440.0 { 3 } else { 5 };
    for row in entries.chunks(per_row) {
        let widths: Vec<_> = row
            .iter()
            .map(|(_, label)| {
                ui.painter()
                    .layout_no_wrap(
                        (*label).into(),
                        egui::FontId::proportional(12.5),
                        theme::TEXT_DIM,
                    )
                    .size()
                    .x
                    + 16.0
            })
            .collect();
        let gap = if per_row == 5 { 20.0 } else { 14.0 };
        let width = widths.iter().sum::<f32>() + gap * (row.len() - 1) as f32;
        let left = ((ui.available_width() - width) / 2.0).max(0.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.add_space(left);
            for (index, (kind, label)) in row.iter().enumerate() {
                if index > 0 {
                    ui.add_space(gap);
                }
                let (dot, _) = ui.allocate_exact_size(egui::vec2(7.0, 7.0), egui::Sense::hover());
                ui.painter().rect_filled(dot, 2, theme::board_hue(*kind));
                ui.add_space(9.0);
                ui.label(RichText::new(*label).size(12.5).color(theme::TEXT_DIM));
            }
        });
    }
}
