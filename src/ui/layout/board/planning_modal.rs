use crate::ui::{Surface, planning_board::ViewModel};

pub(super) fn paint(
    ui: &mut egui::Ui,
    surface: &mut dyn Surface,
    board: &ViewModel,
    selection: &mut Option<String>,
    bounds: egui::Rect,
) {
    if let Some(item) = selection
        .as_ref()
        .and_then(|id| board.planning_items.iter().find(|item| &item.id == id))
    {
        let closed =
            crate::ui::overlays::show_task_modal(ui, item.conversation_key(), bounds, |ui| {
                super::super::task_details::paint_planning_item(
                    ui,
                    surface,
                    board,
                    item,
                    (bounds.height() - 160.0).max(160.0),
                );
            });
        clear_if_closed(selection, closed);
        if closed {
            ui.ctx().data_mut(|data| {
                data.remove::<String>(egui::Id::new("koolade_task_relationship_work_return"));
            });
        }
    } else if let Some(work) = selection
        .as_ref()
        .and_then(|key| board.planning_work.iter().find(|work| &work.key == key))
    {
        let closed = crate::ui::overlays::show_task_modal(ui, &work.key, bounds, |ui| {
            super::super::task_details::paint_planning_work(
                ui,
                surface,
                board,
                work,
                (bounds.height() - 160.0).max(160.0),
            );
        });
        clear_if_closed(selection, closed);
        if closed {
            ui.ctx().data_mut(|data| {
                data.remove::<String>(egui::Id::new("koolade_task_relationship_work_return"));
            });
        }
    } else if let Some(question) = selection
        .as_deref()
        .and_then(|key| key.strip_prefix("clarification:"))
        .and_then(|uid| {
            board
                .clarification_questions
                .iter()
                .find(|question| question.uid == uid)
        })
    {
        let key = clarification_key(&question.uid);
        let closed = crate::ui::overlays::show_task_modal(ui, &key, bounds, |ui| {
            super::super::task_details::paint_clarification(
                ui,
                surface,
                question,
                (bounds.height() - 160.0).max(160.0),
            );
        });
        clear_if_closed(selection, closed);
    } else if let Some((proposal, revision)) = selection.as_deref().and_then(|key| {
        board.opportunities.iter().find(|(proposal, _)| {
            crate::core::task_conversation::opportunity_conversation_key(&proposal.opportunity_uid)
                == key
        })
    }) {
        let key =
            crate::core::task_conversation::opportunity_conversation_key(&proposal.opportunity_uid);
        let closed = crate::ui::overlays::show_task_modal(ui, &key, bounds, |ui| {
            super::super::task_details::paint_opportunity(
                ui,
                surface,
                board,
                proposal,
                *revision,
                (bounds.height() - 160.0).max(160.0),
            );
        });
        clear_if_closed(selection, closed);
        if closed {
            ui.ctx().data_mut(|data| {
                data.remove::<String>(egui::Id::new("koolade_task_relationship_work_return"));
            });
        }
    } else if let Some(feature_uid) = selection
        .as_deref()
        .and_then(|key| key.strip_prefix("feature-plan-review:"))
        .and_then(|uid| {
            board.lifecycle_features.iter().find(|feature| {
                feature.feature_uid == uid
                    && feature.phase == crate::core::lifecycle::FeaturePhase::PlanReviewPending
            })
        })
        .map(|feature| feature.feature_uid.as_str())
    {
        let feature_uid = feature_uid.to_owned();
        let key = format!("feature-plan-review:{feature_uid}");
        let review = surface.lifecycle_plan_review(&feature_uid);
        let closed = crate::ui::overlays::show_task_modal(ui, &key, bounds, |ui| match &review {
            Ok(review) => super::super::task_details::paint_plan_review(ui, review),
            Err(error) => {
                ui.heading("Plan Review");
                ui.label(crate::ui::theme::metadata_text(
                    "Independent reviewer · in progress",
                ));
                ui.colored_label(crate::ui::theme::WARNING, error);
            }
        });
        clear_if_closed(selection, closed);
    } else if let Some(action) = selection.as_deref().and_then(|key| {
        surface
            .feature_actions(None)
            .into_iter()
            .find(|action| action.board_key() == key)
    }) {
        // Only the immutable TaskIntent owner may host this approval.
        // Choosing a random related work item made confirmation controls
        // appear alongside unrelated task details.
        let work = action.plan_snapshot.as_ref().and_then(|snapshot| {
            let mut owners = board.planning_work.iter().filter(|work| {
                crate::ui::feature_approval::belongs_to_root(work, &snapshot.feature_uid)
            });
            let owner = owners.next()?;
            owners.next().is_none().then_some(owner)
        });
        let key = work.map_or_else(|| action.board_key(), |work| work.key.clone());
        let closed = crate::ui::overlays::show_task_modal(ui, &key, bounds, |ui| {
            if let Some(work) = work {
                super::super::task_details::paint_planning_work(
                    ui,
                    surface,
                    board,
                    work,
                    (bounds.height() - 160.0).max(160.0),
                );
            } else {
                super::super::task_details::paint_feature_approval(ui, surface, &action);
            }
        });
        clear_if_closed(selection, closed);
    } else if let Some(task) = board
        .setup_tasks
        .iter()
        .find(|task| selection.as_deref() == Some(task.id.as_str()))
    {
        let closed = crate::ui::overlays::show_task_modal(ui, &task.id, bounds, |ui| {
            super::super::task_details::paint_machine_setup(ui, surface, board, task)
        });
        clear_if_closed(selection, closed);
    } else if let Some(issue) = board
        .setup_attention
        .iter()
        .find(|issue| selection.as_deref() == Some(issue.id))
    {
        let closed = crate::ui::overlays::show_task_modal(ui, issue.id, bounds, |ui| {
            super::super::task_details::paint_setup_issue(ui, surface, board, issue)
        });
        clear_if_closed(selection, closed);
    } else {
        *selection = None;
    }
}

fn clear_if_closed(selection: &mut Option<String>, closed: bool) {
    if closed {
        *selection = None;
    }
}

pub(super) fn clarification_key(uid: &str) -> String {
    format!("clarification:{uid}")
}
