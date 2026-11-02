use crate::ui::task_detail::TaskRelationView;

pub(super) struct NarrowRows {
    pub(super) parent: f32,
    pub(super) dependencies: f32,
    pub(super) selected: f32,
    pub(super) children: f32,
    pub(super) dependents: f32,
}

impl NarrowRows {
    pub(super) fn new(has_parent: bool, dependencies: usize, children: usize) -> Self {
        let parent = 24.0;
        let dependency_start = parent + if has_parent { 74.0 } else { 0.0 } + 12.0;
        let selected = dependency_start + dependencies as f32 * 74.0 + 16.0;
        let child_start = selected + 82.0;
        Self {
            parent,
            dependencies: dependency_start,
            selected,
            children: child_start,
            dependents: child_start + children as f32 * 74.0 + 20.0,
        }
    }
}

pub(super) fn node_position(
    width: f32,
    direction: u8,
    index: usize,
    count: usize,
    narrow_y: f32,
) -> egui::Vec2 {
    if width < 560.0 {
        return egui::vec2(8.0, narrow_y + index as f32 * 74.0);
    }
    let center_y = 126.0;
    let row_y = |index: usize, count: usize| {
        center_y + (index as f32 - (count.saturating_sub(1) as f32 * 0.5)) * 74.0
    };
    match direction {
        0 => egui::vec2(width * 0.5 - 95.0, 26.0),
        1 => egui::vec2(8.0, row_y(index, count)),
        2 => egui::vec2(width - 198.0, row_y(index, count)),
        _ => egui::vec2(width * 0.5 - 95.0, 254.0 + index as f32 * 74.0),
    }
}

pub(super) fn screen_position(
    rect: egui::Rect,
    local: egui::Vec2,
    pan: egui::Vec2,
    zoom: f32,
) -> egui::Pos2 {
    rect.min + pan + local * zoom
}

pub(super) fn task_title(ui: &egui::Ui, ticket: &str) -> String {
    ui.ctx()
        .data_mut(|data| {
            data.get_temp::<std::collections::HashMap<String, String>>(egui::Id::new(
                "koolade_task_titles_by_path",
            ))
            .and_then(|titles| titles.get(ticket).cloned())
        })
        .unwrap_or_else(|| ticket.to_owned())
}

pub fn relation_path(ui: &egui::Ui, relation: &TaskRelationView) -> Option<String> {
    ui.ctx().data_mut(|data| {
        data.get_temp::<std::collections::HashMap<String, String>>(egui::Id::new(
            "koolade_task_paths_by_uid",
        ))
        .and_then(|paths| paths.get(&relation.planning_item_uid).cloned())
    })
}

pub(super) fn draw_arrow(
    painter: &egui::Painter,
    from: egui::Pos2,
    to: egui::Pos2,
    color: egui::Color32,
) {
    let direction = (to - from).normalized();
    let tip = to - direction * 6.0;
    let base = tip - direction * 10.0;
    let side = egui::vec2(-direction.y, direction.x) * 5.0;
    painter.line_segment([from, tip], egui::Stroke::new(1.5, color));
    painter.add(egui::Shape::convex_polygon(
        vec![tip, base + side, base - side],
        color,
        egui::Stroke::NONE,
    ));
}
