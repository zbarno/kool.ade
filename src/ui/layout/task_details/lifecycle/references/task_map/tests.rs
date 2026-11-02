use super::{navigate_back, navigate_to, node_status_text, relation_link};

fn lifecycle_view() -> crate::ui::task_detail::LifecycleView {
    use crate::core::lifecycle::{FeaturePhase, SpecReconciliationStatusV1, TaskPhase};
    crate::ui::task_detail::LifecycleView {
        feature_uid: "feature-uid".into(),
        feature_phase: FeaturePhase::Executing,
        task_phase: TaskPhase::Running,
        parent_task_uid: None,
        parent_task: None,
        dependencies: Vec::new(),
        subtasks: vec![crate::ui::task_detail::TaskRelationView {
            planning_item_uid: "child-uid".into(),
            lifecycle_task_uid: Some("child-task-uid".into()),
            title: "Child relationship node".into(),
            status: "Running".into(),
            acceptance_criteria: Vec::new(),
        }],
        task_revision: 1,
        code_review: None,
        code_review_attempts: Vec::new(),
        current_code_review: None,
        code_review_dispositions: Vec::new(),
        code_review_load_error: None,
        runtime_qa: None,
        runtime_qa_run_id: None,
        spec_reconciliation: None,
        current_spec_hash: None,
        implementation_head_sha: None,
        approved_target_hash: None,
        deployed_spec_hash: None,
        deployment_status: None,
        deployment_observed_at_ms: None,
        spec_revision: None,
        spec_status: Some(SpecReconciliationStatusV1::Synchronized),
        pending_spec_triggers: 0,
        specification_deltas: Vec::new(),
        pull_request_status: None,
        merged_commit_sha: None,
        runtime_qa_evidence: None,
        runtime_qa_evidence_error: None,
        runtime_qa_applicability: None,
    }
}

fn pointer_input(position: egui::Pos2, pressed: bool) -> egui::RawInput {
    egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(600.0, 400.0),
        )),
        events: vec![
            egui::Event::PointerMoved(position),
            egui::Event::PointerButton {
                pos: position,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Default::default(),
            },
        ],
        ..Default::default()
    }
}

#[test]
fn related_node_statuses_keep_exceptions_visible_without_color() {
    assert_eq!(node_status_text("Running"), "● Running");
    assert_eq!(node_status_text("Blocked"), "⚠ Blocked");
    assert_eq!(
        node_status_text("Waiting for your decision"),
        "✋ Needs user input"
    );
    assert_eq!(node_status_text("Cancelled"), "× Cancelled");
    assert_eq!(node_status_text("Failed"), "! Failed");
    assert_eq!(
        node_status_text("Implementation complete · PR awaiting authorization"),
        "◷ Implementation complete · PR awaiting authorization"
    );
    assert_eq!(
        node_status_text("Implementation complete · PR merged"),
        "✓ Implementation complete · PR merged"
    );
    assert_eq!(node_status_text("Waiting for review"), "◷ Awaiting review");
    assert_eq!(
        node_status_text("Waiting for capacity"),
        "… Waiting for capacity"
    );
    assert_eq!(
        node_status_text("Status not recorded"),
        "? Status not recorded"
    );
}

#[test]
fn task_map_relationships_fit_narrow_and_wide_viewports() {
    let lifecycle = lifecycle_view();
    for width in [360.0, 1280.0] {
        let context = egui::Context::default();
        context.data_mut(|data| {
            data.insert_temp(
                egui::Id::new("koolade_task_titles_by_path"),
                std::collections::HashMap::from([(
                    "task.md".to_owned(),
                    "Selected task".to_owned(),
                )]),
            );
            data.insert_temp(
                egui::Id::new("koolade_task_paths_by_uid"),
                std::collections::HashMap::from([("child-uid".to_owned(), "child.md".to_owned())]),
            );
        });
        let mut surface = None;
        let mut output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(width, 720.0),
                )),
                ..Default::default()
            },
            |context| {
                egui::CentralPanel::default().show(context, |ui| {
                    super::relationships(ui, &mut surface, &lifecycle, "task.md");
                });
            },
        );
        let texts = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Text(text) => {
                    Some((text.galley.text().to_owned(), text.pos, text.galley.size()))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(texts.iter().any(|(text, _, _)| text == "Selected task"));
        assert!(
            texts
                .iter()
                .any(|(text, _, _)| text.contains("Child relationship node"))
        );
        assert!(
            texts
                .iter()
                .all(|(_, pos, size)| pos.x >= 0.0 && pos.x + size.x <= width)
        );
        output.textures_delta.clear();
    }
}

#[test]
fn large_sibling_map_reports_all_direct_nodes_in_a_bounded_section() {
    let mut lifecycle = lifecycle_view();
    lifecycle.subtasks = (0..100)
        .map(|index| crate::ui::task_detail::TaskRelationView {
            planning_item_uid: format!("child-{index}"),
            lifecycle_task_uid: Some(format!("child-task-{index}")),
            title: format!("Child {index}"),
            status: "Running".into(),
            acceptance_criteria: Vec::new(),
        })
        .collect();
    let context = egui::Context::default();
    let mut surface = None;
    let mut output = context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(640.0, 720.0),
            )),
            ..Default::default()
        },
        |context| {
            egui::CentralPanel::default().show(context, |ui| {
                super::relationships(ui, &mut surface, &lifecycle, "task.md");
            });
        },
    );
    let rendered = output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) => Some(text.galley.text().to_owned()),
            _ => None,
        })
        .collect::<Vec<_>>();
    let has_full_child_count = rendered
        .iter()
        .any(|text| text.contains("Children · 100 subtasks"));
    let first_child_is_visible = rendered.iter().any(|text| text.contains("Child 0"));
    output.textures_delta.clear();
    assert!(
        has_full_child_count,
        "the section reports all 100 direct children"
    );
    assert!(
        first_child_is_visible,
        "the bounded viewport shows its first child"
    );
    assert_eq!(lifecycle.subtasks.len(), 100);
}

#[test]
fn clicking_a_recorded_task_node_requests_its_own_modal_path() {
    let context = egui::Context::default();
    let current_path = "parent.md".to_owned();
    let target_path = "child.md".to_owned();
    let task_uid = "child-planning-uid".to_owned();
    let paths_id = egui::Id::new("koolade_task_paths_by_uid");
    let selected_id = egui::Id::new("koolade_selected_task");
    let history_id = egui::Id::new("koolade_task_relationship_history");
    let navigation_id = egui::Id::new("koolade_task_relationship_navigation");
    context.data_mut(|data| {
        data.insert_temp(
            paths_id,
            std::collections::HashMap::from([(task_uid.clone(), target_path.clone())]),
        );
        data.insert_temp(selected_id, current_path.clone());
    });
    let relation = crate::ui::task_detail::TaskRelationView {
        planning_item_uid: task_uid,
        lifecycle_task_uid: Some("child-lifecycle-uid".into()),
        title: "Child task".into(),
        status: "Running".into(),
        acceptance_criteria: Vec::new(),
    };
    let mut initial = context.run_ui(egui::RawInput::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            relation_link(ui, "Subtask", &relation);
        });
    });
    let center = initial
        .shapes
        .iter()
        .find_map(|shape| {
            let egui::Shape::Text(text) = &shape.shape else {
                return None;
            };
            text.galley
                .text()
                .contains("Subtask Child task")
                .then(|| text.pos + text.galley.mesh_bounds.center().to_vec2())
        })
        .expect("related task node is rendered as a link");
    initial.textures_delta.clear();
    for pressed in [true, false] {
        let mut frame = context.run_ui(pointer_input(center, pressed), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                relation_link(ui, "Subtask", &relation);
            });
        });
        frame.textures_delta.clear();
    }
    assert_eq!(
        context.data_mut(|data| data.get_temp::<String>(navigation_id)),
        Some(target_path)
    );
    assert_eq!(
        context.data_mut(|data| data.get_temp::<Vec<String>>(history_id)),
        Some(vec![current_path])
    );
}

#[test]
fn task_navigation_pushes_current_path_and_back_restores_previous_path() {
    let context = egui::Context::default();
    let selected = egui::Id::new("koolade_selected_task");
    let history = egui::Id::new("koolade_task_relationship_history");
    let navigation = egui::Id::new("koolade_task_relationship_navigation");
    context.data_mut(|data| data.insert_temp(selected, "original.md".to_owned()));
    let mut frame = context.run_ui(Default::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| navigate_to(ui, "child.md".into()));
    });
    assert_eq!(
        context.data_mut(|data| data.get_temp::<Vec<String>>(history)),
        Some(vec!["original.md".to_owned()])
    );
    assert_eq!(
        context.data_mut(|data| data.get_temp::<String>(navigation)),
        Some("child.md".to_owned())
    );
    frame.textures_delta.clear();
    context.data_mut(|data| data.insert_temp(selected, "child.md".to_owned()));
    let _ = context.run_ui(Default::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| navigate_back(ui, "original.md".into()));
    });
    assert!(
        context
            .data_mut(|data| data.get_temp::<Vec<String>>(history))
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        context.data_mut(|data| data.get_temp::<String>(navigation)),
        Some("original.md".to_owned())
    );
}
