use super::*;

#[test]
fn task_popup_prioritizes_human_title_and_keeps_live_action_visible() {
    for size in [egui::vec2(1280.0, 720.0), egui::vec2(360.0, 720.0)] {
        let mut app = fixture();
        let key = ".koolade-packet/planning/tasks/fixture/F7-TASK-compute-day-and-week-totals";
        let ticket = format!("{key}.md");
        let full_title = "F7-TASK-compute-day-and-week-totals — Compute day and ISO-week per-workspace totals from ledger intervals";
        let Screen::Connected(project) = &mut app.screen else {
            unreachable!()
        };
        project.task_documents[0].path = ticket.clone();
        project.task_documents[0].title = full_title.into();
        project.active_implementations.insert(
            ticket.clone(),
            crate::core::implementation::Controller::idle_fixture(),
        );
        project.activity.tasks.insert(
            ticket.clone(),
            crate::harness::LiveProgress {
                thoughts: "Inspecting the latest check results".into(),
                activity: Some(format!("Implementing {ticket} (attempt 1)…")),
                ..Default::default()
            },
        );
        let ctx = super::mockup_layout::styled_context();
        ctx.data_mut(|data| {
            data.insert_temp(egui::Id::new("koolade_selected_task"), ticket.clone())
        });
        for _ in 0..3 {
            frame_at(&mut app, &ctx, vec![], size);
        }
        let output = frame_at(&mut app, &ctx, vec![], size);
        assert!(text_position(&output, "Task details").is_some());
        assert!(
            text_position(&output, full_title).is_none(),
            "slug must not repeat in the heading"
        );
        let stop = text_position(&output, "Stop task and pause queue").expect("live task action");
        assert!(
            stop.x < size.x && stop.y < size.y - 25.0,
            "action must remain visible: {stop:?}"
        );
        assert!(text_position(&output, "Worker detail").is_some());
        assert!(
            !text_contains(&output, &format!("Implementing {ticket}")),
            "raw path stays in the disclosure"
        );
    }
}
