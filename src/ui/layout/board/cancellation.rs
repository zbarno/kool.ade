use crate::{
    ui::planning_board::ViewModel,
    ui::{ApplicationCommand, Surface},
};

pub(super) fn confirm(ui: &mut egui::Ui, s: &mut dyn Surface, board: &ViewModel) {
    let id = egui::Id::new("koolade_cancel_pending");
    let Some(key) = ui.ctx().data_mut(|data| data.get_temp::<String>(id)) else {
        return;
    };
    let selected = ui.ctx().data_mut(|data| {
        data.get_temp::<String>(egui::Id::new("koolade_selected_task"))
            .or_else(|| data.get_temp::<String>(egui::Id::new("koolade_selected_planning")))
    });
    if selected.as_deref() == Some(key.as_str()) {
        return;
    }
    let work = board.planning_work.iter().find(|work| work.key == key);
    let task = board.task_documents.iter().find(|doc| doc.path == key);
    let unfinished_children =
        work.and_then(|work| work.feature_id.as_deref())
            .map_or(0, |feature_id| {
                board
                    .task_documents
                    .iter()
                    .filter(|doc| {
                        doc.text
                            .lines()
                            .any(|line| line.strip_prefix("Feature ID: ") == Some(feature_id))
                    })
                    .filter(|doc| {
                        s.implementation_state(&doc.path).is_none_or(|state| {
                            state.status
                                != crate::core::implementation::ImplementationStatus::Completed
                                && state.pr_state
                                    != Some(crate::core::implementation::PullRequestState::Merged)
                        })
                    })
                    .filter(|doc| {
                        !board
                            .cancelled
                            .contains(&crate::persistence::cancelled_work::task_id(doc))
                    })
                    .count()
            });
    let running = task.is_some_and(|doc| s.implementation_active(&doc.path))
        || work.is_some_and(|work| {
            s.active_planning_work() == Some(work.key.as_str())
                || work.feature_id.as_deref().is_some_and(|feature_id| {
                    board.task_documents.iter().any(|doc| {
                        doc.text
                            .lines()
                            .any(|line| line.strip_prefix("Feature ID: ") == Some(feature_id))
                            && s.implementation_active(&doc.path)
                    })
                })
        });
    let mut choice = 0;
    crate::ui::overlays::show_modal(ui, true, "Cancel work?", 430.0, |ui| {
        ui.label(if running {
            "This work is running. Kool.ad/e will request a safe stop; the current step may finish before it stops."
        } else {
            "This work will be removed from the execution queue and will not start again automatically."
        });
        if unfinished_children > 0 {
            ui.label(format!("This feature also has {unfinished_children} unfinished task(s); they will be cancelled with it."));
        }
        ui.label("Its conversations, planning files, implementation history, pull requests, branches, and worktrees will be kept.");
        ui.horizontal(|ui| {
            if ui.button("Keep working").clicked() {
                choice = 1;
            }
            if ui.button("Confirm cancel").clicked() {
                choice = 2;
            }
        });
    });
    if choice == 1 {
        ui.ctx().data_mut(|data| data.remove::<String>(id));
    }
    if choice == 2 {
        s.dispatch(ApplicationCommand::CancelWork { key });
        ui.ctx().data_mut(|data| data.remove::<String>(id));
    }
}
