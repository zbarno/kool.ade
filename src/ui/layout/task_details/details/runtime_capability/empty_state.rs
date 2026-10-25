use crate::ui::task_detail::RuntimeTargetChoice;

pub(super) fn paint_not_assessed(ui: &mut egui::Ui) {
    ui.label(egui::RichText::new("Runtime QA applicability: not assessed yet").strong());
    ui.label(
        egui::RichText::new(
            "No project target has been assessed for this source revision. Planning, implementation, and static code review can continue; discover targets to determine runtime QA applicability.",
        )
        .small(),
    );
}

pub(super) fn paint_static_review_only_if_empty(
    ui: &mut egui::Ui,
    targets: &[RuntimeTargetChoice],
) -> bool {
    if !targets.is_empty() {
        return false;
    }
    ui.label(egui::RichText::new("Runtime QA applicability: static review only").strong());
    ui.label(
        egui::RichText::new(
            "No application targets were found for this task. Planning, implementation, and static code review can continue; add an approved custom target if the project contains an app to enable runtime QA.",
        )
        .small(),
    );
    true
}
