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
    } else if let Some(action) = selection
        .as_deref()
        .and_then(|key| key.strip_prefix("feature-approval:"))
        .and_then(|id| {
            surface
                .feature_actions(None)
                .into_iter()
                .find(|action| action.id == id)
        })
    {
        let id = action.id.clone();
        let closed = crate::ui::overlays::show_task_modal(ui, &id, bounds, |ui| {
            super::super::task_details::paint_feature_approval(ui, surface, &action);
        });
        clear_if_closed(selection, closed);
    } else if let Some(issue) = board
        .setup_attention
        .as_ref()
        .filter(|issue| selection.as_deref() == Some(issue.id))
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
