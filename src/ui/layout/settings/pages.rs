use super::*;
use crate::app::dialogs::HarnessSettingsSection;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Page {
    #[default]
    General,
    Appearance,
    ProjectGit,
    CodingTools,
    ModelsRouting,
    Automation,
    People,
}

impl Page {
    const ALL: [(Self, &'static str); 7] = [
        (Self::General, "General"),
        (Self::Appearance, "Appearance"),
        (Self::ProjectGit, "Project & Git"),
        (Self::CodingTools, "Coding Tools"),
        (Self::ModelsRouting, "Models & Routing"),
        (Self::Automation, "Automation"),
        (Self::People, "People & Stakeholders"),
    ];

    pub(super) fn from_key(key: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .find_map(|(page, label)| (*label == key).then_some(*page))
    }
}

pub(super) fn paint_navigation(ui: &mut egui::Ui, selected: &mut Page) {
    ui.label(theme::metadata_text("SETTINGS"));
    ui.add_space(theme::spacing::S);
    for (page, label) in Page::ALL {
        let active = *selected == page;
        let response = ui.add_sized(
            [ui.available_width(), 36.0],
            egui::Button::new(if active {
                theme::section_heading(label)
            } else {
                theme::helper_text(label)
            })
            .selected(active),
        );
        if response.clicked() {
            *selected = page;
        }
    }
}

pub(super) fn paint_compact_navigation(ui: &mut egui::Ui, selected: &mut Page) {
    ui.label(theme::metadata_text("SETTINGS"));
    ui.horizontal_wrapped(|ui| {
        for (page, label) in Page::ALL {
            if ui.selectable_label(*selected == page, label).clicked() {
                *selected = page;
            }
        }
    });
}

pub(super) fn paint_page(
    ui: &mut egui::Ui,
    surface: &mut dyn Surface,
    board: &crate::ui::planning_board::ViewModel,
    page: Page,
) -> Option<String> {
    match page {
        Page::General => paint_general(ui, surface, board),
        Page::Appearance => paint_appearance(ui),
        Page::ProjectGit => paint_project_git(ui, surface),
        Page::CodingTools => paint_harness(ui, HarnessSettingsSection::Tools),
        Page::ModelsRouting => paint_harness(ui, HarnessSettingsSection::Routing),
        Page::Automation => paint_automation(ui, surface),
        Page::People => paint_people(ui, surface),
    }
}

fn paint_appearance(ui: &mut egui::Ui) -> Option<String> {
    ui.label(theme::page_title("Appearance"));
    ui.add_space(theme::spacing::M);
    let reduced_id = egui::Id::new(theme::REDUCE_MOTION_ID);
    let mut reduced = ui
        .ctx()
        .data_mut(|data| data.get_temp::<bool>(reduced_id).unwrap_or(false));
    if ui.checkbox(&mut reduced, "Reduce motion").changed() {
        ui.ctx()
            .data_mut(|data| data.insert_temp(reduced_id, reduced));
    }
    ui.label(theme::helper_text(
        "Stops transitions and animated busy indicators. Live activity updates continue.",
    ));
    ui.ctx().style_mut_of(egui::Theme::Dark, |style| {
        style.animation_time = if reduced { 0.0 } else { 0.2 };
    });
    None
}

fn paint_project_git(ui: &mut egui::Ui, surface: &mut dyn Surface) -> Option<String> {
    ui.label(theme::page_title("Project & Git"));
    let id = egui::Id::new("koolade_settings_project_git_draft");
    let mut draft = ui
        .ctx()
        .data_mut(|data| data.remove_temp::<crate::app::dialogs::DlgProjectSettings>(id))
        .or_else(|| surface.project_settings_draft());
    let Some(mut draft) = draft.take() else {
        ui.label("Project settings are unavailable.");
        return None;
    };
    let (save, discard) = crate::app::dialogs::paint_project_settings_card(ui, &mut draft);
    if save && let Err(error) = surface.save_project_settings(&mut draft) {
        draft.feedback = Some((false, error));
    }
    ui.ctx().data_mut(|data| {
        if !discard {
            data.insert_temp(id, draft);
        }
    });
    None
}

fn paint_people(ui: &mut egui::Ui, surface: &mut dyn Surface) -> Option<String> {
    ui.label(theme::page_title("People & Stakeholders"));
    let id = egui::Id::new("koolade_settings_people_draft");
    let mut draft = ui
        .ctx()
        .data_mut(|data| data.remove_temp::<crate::app::dialogs::DlgSettings>(id))
        .or_else(|| surface.people_settings_draft());
    let Some(mut draft) = draft.take() else {
        ui.label("People settings are unavailable.");
        return None;
    };
    let (save, discard) = crate::app::dialogs::paint_settings_card(ui, &mut draft);
    if save {
        match surface.save_people_settings(&mut draft) {
            Ok(_) => draft = surface.people_settings_draft().unwrap_or(draft),
            Err(error) => draft.feedback = Some((false, error)),
        }
    }
    ui.ctx().data_mut(|data| {
        if !discard {
            data.insert_temp(id, draft);
        }
    });
    None
}

fn paint_harness(ui: &mut egui::Ui, section: HarnessSettingsSection) -> Option<String> {
    let id = egui::Id::new("koolade_settings_harness_draft");
    let mut draft = ui
        .ctx()
        .data_mut(|data| data.remove_temp::<crate::app::dialogs::DlgHarnessSetup>(id))
        .unwrap_or_default();
    crate::app::dialogs::paint_harness_setup_page(ui, &mut draft, section);
    ui.ctx().data_mut(|data| data.insert_temp(id, draft));
    None
}

fn paint_automation(ui: &mut egui::Ui, surface: &mut dyn Surface) -> Option<String> {
    ui.label(theme::page_title("Automation"));
    ui.add_space(theme::spacing::M);
    ui.label("Concurrency");
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
    None
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

fn paint_general(
    ui: &mut egui::Ui,
    surface: &mut dyn Surface,
    board: &crate::ui::planning_board::ViewModel,
) -> Option<String> {
    ui.label(theme::page_title("General"));
    ui.add_space(theme::spacing::M);
    ui.label(theme::section_heading("Planner persona"));
    super::paint_persona_section(ui, surface);
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
