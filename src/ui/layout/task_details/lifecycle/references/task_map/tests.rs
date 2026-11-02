use super::{navigate_back, navigate_to, relation_link};

pub(super) fn lifecycle_view() -> crate::ui::task_detail::LifecycleView {
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
fn planning_work_navigation_preserves_the_returning_task_path() {
    let context = egui::Context::default();
    context.data_mut(|data| {
        data.insert_temp(
            egui::Id::new("koolade_selected_task"),
            "feature/task.md".to_owned(),
        );
    });

    let mut output = context.run_ui(egui::RawInput::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            super::navigate_to_planning(ui, "question:F12".into());
        });
    });
    let selected_work = context.data_mut(|data| {
        data.get_temp::<String>(egui::Id::new(
            "koolade_task_relationship_planning_navigation",
        ))
    });
    let return_path = context.data_mut(|data| {
        data.get_temp::<String>(egui::Id::new("koolade_task_relationship_work_return"))
    });
    output.textures_delta.clear();

    assert_eq!(selected_work.as_deref(), Some("question:F12"));
    assert_eq!(return_path.as_deref(), Some("feature/task.md"));
}

#[test]
fn planning_details_back_button_restores_the_originating_task() {
    let context = egui::Context::default();
    context.data_mut(|data| {
        data.insert_temp(
            egui::Id::new("koolade_task_relationship_work_return"),
            "feature/task.md".to_owned(),
        );
    });
    let mut output = context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(600.0, 400.0),
            )),
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                crate::ui::layout::task_details::planning::paint_relationship_back(ui)
            });
        },
    );
    let position = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.text() == "← Back to task map" => {
                Some(text.pos + text.galley.mesh_bounds.center().to_vec2())
            }
            _ => None,
        })
        .expect("back link is rendered");
    output.textures_delta.clear();
    for pressed in [true, false] {
        let mut output = context.run_ui(pointer_input(position, pressed), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                crate::ui::layout::task_details::planning::paint_relationship_back(ui)
            });
        });
        output.textures_delta.clear();
    }
    let return_path = context.data_mut(|data| {
        data.get_temp::<String>(egui::Id::new("koolade_task_relationship_task_navigation"))
    });
    assert_eq!(return_path.as_deref(), Some("feature/task.md"));
}

#[test]
fn planning_item_relationships_keep_one_hundred_entries_bounded() {
    let records = (0..100)
        .map(|index| crate::ui::task_detail::PlanningItemRelationView {
            planning_item_id: format!("CLR-{index:03}"),
            title: format!("Question {index}"),
            kind: "Question".to_owned(),
            status: "Needs user input".to_owned(),
            blocked_by: Vec::new(),
            legacy_feature_link: false,
            feature_reference_conflict: false,
        })
        .collect::<Vec<_>>();
    let context = egui::Context::default();
    context.data_mut(|data| {
        data.insert_temp(
            egui::Id::new("koolade_task_planning_item_keys"),
            (0..100)
                .map(|index| (format!("CLR-{index:03}"), format!("CLR-{index:03}")))
                .collect::<std::collections::HashMap<_, _>>(),
        );
    });
    let mut output = context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(640.0, 720.0),
            )),
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                super::planning_items::paint(ui, "feature-uid", &records);
            });
        },
    );
    let texts = output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) => Some(text.galley.text().to_owned()),
            _ => None,
        })
        .collect::<Vec<_>>();
    output.textures_delta.clear();

    assert!(
        texts
            .iter()
            .any(|text| text.contains("Questions and decisions · 100 items"))
    );
    assert!(texts.iter().any(|text| text.contains("Question 0")));
}

#[test]
fn missing_planning_item_prerequisites_remain_visible_as_unavailable() {
    let context = egui::Context::default();
    context.data_mut(|data| {
        data.insert_temp(
            egui::Id::new("koolade_task_planning_item_keys"),
            std::collections::HashMap::<String, String>::from([(
                "CLR-001".into(),
                "CLR-001".into(),
            )]),
        );
    });
    let relations = vec![crate::ui::task_detail::PlanningItemRelationView {
        planning_item_id: "CLR-001".into(),
        title: "Choose an access policy".into(),
        kind: "Question".into(),
        status: "Needs user input".into(),
        blocked_by: vec!["CLR-MISSING".into()],
        legacy_feature_link: false,
        feature_reference_conflict: false,
    }];
    let mut output = context.run_ui(egui::RawInput::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            super::planning_items::paint(ui, "feature-uid", &relations);
        });
    });
    let rendered = output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) => Some(text.galley.text().to_owned()),
            _ => None,
        })
        .collect::<Vec<_>>();
    output.textures_delta.clear();

    assert!(
        rendered
            .iter()
            .any(|text| text.contains("Prerequisite unavailable · CLR-MISSING"))
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
