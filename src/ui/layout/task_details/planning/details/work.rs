use super::*;
mod approval;
use approval::paint_approval;
mod git_intents;
mod related_task;
mod task_intent;

pub(super) fn paint(
    ui: &mut egui::Ui,
    surface: &mut dyn Surface,
    work: &crate::core::planning_work::Work,
    planning: &[crate::core::planning_work::Work],
    board: &crate::ui::planning_board::ViewModel,
) {
    use crate::core::planning_work::WorkKind;
    if board
        .cancelled
        .contains(&crate::persistence::cancelled_work::planning_id(&work.uid))
    {
        task_intent::paint(ui, None, board, work);
        ui.heading(&work.title);
        ui.label("Cancelled · This work will not resume automatically.");
        ui.label(&work.detail);
        let mut terminal = work.clone();
        terminal.status = crate::core::planning_work::WorkStatus::Done;
        paint_controls(ui, surface, &terminal, board);
        return;
    }
    // Put a pending approval before the TaskIntent history/map so the action
    // remains visible in the bounded details viewport.
    paint_approval(ui, surface, work);
    task_intent::paint(ui, Some(surface), board, work);
    ui.heading(&work.title);
    ui.label(super::super::super::action::concise_detail(&work.detail));
    ui.add_space(crate::ui::theme::spacing::M);
    ui.label(crate::ui::theme::section_heading("Next step"));
    if work.kind == WorkKind::TaskGeneration {
        let git_intent_approved = git_intents::paint(ui, surface, work);
        let can_generate = work
            .feature_id
            .as_ref()
            .is_some_and(|feature_id| surface.feature_approved(feature_id))
            && matches!(
                work.status,
                crate::core::planning_work::WorkStatus::Todo
                    | crate::core::planning_work::WorkStatus::NeedsAttention
            )
            && git_intent_approved;
        if can_generate {
            ui.label("The independently approved specification is ready for task generation.");
            if ui
                .add_enabled(
                    !surface.conversation_busy(),
                    egui::Button::new("Generate tasks").fill(crate::ui::theme::ACTION),
                )
                .clicked()
                && let Some(feature_id) = work.feature_id.as_ref()
            {
                surface.dispatch(crate::ui::ApplicationCommand::GenerateTasksForFeature {
                    work_key: work.key.clone(),
                    feature_id: feature_id.clone(),
                });
            }
        } else {
            if work.git_intents.is_empty() {
                ui.label("Approve repository Git intent before generating tasks.");
            } else {
                ui.label(&work.detail);
            }
        }
    } else if work.status == crate::core::planning_work::WorkStatus::InProgress {
        ui.label("Kool.ad/e is working on this task.");
    } else if matches!(
        work.status,
        crate::core::planning_work::WorkStatus::NeedsAttention
            | crate::core::planning_work::WorkStatus::InReview
    ) {
        if work.operational_recovery.is_some() {
            ui.label("AGENT · Managed by Kool.ad/e. Your saved answer is preserved.");
            ui.label(&work.detail);
        } else {
            super::decision::free_text(ui, surface, &work.key);
        }
    }
    if let Some(parent) = crate::ui::layout::planning_parent_label(
        work,
        planning.iter().collect::<Vec<_>>().as_slice(),
    ) {
        ui.label(crate::ui::theme::helper_text(parent));
    }
    ui.collapsing("Why am I seeing this?", |ui| {
        ui.label(crate::ui::theme::metadata_text(format!(
            "{} · {:?}",
            work.kind.label(),
            work.status
        )));
        ui.label(&work.detail);
    });
    if !work.git_intents.is_empty() {
        ui.collapsing("Approved Git intent", |ui| {
            for intent in work.git_intents.values() {
                ui.separator();
                ui.label(format!("Repository: {}", intent.repository_id));
                ui.label(format!("Source branch: {}", intent.source_ref));
                ui.label(format!("Pinned source commit: {}", intent.source_sha));
                ui.label(format!("Destination branch: {}", intent.destination_ref));
                ui.label(format!(
                    "Publication mode: {}",
                    match intent.publication_mode {
                        crate::core::git_intent::PublicationModeV1::HoldForReview =>
                            "Hold for review",
                        crate::core::git_intent::PublicationModeV1::CreatePullRequest =>
                            "Create pull request",
                        crate::core::git_intent::PublicationModeV1::AutoPublish => "Auto publish",
                    }
                ));
                ui.label(format!("Approval revision: {}", intent.approval_revision));
                if let Some(parent_uid) = intent.inherited_from_task_uid.as_deref() {
                    ui.label(format!("Inherited from task: {parent_uid}"));
                }
            }
        });
    }
    if let Some(offer) = &work.follow_up_task {
        related_task::paint(ui, surface, work, offer);
    }
    paint_controls(ui, surface, work, board);
}

fn paint_controls(
    ui: &mut egui::Ui,
    surface: &mut dyn Surface,
    work: &crate::core::planning_work::Work,
    board: &crate::ui::planning_board::ViewModel,
) {
    let is_intent_root = work.parent_uid.is_none()
        && work.kind.starts_task_intent()
        && work.task_intent_root_uid() == Some(work.uid.as_str());
    let archived = work.task_intent_archived_at_ms.is_some() || board.is_archived(&work.key);
    ui.horizontal(|ui| {
        if work.status != crate::core::planning_work::WorkStatus::Done
            && ui.button("Cancel").clicked()
        {
            ui.ctx().data_mut(|data| {
                data.insert_temp(egui::Id::new("koolade_cancel_pending"), work.key.clone())
            });
        }
        if is_intent_root && archived {
            if ui.button("Restore TaskIntent").clicked() {
                surface.dispatch(crate::ui::ApplicationCommand::RestoreTaskIntent {
                    root_uid: work.uid.clone(),
                });
            }
        } else if work.status == crate::core::planning_work::WorkStatus::Done
            && work.task_intent_uid.as_deref().is_none_or(|uid| uid == work.uid)
            && (!is_intent_root || board.can_archive_task_intent(work))
            && ui.button("Archive").clicked()
        {
            surface.dispatch(crate::ui::ApplicationCommand::ArchiveTask {
                ticket: work.key.clone(),
            });
            ui.ctx().data_mut(|data| {
                data.insert_temp(egui::Id::new("koolade_task_details_close"), true);
            });
        } else if is_intent_root
            && work.status == crate::core::planning_work::WorkStatus::Done
            && !board.can_archive_task_intent(work)
        {
            ui.label(crate::ui::theme::helper_text(
                "Archive is available after the current work cycle reaches a verified terminal outcome.",
            ));
        }
    });
    if work.status == crate::core::planning_work::WorkStatus::Done
        && work
            .task_intent_uid
            .as_deref()
            .is_some_and(|uid| uid != work.uid)
    {
        ui.label(crate::ui::theme::helper_text("This card stays with its TaskIntent. Archive the parent request when its work is complete."));
    }
    let cancel_id = egui::Id::new("koolade_cancel_pending");
    let confirming = ui
        .ctx()
        .data_mut(|data| data.get_temp::<String>(cancel_id))
        .as_deref()
        == Some(work.key.as_str());
    if confirming {
        ui.group(|ui| {
            ui.label(crate::ui::theme::section_heading("Cancel this task?"));
            ui.label(crate::ui::theme::helper_text(
                "The task will leave the queue. Its files, conversation, and planning history will be kept.",
            ));
            ui.horizontal(|ui| {
                if ui.button("Keep working").clicked() {
                    ui.ctx().data_mut(|data| data.remove::<String>(cancel_id));
                }
                if ui
                    .button(egui::RichText::new("Confirm cancel").color(crate::ui::theme::DANGER))
                    .clicked()
                {
                    surface.dispatch(crate::ui::ApplicationCommand::CancelWork {
                        key: work.key.clone(),
                    });
                    ui.ctx().data_mut(|data| data.remove::<String>(cancel_id));
                }
            });
        });
    }
}
