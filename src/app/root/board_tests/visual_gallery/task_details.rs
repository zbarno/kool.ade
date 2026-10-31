//! Production modal visual proof, using synthetic task state and no AI workers.
//! The GPU render verifies layout that state-only unit tests cannot establish.
//! Dependencies: local wgpu adapter; setup cost: one adapter plus eight frames.
use super::*;

#[test]
#[ignore = "GPU screenshot review; requires isolated KOOLADE_HOME and KOOLADE_UI_REVIEW_DIR"]
fn render_simplified_task_details_reference() {
    assert!(std::env::var_os("KOOLADE_HOME").is_some());
    let dir = std::path::PathBuf::from(std::env::var("KOOLADE_UI_REVIEW_DIR").unwrap());
    std::fs::create_dir_all(&dir).unwrap();
    for size in [[1280, 820], [900, 720], [360, 720]] {
        let mut gallery = Gallery::new(size, dir.clone());
        let key = "planning/tasks/fixture/001-task.md";
        gallery.app.attention_fixture.insert(key.into(), crate::core::attention::Brief {
            problem: "Should I update the plan to use the new default?".into(),
            recommendation: None,
            options: [("update", "Yes, update it", "Use the new default in the plan."),
                ("keep", "No, leave it as-is", "Keep the current plan unchanged.")]
                .into_iter().map(|(id, label, meaning)| crate::core::attention::OptionBrief {
                    id: id.into(), label: label.into(), meaning: meaning.into(),
                    consequence: meaning.into(), clarification: false, source_evidence: None,
                }).collect(),
            steps: vec![],
            after: "A related issue was resolved. Now Kool.ad/e needs to know whether it should write the new default directly into the plan.".into(),
        });
        if let Screen::Connected(project) = &mut gallery.app.screen {
            project.queue.blocked.insert(
                key.into(),
                crate::core::implementation::Failure::other(
                    "A related issue was resolved; a plan update needs your answer.",
                ),
            );
        }
        let Screen::Connected(project) = &mut gallery.app.screen else {
            unreachable!()
        };
        project.task_chats.messages.insert(
            key.into(),
            vec![
                ChatMessage::new(
                    ChatRole::Agent,
                    "I need your choice before I continue.",
                    None,
                ),
                ChatMessage::new(ChatRole::User, "What does this change do?", None),
                ChatMessage::new(
                    ChatRole::Agent,
                    "It updates the plan to use the default we already settled on.",
                    None,
                ),
            ],
        );
        gallery.ctx.data_mut(|data| {
            data.insert_temp(egui::Id::new("koolade_selected_task"), key.to_owned())
        });
        gallery.save("simplified-task-details");
        if size[0] < 760 {
            gallery.scroll_to_bottom();
            gallery.save("simplified-task-details-scrolled");
        }
        if size[0] == 1280 {
            if let Screen::Connected(project) = &mut gallery.app.screen {
                project.active_implementations.insert(
                    key.into(),
                    crate::core::implementation::Controller::idle_fixture(),
                );
            }
            gallery.save("simplified-task-working");
            if let Screen::Connected(project) = &mut gallery.app.screen {
                project.active_implementations.remove(key);
            }
            let brief = gallery.app.attention_fixture.get_mut(key).unwrap();
            brief.options.clear();
            brief.problem = "The project check could not finish.".into();
            brief.after = "Review the saved error and retry when the check is available.".into();
            gallery.save("simplified-task-failed");
            if let Screen::Connected(project) = &mut gallery.app.screen {
                project.queue.blocked.insert(
                    key.into(),
                    crate::core::implementation::Failure::new(
                        crate::core::implementation::FailureKind::ExternalPrerequisite,
                        crate::core::implementation::RecoveryDisposition::UserAction,
                        "The required project service is unavailable.",
                    ),
                );
            }
            gallery.save("simplified-task-blocked");
            gallery.ctx.data_mut(|data| {
                data.insert_temp(
                    egui::Id::new("koolade_selected_task"),
                    "planning/tasks/fixture/003-task.md".to_owned(),
                )
            });
            gallery.save("simplified-task-completed");
        }
    }
}
