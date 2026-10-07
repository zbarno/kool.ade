use super::*;

pub(super) fn paint(
    ui: &mut egui::Ui,
    surface: &mut dyn Surface,
    board: &crate::ui::planning_board::ViewModel,
) -> Option<String> {
    ui.label(theme::page_title("Automation"));
    ui.add_space(theme::spacing::M);
    ui.label(theme::helper_text(
        "Control when work starts and how verified changes are shared.",
    ));
    ui.add_space(theme::spacing::M);
    ui.label(theme::section_heading("Worker capacity"));
    let mut parallel = surface.max_parallel_tasks();
    if ui
        .add(egui::Slider::new(&mut parallel, 1..=8).text("Concurrent tasks"))
        .changed()
    {
        surface.dispatch(ApplicationCommand::SetMaxParallelTasks { count: parallel });
    }
    ui.label(theme::helper_text(format!(
        "{} workers active. Dependencies must merge before dependent tasks start.",
        surface.active_task_count()
    )));
    ui.add_space(theme::spacing::L);
    ui.label(theme::section_heading("Automatic workflow"));
    setting_toggle(
        ui,
        "Plan automatically",
        surface.auto_plan(),
        |enabled| ApplicationCommand::SetAutoPlan { enabled },
        surface,
    );
    setting_toggle(
        ui,
        "Build approved changes automatically",
        surface.auto_build(),
        |enabled| ApplicationCommand::SetAutoBuild { enabled },
        surface,
    );
    setting_toggle(
        ui,
        "Publish verified changes automatically",
        surface.auto_publish(),
        |enabled| ApplicationCommand::SetAutoPublish { enabled },
        surface,
    );
    ui.add_space(theme::spacing::L);
    ui.label(theme::section_heading("Publication checks"));
    let mut checks = surface.require_independent_checks();
    if ui
        .add_enabled(
            !surface.auto_publish(),
            egui::Checkbox::new(&mut checks, "Wait for project checks before publishing"),
        )
        .changed()
    {
        surface.dispatch(ApplicationCommand::SetRequireIndependentChecks { enabled: checks });
    }
    ui.label(theme::helper_text("Kool.ad/e runs its checks first. Publishing waits for separate project checks when enabled."));
    paint_queue(ui, surface, board)
}

fn setting_toggle(
    ui: &mut egui::Ui,
    label: &str,
    current: bool,
    command: impl FnOnce(bool) -> ApplicationCommand,
    surface: &mut dyn Surface,
) {
    let mut enabled = current;
    if ui.checkbox(&mut enabled, label).changed() {
        surface.dispatch(command(enabled));
    }
}

fn paint_queue(
    ui: &mut egui::Ui,
    surface: &mut dyn Surface,
    board: &crate::ui::planning_board::ViewModel,
) -> Option<String> {
    if !surface.queue_status().is_empty() {
        ui.separator();
        ui.label(theme::section_heading("Queue status"));
        ui.label(surface.queue_status());
    }
    if let Some((ticket, claim)) = surface.stale_task_claim() {
        ui.separator();
        ui.label(theme::section_heading("Stale task claim"));
        ui.label(format!(
            "{} is held by {} since {} on base {}.",
            ticket,
            claim.owner,
            chrono::DateTime::from_timestamp(claim.claimed_at, 0)
                .map(|time| time.to_rfc3339())
                .unwrap_or_else(|| claim.claimed_at.to_string()),
            claim.base_commit
        ));
        if ui
            .button("I confirmed the old worker stopped — take over stale claim")
            .clicked()
        {
            surface.dispatch(ApplicationCommand::TakeOverStaleTaskClaim { ticket });
        }
    }
    for doc in board
        .task_documents
        .iter()
        .filter(|doc| doc.path.ends_with("/README.md"))
    {
        ui.separator();
        if ui
            .button(format!("Batch overview · {}", doc.title))
            .clicked()
        {
            return Some(doc.path.clone());
        }
    }
    None
}
