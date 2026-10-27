use egui::Ui;

use super::super::KooladeApp;

pub(super) fn paint(
    app: &mut KooladeApp,
    ui: &mut Ui,
    prerequisite_id: &str,
) -> (bool, Option<String>) {
    let is_general_image = prerequisite_id
        .strip_prefix("setup:images:general:")
        .is_some_and(|architecture| matches!(architecture, "amd64" | "arm64"));
    if is_general_image {
        app.setup_image_preparation.paint(ui, prerequisite_id)
    } else {
        app.setup_remediation.paint(ui, prerequisite_id)
    }
}
