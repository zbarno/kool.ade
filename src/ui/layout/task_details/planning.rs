use super::*;

mod conversation;
mod details;

pub(super) fn paint_item(
    ui: &mut egui::Ui,
    surface: &mut dyn Surface,
    board: &crate::ui::planning_board::ViewModel,
    item: &crate::domain::item::OpenItem,
    height: f32,
) {
    let key = item.conversation_key();
    details::paint_panes(
        ui,
        surface,
        key,
        height,
        details::PlanningDetails::Item(item, &board.planning_items),
    );
}

pub(super) fn paint_work(
    ui: &mut egui::Ui,
    surface: &mut dyn Surface,
    board: &crate::ui::planning_board::ViewModel,
    work: &crate::core::planning_work::Work,
    height: f32,
) {
    let key = work.key.as_str();
    details::paint_panes(
        ui,
        surface,
        key,
        height,
        details::PlanningDetails::Work(work, &board.planning_work),
    );
}

pub(super) fn paint_setup_issue(
    ui: &mut egui::Ui,
    surface: &mut dyn Surface,
    board: &crate::ui::planning_board::ViewModel,
    issue: &crate::app::setup_attention::SetupIssue,
) {
    ui.heading(issue.title);
    ui.label(crate::ui::theme::metadata_text("Setup · Needs your input"));
    ui.add_space(crate::ui::theme::spacing::M);
    ui.label(crate::ui::theme::section_heading("What needs attention"));
    ui.label(&issue.issue);
    ui.collapsing("Why this matters", |ui| ui.label(issue.why));
    ui.label(crate::ui::theme::section_heading("Recommended next step"));
    ui.label(issue.recommendation);
    ui.label(crate::ui::theme::helper_text(issue.impact));
    ui.label(issue.next_action);
    if ui.button("Open workspace settings").clicked() {
        ui.ctx().data_mut(|data| {
            data.insert_temp(egui::Id::new("koolade_workspace_settings_open"), true);
        });
    }
    let label = if board.setup_checking {
        "Checking setup…"
    } else {
        "Retry setup check"
    };
    if ui
        .add_enabled(
            !board.setup_checking,
            egui::Button::new(label).fill(crate::ui::theme::ACTION),
        )
        .clicked()
    {
        surface.dispatch(crate::ui::ApplicationCommand::RetrySetupCheck);
    }
}

pub(super) fn paint_feature_approval(
    ui: &mut egui::Ui,
    surface: &mut dyn Surface,
    action: &crate::ui::feature_approval::Action,
) {
    ui.heading(&action.id);
    ui.label(crate::ui::theme::metadata_text(
        "Specification · Needs approval",
    ));
    ui.add_space(crate::ui::theme::spacing::M);
    ui.collapsing("Specification", |ui| {
        crate::ui::markdown::paint(ui, &action.specification, crate::ui::markdown::CHAT);
    });
    if action
        .plan_comparison
        .as_ref()
        .is_some_and(|comparison| comparison.selected_plan.is_none())
    {
        match crate::ui::feature_approval::paint_comparison(ui, action) {
            Some(crate::ui::feature_approval::ComparisonIntent::Adopt(plan_id)) => surface
                .dispatch(crate::ui::ApplicationCommand::ChooseFeaturePlan {
                    id: action.id.clone(),
                    plan_id,
                }),
            Some(crate::ui::feature_approval::ComparisonIntent::Discard) => {
                surface.dispatch(crate::ui::ApplicationCommand::DiscardFeaturePlans {
                    id: action.id.clone(),
                })
            }
            None => {}
        }
    } else if ui
        .add_enabled(
            !surface.conversation_busy(),
            egui::Button::new(action.label()).fill(crate::ui::theme::ACTION),
        )
        .clicked()
    {
        surface.dispatch(crate::ui::ApplicationCommand::ApproveFeatureForBoard {
            id: action.id.clone(),
        });
        ui.ctx().data_mut(|data| {
            data.insert_temp(egui::Id::new("koolade_task_details_close"), true);
        });
    }
}
