mod automation;
use super::*;
use crate::app::dialogs::HarnessSettingsSection;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
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
    for (group, pages) in [
        ("APPLICATION", &Page::ALL[..2]),
        ("WORK & TOOLS", &Page::ALL[3..6]),
        ("PROJECT", &[Page::ALL[2], Page::ALL[6]][..]),
    ] {
        ui.label(theme::metadata_text(group));
        ui.add_space(4.0);
        for &(page, label) in pages {
            let active = *selected == page;
            if ui
                .add_sized(
                    [ui.available_width(), 36.0],
                    egui::Button::new(egui::RichText::new(label).size(13.0).color(if active {
                        theme::TEXT
                    } else {
                        theme::TEXT_DIM
                    }))
                    .selected(active),
                )
                .clicked()
            {
                *selected = page;
            }
        }
        ui.add_space(theme::spacing::L);
    }
}

pub(super) fn paint_compact_navigation(ui: &mut egui::Ui, selected: &mut Page) {
    let label = Page::ALL
        .iter()
        .find(|(page, _)| page == selected)
        .unwrap()
        .1;
    egui::ComboBox::from_id_salt("settings_page_selector")
        .selected_text(label)
        .width(ui.available_width())
        .show_ui(ui, |ui| {
            for (page, label) in Page::ALL {
                ui.selectable_value(selected, page, label);
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
        Page::General => paint_general(ui, surface),
        Page::Appearance => paint_appearance(ui),
        Page::ProjectGit => paint_project_git(ui, surface),
        Page::CodingTools => paint_harness(ui, HarnessSettingsSection::Tools),
        Page::ModelsRouting => paint_harness(ui, HarnessSettingsSection::Routing),
        Page::Automation => automation::paint(ui, surface, board),
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
        if discard {
            data.insert_temp(egui::Id::new("koolade_settings_close"), true);
        }
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
        if discard {
            data.insert_temp(egui::Id::new("koolade_settings_close"), true);
        }
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

fn paint_general(ui: &mut egui::Ui, surface: &mut dyn Surface) -> Option<String> {
    ui.label(theme::page_title("General"));
    ui.label(theme::helper_text(
        "Choose how your planning assistant communicates.",
    ));
    ui.add_space(theme::spacing::L);
    super::paint_persona_section(ui, surface);
    None
}
