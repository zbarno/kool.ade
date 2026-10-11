use crate::ui::{ApplicationCommand, PlanningRepositoryCommand, Surface};

pub(super) fn paint_cancel_modal(ui: &mut egui::Ui, surface: &mut dyn Surface) {
    let id = egui::Id::new("planning_cancel_pending");
    let operation = ui.ctx().data_mut(|data| data.get_temp::<uuid::Uuid>(id));
    let Some(operation_id) = operation else {
        return;
    };
    let confirm = std::cell::Cell::new(false);
    let dismiss = std::cell::Cell::new(false);
    let closed = crate::ui::overlays::show_modal(
        ui,
        true,
        "Cancel pending planning operation?",
        500.0,
        |ui| {
            ui.label("Only this uncommitted local operation can be removed. Committed operations remain in history.");
            ui.horizontal(|ui| {
                if ui.button("Keep operation").clicked() {
                    dismiss.set(true);
                }
                if ui.button("Cancel operation").clicked() {
                    confirm.set(true);
                }
            });
        },
    );
    if confirm.get() {
        surface.dispatch(ApplicationCommand::PlanningRepository(
            PlanningRepositoryCommand::CancelPendingOperation { operation_id },
        ));
        ui.ctx().data_mut(|data| data.remove_temp::<uuid::Uuid>(id));
    } else if closed || dismiss.get() {
        ui.ctx().data_mut(|data| data.remove_temp::<uuid::Uuid>(id));
    }
}
