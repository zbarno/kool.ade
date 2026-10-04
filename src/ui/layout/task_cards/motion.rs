//! Short, bounded transitions; snapshots never participate in input handling.
use crate::ui::theme;
use egui::{Color32, Id, Rect, Ui};
use std::collections::HashMap;

#[derive(Clone)]
struct Card {
    born: f64,
    seen: u64,
    leaving: Option<f64>,
    rect: Rect,
    shapes: Vec<egui::epaint::ClippedShape>,
}

#[derive(Clone, Default)]
struct Motion {
    cards: HashMap<Id, Card>,
}

fn storage() -> Id {
    Id::new("board_card_motion")
}

pub(super) fn enter(ui: &mut Ui) -> f32 {
    let now = ui.input(|i| i.time);
    let frame = ui.ctx().cumulative_frame_nr();
    let id = ui.id();
    let born = ui.ctx().data_mut(|data| {
        let state = data.get_temp_mut_or_default::<Motion>(storage());
        let card = state.cards.entry(id).or_insert_with(|| Card {
            born: now,
            seen: frame,
            leaving: None,
            rect: Rect::NOTHING,
            shapes: vec![],
        });
        if card.leaving.take().is_some() {
            card.born = now;
        }
        card.seen = frame;
        card.born
    });
    let progress = if ui.style().animation_time <= 0.0 {
        1.0
    } else {
        ((now - born) / 0.38).clamp(0.0, 1.0) as f32
    };
    if progress < 1.0 {
        ui.ctx().request_repaint();
    }
    let eased = 1.0 - (1.0 - progress).powi(3);
    ui.multiply_opacity(eased);
    ui.add_space(10.0 * (1.0 - eased));
    progress
}

pub(super) fn shape_count(ui: &Ui) -> usize {
    ui.ctx()
        .graphics(|g| g.get(ui.layer_id()).map_or(0, |p| p.all_entries().len()))
}

pub(super) fn remember(ui: &Ui, start: usize, rect: Rect) {
    let shapes = ui.ctx().graphics(|g| {
        g.get(ui.layer_id())
            .map(|p| p.all_entries().skip(start).cloned().collect())
            .unwrap_or_default()
    });
    ui.ctx().data_mut(|data| {
        if let Some(card) = data
            .get_temp_mut_or_default::<Motion>(storage())
            .cards
            .get_mut(&ui.id())
        {
            card.rect = rect;
            card.shapes = shapes;
        }
    });
}

pub(in crate::ui::layout) fn departures(ui: &Ui) {
    let now = ui.input(|i| i.time);
    let frame = ui.ctx().cumulative_frame_nr();
    let enabled = ui.style().animation_time > 0.0;
    let ghosts = ui.ctx().data_mut(|data| {
        let state = data.get_temp_mut_or_default::<Motion>(storage());
        state.cards.retain(|_, c| {
            (enabled || c.seen == frame) && c.leaving.is_none_or(|start| now - start < 0.26)
        });
        state
            .cards
            .values_mut()
            .filter_map(|card| {
                if card.seen == frame {
                    return None;
                }
                let start = *card.leaving.get_or_insert(now);
                Some((card.clone(), ((now - start) / 0.26) as f32))
            })
            .collect::<Vec<_>>()
    });
    for (card, progress) in ghosts {
        let scale = 1.0 - progress * 0.08;
        let transform = egui::emath::TSTransform {
            scaling: scale,
            translation: card.rect.center().to_vec2() * (1.0 - scale)
                + egui::vec2(0.0, -12.0 * progress),
        };
        for clipped in card.shapes {
            let mut painter = ui
                .painter()
                .with_clip_rect(clipped.clip_rect.intersect(ui.clip_rect()));
            painter.multiply_opacity(1.0 - progress);
            let mut shape = clipped.shape;
            shape.transform(transform);
            painter.add(shape);
        }
        ui.ctx().request_repaint();
    }
}

pub(super) fn flourish(ui: &Ui, rect: Rect, progress: f32, attention: bool, done: bool) {
    if !attention && !done {
        return;
    }
    let color = if done { theme::SUCCESS } else { theme::WARNING };
    let label = if done { "Completed" } else { "Needs attention" };
    let marker = Rect::from_min_size(
        rect.right_top() + egui::vec2(-8.0, 8.0),
        egui::vec2(4.0, 16.0),
    );
    ui.painter().rect_filled(marker, 2, color);
    if progress < 1.0 {
        let strength = (std::f32::consts::PI * progress).sin();
        ui.painter().rect_stroke(
            rect.expand(5.0 * progress),
            9,
            egui::Stroke::new(2.0, color.gamma_multiply(strength)),
            egui::StrokeKind::Outside,
        );
        if done {
            for index in 0..10 {
                let angle = index as f32 * std::f32::consts::TAU / 10.0;
                let center = rect.right_top() + egui::vec2(-18.0, 18.0);
                let point = center + egui::vec2(angle.cos(), angle.sin()) * (8.0 + 24.0 * progress);
                ui.painter().circle_filled(
                    point,
                    2.0 * (1.0 - progress),
                    Color32::from_rgb(128, 230, 180).gamma_multiply(strength),
                );
            }
        }
    }
    ui.interact(
        marker.expand(4.0),
        ui.id().with("status_marker"),
        egui::Sense::hover(),
    )
    .on_hover_text(label);
}

#[cfg(test)]
mod tests;
