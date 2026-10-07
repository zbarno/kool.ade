//! Secondary dialogs: reference-doc import and stakeholder/identity
//! settings. Business effects (files, commits) run on SAVE only.

use egui::{Layout, RichText, TextEdit};

use crate::app::session::Project;
use crate::artifacts::{CONFIG_FILE, IMPORTS_DIR, atomic_write, config_io, imports_io};
use crate::core::gitops;
use crate::domain::stakeholder::{CategoryOwners, Stakeholders};
use crate::domain::user::{CurrentUser, IdentitySource};
use crate::error::AppError;
use crate::persistence::persona;
use crate::ui::theme;

mod browse;
mod harness_setup;
mod import;
mod mcp;
mod persona_card;
mod settings;

use browse::expand_tilde;
#[cfg(test)]
use browse::{DirRow, resolve_seed};
pub use browse::{DlgBrowse, paint_browse_card};
pub use harness_setup::{
    DlgHarnessSetup, HarnessSettingsSection, paint_harness_setup_card, paint_harness_setup_page,
};
pub use import::DlgImport;
pub use mcp::{DlgMcp, MCP_EXAMPLE_HINT, paint_mcp_card};
pub use persona_card::{
    DlgPersona, PERSONA_SUBORDINATION_NOTICE, PersonaSaveOutcome, paint_persona_card,
};
pub use settings::{
    DlgProjectSettings, DlgSettings, RepositoryNameRow, Row, paint_import_card,
    paint_project_settings_card, paint_settings_card,
};
#[cfg(test)]
use settings::{owner_choices, set_owner_selected};

// ---------------------------------------------------------------------------
fn footers(ui: &mut egui::Ui, feedback: &Option<(bool, String)>) -> (bool, bool) {
    if let Some((ok, msg)) = feedback {
        ui.add_space(8.0);
        ui.label(RichText::new(msg).size(11.5).color(if *ok {
            theme::SUCCESS
        } else {
            theme::DANGER
        }));
    }
    let mut save = false;
    let mut close = false;
    ui.add_space(10.0);
    ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
        let save_btn = ui.add(
            egui::Button::new(RichText::new("Save").strong().color(theme::TEXT))
                .fill(theme::ACCENT_SOFT)
                .corner_radius(6.0),
        );
        if save_btn.clicked() {
            save = true;
        }
        if ui.button(RichText::new("Close").weak()).clicked() {
            close = true;
        }
        ui.add_space(4.0);
    });
    (save, close)
}

fn csv_parts(csv: &str) -> Vec<String> {
    csv.split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

#[cfg(test)]
mod tests;
