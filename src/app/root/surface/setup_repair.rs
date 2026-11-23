use egui::Ui;

use super::super::KooladeApp;

pub(super) fn paint(
    app: &mut KooladeApp,
    ui: &mut Ui,
    prerequisite_id: &str,
) -> (bool, Option<String>) {
    if crate::app::bootstrap_repair_policy::parse_locked_image_task_id(prerequisite_id).is_some() {
        app.setup_image_preparation.paint(ui, prerequisite_id)
    } else {
        app.setup_remediation.paint(ui, prerequisite_id)
    }
}
