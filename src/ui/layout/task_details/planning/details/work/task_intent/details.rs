pub(super) fn paint_task_map(
    ui: &mut egui::Ui,
    intent_uid: &str,
    conversation_key: &str,
    planning_children: &[&crate::core::planning_work::Work],
    task_documents: &[&crate::artifacts::task_docs::TaskDocument],
    decision_items: &[&crate::domain::item::OpenItem],
    board: &crate::ui::planning_board::ViewModel,
) {
    let child_count = planning_children.len() + task_documents.len() + decision_items.len();
    ui.add_space(crate::ui::theme::spacing::S);
    egui::CollapsingHeader::new(format!(
        "Task Map · {child_count} child {}",
        if child_count == 1 { "item" } else { "items" }
    ))
    .id_salt(("task_intent_map", intent_uid))
    .default_open(child_count <= 4)
    .show(ui, |ui| {
        if child_count == 0 {
            ui.label(crate::ui::theme::helper_text(
                "This request is ready for planning. Its root stays visible while work is created.",
            ));
        }
        egui::ScrollArea::vertical()
            .id_salt(("task_intent_children", intent_uid))
            .max_height(220.0)
            .show(ui, |ui| {
                for child in planning_children {
                    if ui
                        .button(format!("{} · {}", child.title, work_status(child.status)))
                        .clicked()
                    {
                        navigate_to_planning_child(ui, conversation_key, &child.key);
                    }
                }
                for document in task_documents {
                    if ui
                        .button(format!(
                            "{} · {}",
                            document.title,
                            document_status(board, document)
                        ))
                        .clicked()
                    {
                        navigate_to_task_document(ui, conversation_key, &document.path);
                    }
                }
                for item in decision_items {
                    if ui
                        .button(format!("{} · {}", item.question, item_status(item.status)))
                        .clicked()
                    {
                        navigate_to_planning_child(ui, conversation_key, &item.id);
                    }
                }
            });
    });
}

pub(super) fn paint_history(
    ui: &mut egui::Ui,
    intent_uid: &str,
    history: &[crate::core::planning_work::TaskIntentEventV1],
) {
    if history.is_empty() {
        return;
    }
    ui.add_space(crate::ui::theme::spacing::S);
    egui::CollapsingHeader::new(format!("History · {} events", history.len()))
        .id_salt(("task_intent_history", intent_uid))
        .show(ui, |ui| {
            egui::ScrollArea::vertical()
                .id_salt(("task_intent_history_scroll", intent_uid))
                .max_height(180.0)
                .show(ui, |ui| {
                    for event in history.iter().rev() {
                        ui.strong(event_label(&event.kind));
                        ui.label(event.summary.as_str());
                        if event.related_uid.is_some() {
                            ui.label(crate::ui::theme::helper_text(
                                "Related work is available in the Task Map.",
                            ));
                        }
                        ui.add_space(crate::ui::theme::spacing::XS);
                    }
                });
        });
}

fn event_label(kind: &crate::core::planning_work::TaskIntentEventKindV1) -> &'static str {
    use crate::core::planning_work::TaskIntentEventKindV1 as Kind;
    match kind {
        Kind::Created => "Request created",
        Kind::GoalRevised => "Goal revised",
        Kind::StatusChanged => "Planning status changed",
        Kind::WorkLinked => "Planning work linked",
        Kind::ExecutableTaskLinked => "Executable task linked",
        Kind::FollowUpCycleStarted => "Follow-up cycle started",
        Kind::DecisionRecorded => "Decision recorded",
        Kind::ApprovalRecorded => "Plan approved",
        Kind::PlanDecisionRecorded => "Plan decision recorded",
        Kind::ReviewRecorded => "Independent review recorded",
        Kind::QualityAssuranceRecorded => "Runtime QA recorded",
        Kind::SpecificationReconciliationRecorded => "Specification reconciliation recorded",
        Kind::DeliveryOutcomeRecorded => "Delivery outcome recorded",
        Kind::Archived => "TaskIntent archived",
        Kind::Restored => "TaskIntent restored",
    }
}

fn work_status(status: crate::core::planning_work::WorkStatus) -> &'static str {
    match status {
        crate::core::planning_work::WorkStatus::Todo => "Ready",
        crate::core::planning_work::WorkStatus::InProgress => "In progress",
        crate::core::planning_work::WorkStatus::InReview => "In review",
        crate::core::planning_work::WorkStatus::NeedsAttention => "Needs attention",
        crate::core::planning_work::WorkStatus::Done => "Completed",
    }
}

fn document_status(
    board: &crate::ui::planning_board::ViewModel,
    document: &crate::artifacts::task_docs::TaskDocument,
) -> &'static str {
    board
        .lifecycle_tasks
        .get(&document.path)
        .map(|phase| match phase {
            crate::core::lifecycle::TaskPhase::Ready => "Ready",
            crate::core::lifecycle::TaskPhase::Blocked => "Blocked",
            crate::core::lifecycle::TaskPhase::Running => "Running",
            crate::core::lifecycle::TaskPhase::AwaitingReview => "Awaiting review",
            crate::core::lifecycle::TaskPhase::Repairing => "Repairing",
            crate::core::lifecycle::TaskPhase::ReReview => "In review",
            crate::core::lifecycle::TaskPhase::AwaitingRuntimeQa => "Runtime QA",
            crate::core::lifecycle::TaskPhase::AwaitingSpecReconciliation => "Spec review",
            crate::core::lifecycle::TaskPhase::Done => "Verified",
            crate::core::lifecycle::TaskPhase::PrPending => "Preparing pull request",
            crate::core::lifecycle::TaskPhase::PrOpened => "Pull request open",
            crate::core::lifecycle::TaskPhase::PrMerged => "Merged",
            crate::core::lifecycle::TaskPhase::Failed => "Failed",
            crate::core::lifecycle::TaskPhase::Cancelled => "Cancelled",
        })
        .unwrap_or("Status unavailable")
}

fn item_status(status: crate::domain::ItemStatus) -> &'static str {
    match status {
        crate::domain::ItemStatus::Open => "Open",
        crate::domain::ItemStatus::Resolved => "Resolved",
    }
}

pub(super) fn navigate_to_planning_child(ui: &mut egui::Ui, return_key: &str, child_key: &str) {
    ui.ctx().data_mut(|data| {
        data.insert_temp(
            egui::Id::new("koolade_task_relationship_intent_return"),
            return_key.to_owned(),
        );
        data.insert_temp(
            egui::Id::new("koolade_task_relationship_planning_navigation"),
            child_key.to_owned(),
        );
    });
}

fn navigate_to_task_document(ui: &mut egui::Ui, return_key: &str, path: &str) {
    ui.ctx().data_mut(|data| {
        data.insert_temp(
            egui::Id::new("koolade_task_relationship_intent_return"),
            return_key.to_owned(),
        );
        data.insert_temp(
            egui::Id::new("koolade_task_relationship_task_navigation"),
            path.to_owned(),
        );
    });
}
