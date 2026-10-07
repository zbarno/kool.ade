use super::*;
use crate::app::dialogs;

mod pages;

pub(super) fn paint(
    ui: &mut egui::Ui,
    s: &mut dyn Surface,
    board: &crate::ui::planning_board::ViewModel,
    mut settings_open: bool,
    settings_was_open: bool,
) -> bool {
    let settings_id = egui::Id::new("koolade_workspace_settings_open");
    let defer = ui.ctx().data_mut(|data| {
        data.remove_temp::<bool>(egui::Id::new("koolade_defer_settings_one_frame"))
            .unwrap_or(false)
    });
    if defer {
        ui.ctx()
            .data_mut(|data| data.insert_temp(settings_id, true));
        return true;
    }
    settings_open |= ui
        .ctx()
        .data_mut(|data| data.get_temp::<bool>(settings_id).unwrap_or(false));
    if settings_open {
        let page_id = egui::Id::new("koolade_settings_page");
        let requested_page = ui.ctx().data_mut(|data| {
            data.remove_temp::<String>(egui::Id::new("koolade_settings_open_page"))
        });
        let mut page = requested_page
            .as_deref()
            .and_then(pages::Page::from_key)
            .unwrap_or_else(|| {
                ui.ctx()
                    .data_mut(|data| data.get_temp::<pages::Page>(page_id).unwrap_or_default())
            });
        let mut open_batch = None;
        let closed =
            crate::ui::overlays::show_settings_modal(ui, true, "Workspace settings", |ui| {
                let height = ui.available_height().max(80.0);
                let compact = ui.available_width() < 720.0 || height < 450.0;
                ui.set_min_height(height);
                if compact {
                    pages::paint_compact_navigation(ui, &mut page);
                    ui.separator();
                    egui::ScrollArea::vertical()
                        .id_salt(("settings_selected_page", page))
                        .max_height(ui.available_height())
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            open_batch = pages::paint_page(ui, s, board, page);
                        });
                } else {
                    ui.horizontal_top(|ui| {
                        ui.allocate_ui_with_layout(
                            egui::vec2(190.0, height),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| pages::paint_navigation(ui, &mut page),
                        );
                        ui.separator();
                        egui::ScrollArea::vertical()
                            .id_salt(("settings_selected_page", page))
                            .max_height(height)
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                                    ui.set_min_width(ui.available_width());
                                    open_batch = pages::paint_page(ui, s, board, page);
                                });
                            });
                    });
                }
            });
        ui.ctx().data_mut(|data| data.insert_temp(page_id, page));
        if closed
            || ui.ctx().data_mut(|data| {
                data.remove_temp::<bool>(egui::Id::new("koolade_settings_close"))
                    .unwrap_or(false)
            })
        {
            settings_open = false;
        }
        if let Some(path) = open_batch {
            settings_open = false;
            ui.ctx().data_mut(|data| {
                data.insert_temp(egui::Id::new("koolade_document_tab"), true);
                data.insert_temp(egui::Id::new("koolade_selected_task"), path);
            });
        }
    }
    if settings_was_open && !settings_open {
        ui.ctx().data_mut(|data| {
            data.remove_temp::<bool>(egui::Id::new("koolade_persona_was_closed"));
            data.remove_temp::<dialogs::DlgPersona>(egui::Id::new("koolade_persona_card"));
            data.remove_temp::<crate::app::dialogs::DlgHarnessSetup>(egui::Id::new(
                "koolade_settings_harness_draft",
            ));
            data.remove_temp::<crate::app::dialogs::DlgProjectSettings>(egui::Id::new(
                "koolade_settings_project_git_draft",
            ));
            data.remove_temp::<crate::app::dialogs::DlgSettings>(egui::Id::new(
                "koolade_settings_people_draft",
            ));
        });
    }
    ui.ctx()
        .data_mut(|data| data.insert_temp(settings_id, settings_open));
    settings_open
}

/// Planner persona section inside the Workspace settings modal (host glue
/// only — the card model and painter live in [`crate::app::dialogs`]).
///
/// Lifecycle (bind-on-open):
/// * the FIRST entry into a modal open-cycle constructs
///   `DlgPersona::from_load(&persona::load_persona())` — that load IS the
///   first-run seed trigger (story 001 owns the seed write itself);
/// * later frames of the same cycle reuse the temp-slot draft, so unsaved
///   edits survive redraws for the duration of the open;
/// * when the modal closes, [`paint`] expunges the slots and the next open
///   re-binds to the live file bytes — no stale draft resurrection.
///
/// Signals: save → Written (success toast) / Unchanged (info toast) /
/// Err (card keeps the modal open with the red feedback line already set);
/// restore → Ok (success toast) / Err (same keep-open red line).
fn paint_persona_section(ui: &mut egui::Ui, s: &mut dyn Surface) {
    let card_slot = egui::Id::new("koolade_persona_card");
    let live_flag = egui::Id::new("koolade_persona_was_closed");
    // Flag absent or true == "was closed" == pristine: first entry of this
    // open cycle. false == a live draft occupies the slot.
    let live = ui.ctx().data_mut(|d| d.get_temp::<bool>(live_flag)) == Some(false);
    let mut card = if live {
        ui.ctx()
            .data_mut(|d| d.remove_temp::<dialogs::DlgPersona>(card_slot))
            .unwrap_or_else(|| {
                dialogs::DlgPersona::from_load(&crate::persistence::persona::load_persona())
            })
    } else {
        dialogs::DlgPersona::from_load(&crate::persistence::persona::load_persona())
    };
    let (save_pressed, restore_pressed) = dialogs::paint_persona_card(ui, &mut card);
    if save_pressed {
        match card.save() {
            Ok(dialogs::PersonaSaveOutcome::Written) => s
                .toasts()
                .success("Persona saved \u{2014} effective from the next reply."),
            Ok(dialogs::PersonaSaveOutcome::Unchanged) => {
                s.toasts().info("Persona already in sync.")
            }
            // Err: the card already carries the red feedback line; the modal
            // stays open (only its X dismisses).
            Err(_) => {}
        }
    }
    if restore_pressed && card.restore_default().is_ok() {
        s.toasts().success("Persona default restored")
    }
    // Err path: red feedback already set on the card; modal stays open.
    ui.ctx().data_mut(|d| {
        d.insert_temp(live_flag, false);
        d.insert_temp(card_slot, card);
    });
}
