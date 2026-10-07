use super::*;

#[test]
fn board_mockup_hierarchy_and_centered_legend_at_desktop_sizes() {
    for width in [1280.0, 1600.0] {
        let mut app = fixture();
        let Screen::Connected(project) = &mut app.screen else {
            unreachable!()
        };
        let doc = &mut project.task_documents[0];
        doc.path =
            ".koolade-packet/planning/tasks/fixture/F7-TASK-generate-completed-month-report.md"
                .into();
        doc.title =
            "F7-TASK-generate-completed-month-report — Generate completed-month report".into();
        doc.text = "# Report\n\n## Intent\nBuild a report from recorded activity.\n\n## Acceptance criteria\n- Handle empty months\n- Preserve day ordering\n- Sum recorded time\n- Keep workspaces separate\n- Include report totals\n".into();
        let ctx = styled_context();
        let size = egui::vec2(width, 900.0);
        for _ in 0..3 {
            frame_at(&mut app, &ctx, vec![], size);
        }
        let output = frame_at(&mut app, &ctx, vec![], size);
        let title = text_position(&output, "Generate completed-month report").expect("human title");
        assert!(
            text_position(
                &output,
                "F7-TASK-generate-completed-month-report — Generate completed-month report"
            )
            .is_none()
        );
        assert!(text_position(&output, "+ 2 more in task details").is_none());
        assert!(text_position(&output, "Keep workspaces separate").is_none());
        let first = text_position(&output, "Task").expect("legend starts");
        let last = text_position(&output, "Ownership").expect("legend ends");
        let board_tab = text_position(&output, "Board  3").unwrap();
        let new_task = text_position(&output, "+ New Task").unwrap();
        assert!(
            first.x > board_tab.x && last.x < new_task.x,
            "legend fits between Board and New Task"
        );
        assert!(
            (first.y - board_tab.y).abs() < 6.0 && (last.y - new_task.y).abs() < 6.0,
            "legend shares the navigation row"
        );
        assert!(title.y > first.y);
        let lanes: Vec<_> = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Rect(rect) if rect.fill == crate::ui::theme::COLUMN => Some(rect.rect),
                _ => None,
            })
            .collect();
        assert_eq!(lanes.len(), 5);
        assert!(
            lanes
                .iter()
                .all(|rect| rect.right() <= width && rect.left() >= 0.0)
        );
        assert!(text_position(&output, "Handle empty months").is_none());
    }
}

// The redesigned lane header paints its label and count separately. Locate the
// matching count inside the same lane and on the same row, preserving count checks.
pub(super) fn lane_position(output: &egui::FullOutput, needle: &str) -> Option<egui::Pos2> {
    let (label, count) = needle.split_once(" · ")?;
    if !crate::core::implementation::BOARD_COLUMNS.contains(&label) {
        return None;
    }
    let label_pos = output.shapes.iter().find_map(|shape| match &shape.shape {
        egui::Shape::Text(text) if text.galley.text() == label => {
            Some(text.pos + text.galley.mesh_bounds.center().to_vec2())
        }
        _ => None,
    })?;
    let lane = output.shapes.iter().find_map(|shape| match &shape.shape {
        egui::Shape::Rect(rect)
            if rect.fill == crate::ui::theme::COLUMN && rect.rect.contains(label_pos) =>
        {
            Some(rect.rect)
        }
        _ => None,
    })?;
    output.shapes.iter().find_map(|shape| match &shape.shape {
        egui::Shape::Text(text) if text.galley.text() == count => {
            let pos = text.pos + text.galley.mesh_bounds.center().to_vec2();
            (lane.contains(pos) && pos.x > label_pos.x && (pos.y - label_pos.y).abs() < 5.0)
                .then_some(label_pos)
        }
        _ => None,
    })
}

pub(super) fn styled_context() -> egui::Context {
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    ctx
}

#[test]
fn short_desktop_keeps_blocker_action_visible_and_branch_badge_compact() {
    let mut app = fixture();
    let Screen::Connected(project) = &mut app.screen else {
        unreachable!()
    };
    project.git.branch = "master".into();
    let doc = &mut project.task_documents[0];
    doc.title = "Prove the dual-instance, browse and clone flows end to end and meet the D-34 warning and regression gates".into();
    doc.text = format!(
        "## Purpose\n{}\n\n## Acceptance criteria\n{}",
        "The brief's success criteria couple behaviors across the connected workspace.",
        "- Given the in-repo connected fixture with spawn task evidence preserved\n".repeat(6)
    );
    project.queue.blocked.insert(doc.path.clone(), crate::core::implementation::Failure::other(
        "### Next action(s)\n- ADJUDICATOR: review the preserved evidence and approve the next action."));
    let ctx = styled_context();
    let size = egui::vec2(1280.0, 720.0);
    for _ in 0..3 {
        frame_at(&mut app, &ctx, vec![], size);
    }
    frame_at(&mut app, &ctx, vec![], size);
    let output = click_text_at(&mut app, &ctx, "Prove the dual-instance", size);
    let action = text_position(&output, "Resume implementation")
        .or_else(|| text_position(&output, "Resume after action"))
        .expect("visible blocker action in task details");
    assert!(
        action.y < 620.0,
        "the main action must fit above the fold: {action:?}"
    );
    if let Some(checklist) = text_position(&output, "0 of 6 complete") {
        assert!(
            action.y < checklist.y,
            "the blocker takes priority over checklist detail"
        );
    }
    let branch = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.text() == "master" => {
                Some(egui::Rect::from_min_size(text.pos, text.galley.size()))
            }
            _ => None,
        })
        .expect("branch remains visible in the header");
    assert!(branch.height() <= 30.0 && branch.bottom() < 140.0);
}

#[test]
fn header_keeps_title_controls_and_live_graph_inside_its_own_rows() {
    for width in [360.0, 900.0, 1280.0, 1600.0] {
        let mut app = fixture();
        if let Screen::Connected(project) = &mut app.screen {
            project.state.title =
                "A very long project name with several words to fit safely".into();
        }
        let ctx = styled_context();
        let size = egui::vec2(width, 720.0);
        for _ in 0..3 {
            frame_at(&mut app, &ctx, vec![], size);
        }
        let output = frame_at(&mut app, &ctx, vec![], size);
        let text_rect = |needle: &str| {
            output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) if text.galley.text().starts_with(needle) => {
                        Some(egui::Rect::from_min_size(text.pos, text.galley.size()))
                    }
                    _ => None,
                })
                .unwrap()
        };
        let title = text_rect("A very long project");
        let menu = text_rect("Workspace");
        let activity = text_rect("Live activity");
        assert!(
            !title.intersects(menu),
            "title must not cover workspace controls"
        );
        assert!(title.left() >= 0.0 && title.right() <= width);
        assert!(activity.bottom() < if width < 960.0 { 126.0 } else { 140.0 });
        assert!(activity.top() > title.bottom());
    }
}

#[test]
fn active_work_keeps_equal_lanes_and_uses_the_larger_logo() {
    let mut app = fixture();
    let Screen::Connected(project) = &mut app.screen else {
        unreachable!()
    };
    let ticket = project.task_documents[0].path.clone();
    project.task_documents[0].title = "Prove the dual-instance, browse and clone flows end to end and meet the D-34 warning and regression gates".into();
    project.active_implementations.insert(
        ticket,
        crate::core::implementation::Controller::idle_fixture(),
    );
    let ctx = styled_context();
    let size = egui::vec2(1280.0, 740.0);
    for _ in 0..3 {
        frame_at(&mut app, &ctx, vec![], size);
    }
    let output = frame_at(&mut app, &ctx, vec![], size);
    let lanes: Vec<_> = output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Rect(rect) if rect.fill == crate::ui::theme::COLUMN => Some(rect.rect),
            _ => None,
        })
        .collect();
    assert_eq!(lanes.len(), 5);
    assert!(
        lanes
            .iter()
            .all(|lane| (lane.width() - lanes[0].width()).abs() < 1.0 && lane.right() <= size.x)
    );
    let logo = ctx
        .data_mut(|data| {
            data.get_temp::<egui::TextureHandle>(egui::Id::new("kool_ade_logo_texture"))
        })
        .unwrap();
    let bounds = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Rect(rect) if rect.fill_texture_id() == logo.id() => Some(rect.rect),
            _ => None,
        })
        .unwrap();
    assert!((bounds.width() - 128.0).abs() < 1.0);
    let title = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text)
                if text.galley.text().starts_with("Prove the dual-instance") =>
            {
                Some(&text.galley)
            }
            _ => None,
        })
        .unwrap();
    assert!(title.rows.len() <= 3);
}
