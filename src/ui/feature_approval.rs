//! Actions come from application state, never from parsing an assistant's prose.

#[derive(Clone, Debug)]
pub struct Action {
    pub id: String,
    pub specification: String,
    pub approved: bool,
    pub prepare_tasks: bool,
}

impl Action {
    pub fn label(&self) -> String {
        match (self.approved, self.prepare_tasks) {
            (true, _) => format!("Prepare tasks for {}", self.id),
            (false, true) => format!("Approve {} and prepare tasks", self.id),
            (false, false) => format!("Approve {} for implementation", self.id),
        }
    }
}

pub fn paint(ui: &mut egui::Ui, actions: &[Action], busy: bool) -> Option<String> {
    let mut selected = None;
    for action in actions {
        ui.push_id((&action.id, "feature_approval"), |ui| {
            super::theme::card_frame().show(ui, |ui| {
                ui.collapsing(format!("Review {} specification", action.id), |ui| {
                    egui::ScrollArea::vertical()
                        .max_height(180.0)
                        .show(ui, |ui| {
                            super::markdown::paint(
                                ui,
                                &action.specification,
                                super::markdown::CHAT,
                            );
                        });
                });
                if ui
                    .add_enabled(!busy, egui::Button::new(action.label()))
                    .clicked()
                {
                    selected = Some(action.id.clone());
                }
                if action.prepare_tasks {
                    ui.label("Refreshes the task plan if needed, then generates task stories.");
                }
            });
        });
    }
    selected
}
