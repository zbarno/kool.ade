use crate::ui::{Surface, task_detail::LifecycleView};

pub(super) fn relationships(
    ui: &mut egui::Ui,
    surface: &mut Option<&mut dyn Surface>,
    lifecycle: &LifecycleView,
) {
    if lifecycle.parent_task.is_none()
        && lifecycle.dependencies.is_empty()
        && lifecycle.subtasks.is_empty()
    {
        return;
    }
    ui.label(crate::ui::theme::section_heading("Task relationships"));
    if let Some(parent) = &lifecycle.parent_task {
        relation_link(ui, surface, lifecycle, "Subtask of", parent);
        ui.label(crate::ui::theme::helper_text(format!(
            "{} · changes are included in the parent's single pull request.",
            parent.status
        )));
    }
    if !lifecycle.dependencies.is_empty() {
        ui.label("Dependencies");
        for dependency in &lifecycle.dependencies {
            relation_link(ui, surface, lifecycle, "Dependency", dependency);
            ui.label(crate::ui::theme::helper_text(&dependency.status));
        }
    }
    if !lifecycle.subtasks.is_empty() {
        ui.label("Owned subtasks · all changes are included in this parent's single pull request");
        for subtask in &lifecycle.subtasks {
            relation_link(ui, surface, lifecycle, "Subtask", subtask);
            ui.label(crate::ui::theme::helper_text(&subtask.status));
            for criterion in &subtask.acceptance_criteria {
                ui.label(format!("  • {criterion}"));
            }
        }
    }
}

pub(super) fn deltas(
    ui: &mut egui::Ui,
    surface: &mut Option<&mut dyn Surface>,
    lifecycle: &LifecycleView,
) {
    if lifecycle.specification_deltas.is_empty() {
        return;
    }
    ui.label("Specification findings linked to this task");
    for (revision, delta) in &lifecycle.specification_deltas {
        ui.label(format!(
            "Revision {revision} · {} · {:?}",
            delta.requirement_id, delta.classification
        ));
        if let Some(proposed) = delta.proposed_target.as_deref() {
            ui.small(format!("Proposed target: {proposed}"));
        }
        if let Some(observed) = delta.observed_current.as_deref() {
            ui.small(format!("Observed current state: {observed}"));
        }
        for evidence in &delta.evidence_refs {
            let kind = if evidence.starts_with("provider-pr-state:") {
                crate::core::lifecycle::SpecTraceReferenceKindV1::PullRequestState
            } else {
                crate::core::lifecycle::SpecTraceReferenceKindV1::Evidence
            };
            trace_link(ui, surface, lifecycle, kind, "Evidence", evidence);
        }
        trace_link(
            ui,
            surface,
            lifecycle,
            crate::core::lifecycle::SpecTraceReferenceKindV1::LifecycleEvent,
            "Event",
            &delta.event_id,
        );
        for reference in &delta.decision_refs {
            trace_link(
                ui,
                surface,
                lifecycle,
                crate::core::lifecycle::SpecTraceReferenceKindV1::DecisionOrReceipt,
                "Decision or receipt",
                reference,
            );
        }
        for reference in &delta.test_refs {
            trace_link(
                ui,
                surface,
                lifecycle,
                crate::core::lifecycle::SpecTraceReferenceKindV1::Test,
                "Test",
                reference,
            );
        }
        for reference in &delta.affected_task_ids {
            trace_link(
                ui,
                surface,
                lifecycle,
                crate::core::lifecycle::SpecTraceReferenceKindV1::Task,
                "Affected task",
                reference,
            );
        }
        for reference in &delta.followup_ids {
            trace_link(
                ui,
                surface,
                lifecycle,
                crate::core::lifecycle::SpecTraceReferenceKindV1::FollowupTask,
                "Follow-up task",
                reference,
            );
        }
    }
}

pub(super) fn receipts(
    ui: &mut egui::Ui,
    surface: &mut Option<&mut dyn Surface>,
    lifecycle: &LifecycleView,
) {
    receipt_row(
        ui,
        surface,
        lifecycle,
        "Code review",
        lifecycle.code_review.as_ref(),
    );
    receipt_row(
        ui,
        surface,
        lifecycle,
        "Runtime QA",
        lifecycle.runtime_qa.as_ref(),
    );
    receipt_row(
        ui,
        surface,
        lifecycle,
        "Specification reconciliation",
        lifecycle.spec_reconciliation.as_ref(),
    );
}

fn relation_link(
    ui: &mut egui::Ui,
    surface: &mut Option<&mut dyn Surface>,
    lifecycle: &LifecycleView,
    label: &str,
    relation: &crate::ui::task_detail::TaskRelationView,
) {
    if let Some(task_uid) = relation.lifecycle_task_uid.as_deref() {
        trace_link(
            ui,
            surface,
            lifecycle,
            crate::core::lifecycle::SpecTraceReferenceKindV1::Task,
            label,
            task_uid,
        );
    } else {
        ui.label(label);
    }
    ui.small(&relation.title);
}

fn receipt_row(
    ui: &mut egui::Ui,
    surface: &mut Option<&mut dyn Surface>,
    lifecycle: &LifecycleView,
    label: &str,
    receipt: Option<&crate::core::lifecycle::GateReceipt>,
) {
    ui.horizontal_wrapped(|ui| {
        ui.label(egui::RichText::new(label).strong());
        ui.label(receipt_summary(receipt));
    });
    if let Some(receipt) = receipt {
        for evidence in &receipt.evidence_refs {
            trace_link(
                ui,
                surface,
                lifecycle,
                crate::core::lifecycle::SpecTraceReferenceKindV1::Evidence,
                "Receipt evidence",
                evidence,
            );
        }
    }
}

fn trace_link(
    ui: &mut egui::Ui,
    surface: &mut Option<&mut dyn Surface>,
    lifecycle: &LifecycleView,
    kind: crate::core::lifecycle::SpecTraceReferenceKindV1,
    label: &str,
    reference: &str,
) {
    if ui.link(format!("{label}: {reference}")).clicked()
        && let Some(surface) = surface.as_deref_mut()
    {
        surface.dispatch(crate::ui::ApplicationCommand::OpenSpecificationTrace {
            feature_uid: lifecycle.feature_uid.clone(),
            kind,
            reference: reference.to_owned(),
        });
    }
}

pub(super) fn receipt_summary(receipt: Option<&crate::core::lifecycle::GateReceipt>) -> String {
    receipt.map_or_else(
        || "Not recorded".to_owned(),
        |receipt| {
            format!(
                "{} · {} evidence item(s)",
                decision_label(receipt.decision),
                receipt.evidence_refs.len()
            )
        },
    )
}

pub(super) fn decision_label(decision: crate::core::lifecycle::GateDecision) -> &'static str {
    use crate::core::lifecycle::GateDecision;
    match decision {
        GateDecision::Approved => "Approved",
        GateDecision::Rejected => "Rejected",
        GateDecision::ChangesRequested => "Changes requested",
        GateDecision::NotApplicable => "Not applicable",
    }
}
