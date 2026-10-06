use super::*;
use crate::app::dialogs;

pub(super) fn paint(
    ui: &mut egui::Ui,
    s: &mut dyn Surface,
    board: &crate::ui::planning_board::ViewModel,
    mut settings_open: bool,
    settings_was_open: bool,
) -> bool {
    let settings_id = egui::Id::new("koolade_workspace_settings_open");
    settings_open |= ui
        .ctx()
        .data_mut(|data| data.get_temp::<bool>(settings_id).unwrap_or(false));
    if settings_open {
        let mut open_batch = None;
        let closed = crate::ui::overlays::show_modal(ui, true, "Workspace settings", 640.0, |ui| {
            ui.heading("Appearance");
            let reduced_id = egui::Id::new(theme::REDUCE_MOTION_ID);
            let mut reduced_motion = ui
                .ctx()
                .data_mut(|data| data.get_temp::<bool>(reduced_id).unwrap_or(false));
            if ui.checkbox(&mut reduced_motion, "Reduce motion").changed() {
                ui.ctx()
                    .data_mut(|data| data.insert_temp(reduced_id, reduced_motion));
            }
            ui.ctx().style_mut_of(egui::Theme::Dark, |style| {
                style.animation_time = if reduced_motion { 0.0 } else { 0.2 };
            });
            ui.label(
                "Stops interface transitions and animated busy indicators. Live activity updates continue.",
            );
            ui.separator();
            ui.heading("Project / Git");
            if ui.button("Repository names…").clicked() {
                ui.ctx().data_mut(|data| {
                    data.insert_temp(egui::Id::new("koolade_open_project_settings"), true);
                });
            }
            ui.separator();
            ui.heading("Automation policy");
            if ui.button("Coding tools and models…").clicked() {
                ui.ctx().data_mut(|data| {
                    data.insert_temp(egui::Id::new("koolade_open_coding_settings"), true);
                });
            }
            ui.label("Configure installed coding tools separately from the models and work types that use them.");
            ui.label("Saved for this project on this device, across Kool.ad/e windows.");
            let mut parallel = s.max_parallel_tasks();
            if ui
                .add(egui::Slider::new(&mut parallel, 1..=8).text("Concurrent tasks"))
                .changed()
            {
                s.dispatch(ApplicationCommand::SetMaxParallelTasks { count: parallel });
            }
            ui.label(format!("{} workers active. Dependencies must merge before dependent tasks start. Merges are serialized and reverified.", s.active_task_count()));
            ui.label("Lowering the limit affects new starts; running tasks keep their work.");
            let mut auto_plan = s.auto_plan();
            if ui.checkbox(&mut auto_plan, "Plan automatically").changed() {
                s.dispatch(ApplicationCommand::SetAutoPlan { enabled: auto_plan });
            }
            ui.label("Kool.ad/e can investigate Agent-owned planning questions, record evidence-backed findings, and suggest next steps in the background. It does not create implementation tasks or start builds. Turning this off stops new investigations; a change already being saved may finish.");
            let mut auto_build = s.auto_build();
            if ui
                .checkbox(&mut auto_build, "Build approved changes automatically")
                .changed()
            {
                s.dispatch(ApplicationCommand::SetAutoBuild {
                    enabled: auto_build,
                });
            }
            ui.label("Auto-Implement keeps the TODO queue ready and starts eligible tasks as they become available. Feature tasks still need explicit approval. Kool.ad/e manages dependencies and verification; verified work stays local unless Auto Publish is on.");
            let mut auto_publish = s.auto_publish();
            if ui
                .checkbox(&mut auto_publish, "Publish verified changes automatically")
                .changed()
            {
                s.dispatch(ApplicationCommand::SetAutoPublish {
                    enabled: auto_publish,
                });
            }
            ui.label("Kool.ad/e checks its work locally first. When Auto Publish is on, it also waits for the project's separate checks before sharing. If those checks fail or are unavailable, verified work stays on this device. Enabling Auto Publish turns on this check.");
            let mut require_checks = s.require_independent_checks();
            let check = ui.add_enabled(
                !auto_publish,
                egui::Checkbox::new(
                    &mut require_checks,
                    "Wait for project checks before publishing",
                ),
            );
            if check.changed() {
                s.dispatch(ApplicationCommand::SetRequireIndependentChecks {
                    enabled: require_checks,
                });
            }
            ui.label("On supported GitHub projects, Kool.ad/e waits for the project's checks after its own verification. If checks fail or are unavailable, the verified work stays unshared.");
            ui.label("Create a Feature task from the board to start planning. You can also approve a feature in Specifications, then use a task’s Implement action.");
            ui.separator();
            paint_persona_section(ui, s);
            if !s.queue_status().is_empty() {
                ui.separator();
                ui.label(s.queue_status());
            }
            if let Some((ticket, claim)) = s.stale_task_claim() {
                ui.separator();
                ui.label(format!(
                    "{} is held by {} (session {}) since {} on base {}.",
                    ticket,
                    claim.owner,
                    claim.session_id,
                    chrono::DateTime::from_timestamp(claim.claimed_at, 0)
                        .map(|time| time.to_rfc3339())
                        .unwrap_or_else(|| claim.claimed_at.to_string()),
                    claim.base_commit
                ));
                if ui
                    .button("I confirmed the old worker stopped — take over stale claim")
                    .clicked()
                {
                    s.dispatch(ApplicationCommand::TakeOverStaleTaskClaim { ticket });
                }
            }
            for doc in board
                .task_documents
                .iter()
                .filter(|d| d.path.ends_with("/README.md"))
            {
                ui.separator();
                ui.label(&doc.title);
                if ui.button("Batch overview").clicked() {
                    open_batch = Some(doc.path.clone());
                }
            }
        });
        if closed {
            settings_open = false;
        }
        if let Some(path) = open_batch {
            settings_open = false;
            ui.ctx().data_mut(|d| {
                d.insert_temp(egui::Id::new("koolade_document_tab"), true);
                d.insert_temp(egui::Id::new("koolade_selected_task"), path);
            });
        }
    }
    // Close edge: the modal went open → closed this frame. Drop the VOLATILE
    // persona draft (its two session-lived temp slots) so the NEXT open
    // re-binds to the live persona.md bytes — bind-on-open doctrine, no ghost
    // draft (unsaved edits and diagnostics die with the modal on purpose).
    //
    // Deliberately SURGICAL removals, not an egui `IdTypeMap::clear()`:
    // on egui 0.36 a clear() would wipe EVERY temporary and persisted value
    // (the operator's open chat tabs, document views, widget state) — far
    // beyond this ticket's purely-additive bounds. Only the two persona
    // slots owe expiry here.
    if settings_was_open && !settings_open {
        ui.ctx().data_mut(|d| {
            d.remove_temp::<bool>(egui::Id::new("koolade_persona_was_closed"));
            d.remove_temp::<dialogs::DlgPersona>(egui::Id::new("koolade_persona_card"));
        });
    }
    ui.ctx()
        .data_mut(|d| d.insert_temp(settings_id, settings_open));
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
