pub(super) mod motion;
use crate::ui::theme;

const CARD_HEIGHT: f32 = 162.0;

pub(crate) fn board_card(
    ui: &mut egui::Ui,
    key: &str,
    kind: Option<crate::domain::ItemKind>,
    active: bool,
    attention: bool,
    done: bool,
    body: impl FnOnce(&mut egui::Ui),
) -> bool {
    ui.push_id(key, |ui| {
        let progress = motion::enter(ui);
        let shape_start = motion::shape_count(ui);
        let frame = theme::board_state_frame(kind, active, attention, done);
        let corners = frame.corner_radius;
        let base_fill = frame.fill;
        let halo = ui.painter().add(egui::Shape::Noop);
        let background = ui.painter().add(egui::Shape::Noop);
        let border = ui.painter().add(egui::Shape::Noop);
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), CARD_HEIGHT),
            egui::Sense::click(),
        );
        let hovered = ui.rect_contains_pointer(rect);
        let relation_id = egui::Id::new("koolade_board_relationship_map");
        let relationships = ui.ctx().data_mut(|data| {
            data.get_temp::<std::collections::HashMap<String, std::collections::HashSet<String>>>(
                relation_id,
            )
        });
        let mut related_keys = relationships
            .and_then(|map| map.get(key).cloned())
            .unwrap_or_default();
        related_keys.insert(key.to_owned());
        if hovered {
            ui.ctx().data_mut(|data| {
                data.insert_temp(egui::Id::new("koolade_board_hover_next"), related_keys);
            });
        }
        let active_related = ui
            .ctx()
            .data_mut(|data| {
                data.get_temp::<std::collections::HashSet<String>>(egui::Id::new(
                    "koolade_board_hover_active",
                ))
            })
            .is_some_and(|keys| keys.len() > 1 && keys.contains(key));
        let active_mix = ui
            .ctx()
            .animate_bool(egui::Id::new(("card_active", key)), active);
        let attention_mix = ui
            .ctx()
            .animate_bool(egui::Id::new(("card_attention", key)), attention);
        let done_mix = ui
            .ctx()
            .animate_bool(egui::Id::new(("card_done", key)), done);
        let idle_fill = if active { theme::PANEL_ALT } else { base_fill };
        let fill = idle_fill.lerp_to_gamma(theme::CARD_HOVER, active_mix);
        let fill = if hovered {
            fill.lerp_to_gamma(theme::CARD_HOVER, 1.0)
        } else {
            fill
        };
        let edge_color = if attention {
            theme::WARNING
        } else if active {
            theme::BLUE_BRIGHT
        } else if done {
            theme::SUCCESS
        } else {
            theme::BORDER_STRONG
        };
        let emphasis = active_mix.max(attention_mix).max(done_mix);
        let edge_width = 1.0 + emphasis;
        if emphasis > 0.0 || hovered {
            let strength = if hovered { 0.8 } else { emphasis };
            ui.painter().set(
                halo,
                egui::Shape::Vec(
                    (1..=4)
                        .rev()
                        .map(|step| {
                            egui::Shape::rect_stroke(
                                rect.expand(step as f32),
                                corners,
                                egui::Stroke::new(
                                    2.0,
                                    edge_color.gamma_multiply(strength * 0.035 * (5 - step) as f32),
                                ),
                                egui::StrokeKind::Outside,
                            )
                        })
                        .collect(),
                ),
            );
        }
        ui.painter()
            .set(background, egui::Shape::rect_filled(rect, corners, fill));
        ui.painter().set(
            border,
            egui::Shape::rect_stroke(
                rect,
                corners,
                egui::Stroke::new(edge_width, edge_color),
                egui::StrokeKind::Inside,
            ),
        );
        motion::flourish(ui, rect, progress, attention, done);
        motion::remember(ui, shape_start, rect);
        if active_related {
            ui.painter().rect_stroke(
                rect.expand(2.0),
                corners,
                egui::Stroke::new(2.0, theme::BLUE_BRIGHT),
                egui::StrokeKind::Outside,
            );
        }
        let mut content = ui.new_child(
            egui::UiBuilder::new()
                .id_salt(("board_card_content", key))
                .max_rect(rect.shrink2(egui::vec2(12.0, 10.0)))
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        content.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
        content.spacing_mut().item_spacing.y = 5.0;
        body(&mut content);
        ui.add_space(10.0);
        response.clicked()
    })
    .inner
}

pub(crate) fn task_key(path: &str) -> String {
    let name = path
        .rsplit('/')
        .next()
        .unwrap_or(path)
        .trim_end_matches(".md");
    if name
        .split_once("-TASK-")
        .is_some_and(|(id, _)| crate::artifacts::product_docs::valid_feature_id(id))
    {
        return name.to_owned();
    }
    let prefix = name.split('-').next().unwrap_or(name);
    format!("TASK-{}", prefix.to_uppercase())
}

pub(crate) fn card_summary(text: &str) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= 56 {
        flat
    } else {
        format!("{}…", flat.chars().take(56).collect::<String>())
    }
}
