use super::*;

#[test]
fn lane_changes_animate_arrival_and_departure_then_release_snapshots() {
    let ctx = egui::Context::default();
    let frame = |time, lane: Option<usize>| {
        let mut output = ctx.run_ui(
            egui::RawInput {
                time: Some(time),
                ..Default::default()
            },
            |ui| {
                if let Some(lane) = lane {
                    ui.push_id(lane, |ui| {
                        crate::ui::layout::task_cards::board_card(
                            ui,
                            "task",
                            None,
                            false,
                            lane == 3,
                            lane == 4,
                            |ui| {
                                ui.label("A task");
                            },
                        );
                    });
                }
                departures(ui);
            },
        );
        output.textures_delta.clear();
        output
    };
    frame(0.0, Some(0));
    frame(0.5, Some(0));
    frame(0.6, Some(4));
    let state = ctx.data_mut(|data| data.get_temp::<Motion>(storage()).unwrap());
    assert_eq!(state.cards.len(), 2);
    assert_eq!(
        state.cards.values().filter(|c| c.leaving.is_some()).count(),
        1
    );
    let output = frame(0.75, Some(4));
    assert!(
        output
            .shapes
            .iter()
            .any(|s| matches!(&s.shape, egui::Shape::Circle(circle) if circle.radius < 2.0)),
        "completion burst is painted"
    );
    frame(1.0, Some(4));
    assert_eq!(
        ctx.data_mut(|data| data.get_temp::<Motion>(storage()).unwrap().cards.len()),
        1
    );
    frame(1.1, None);
    frame(1.5, None);
    assert!(ctx.data_mut(|data| data.get_temp::<Motion>(storage()).unwrap().cards.is_empty()));
}
