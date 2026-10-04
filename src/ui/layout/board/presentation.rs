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

pub(super) fn metadata(ui: &mut Ui, kind: Option<ItemKind>, id: &str) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let (dot, _) = ui.allocate_exact_size(egui::vec2(6.0, 6.0), egui::Sense::hover());
        ui.painter().rect_filled(dot, 1, theme::board_hue(kind));
        let label = kind.map_or_else(|| "TASK".to_owned(), |kind| kind.to_string().to_uppercase());
        ui.label(
            RichText::new(label)
                .size(10.5)
                .strong()
                .color(theme::TEXT_DIM),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add(
                egui::Label::new(
                    RichText::new(id)
                        .monospace()
                        .size(10.5)
                        .color(theme::TEXT_MUTED),
                )
                .truncate(),
            )
            .on_hover_text(id);
        });
    });
}

pub(super) fn description(ui: &mut Ui, markdown: &str) {
    let mut in_summary = false;
    let mut lines = Vec::new();
    for line in markdown.lines() {
        if let Some(heading) = line.strip_prefix("## ") {
            if in_summary {
                break;
            }
            in_summary = matches!(
                heading.trim().to_lowercase().as_str(),
                "intent" | "summary" | "description" | "goal" | "purpose"
            );
        } else if in_summary && !line.trim().is_empty() {
            if line.starts_with("<!--") || line.starts_with('-') {
                break;
            }
            lines.push(line.trim());
        } else if in_summary && !lines.is_empty() {
            break;
        }
    }
    let summary = lines.join(" ");
    if !summary.is_empty() {
        let mut job = egui::text::LayoutJob::simple(
            summary.clone(),
            egui::FontId::proportional(13.0),
            theme::TEXT_DIM,
            ui.available_width(),
        );
        job.wrap.max_rows = 2;
        let galley = ui.painter().layout_job(job);
        ui.add(egui::Label::new(galley)).on_hover_text(summary);
    }
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

pub(super) fn title_text(ui: &Ui, title: &str) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple(
        title.to_owned(),
        egui::FontId::proportional(15.5),
        theme::TEXT,
        (ui.available_width() - 2.0 * ui.spacing().button_padding.x).max(40.0),
    );
    job.wrap.max_rows = 3;
    ui.painter().layout_job(job)
}
