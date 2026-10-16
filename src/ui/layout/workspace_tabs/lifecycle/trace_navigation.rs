use crate::{
    core::lifecycle::{SpecTraceNavigationV1, SpecTraceReferenceKindV1 as RefKind},
    ui::{ApplicationCommand, Surface},
};

#[cfg(test)]
#[path = "trace_navigation/tests.rs"]
mod tests;

pub(super) fn paint(
    ui: &mut egui::Ui,
    surface: &mut dyn Surface,
    navigation: SpecTraceNavigationV1,
) {
    ui.group(|ui| {
        ui.horizontal(|ui| {
            ui.heading(navigation.title);
            if ui.button("Close trace").clicked() {
                surface.dispatch(ApplicationCommand::CloseSpecificationTrace);
            }
        });
        ui.label(format!(
            "{} · {}",
            target_label(navigation.target_kind),
            navigation.detail
        ));
        ui.label("Persisted requirement links");
        for trace in navigation.requirement_traces {
            egui::CollapsingHeader::new(format!(
                "{} · revision {}",
                trace.requirement_id, trace.revision
            ))
            .default_open(true)
            .show(ui, |ui| {
                reference_link(
                    ui,
                    surface,
                    &navigation.feature_uid,
                    RefKind::LifecycleEvent,
                    "Event",
                    &trace.event_id,
                );
                for reference in trace.decision_refs {
                    reference_link(
                        ui,
                        surface,
                        &navigation.feature_uid,
                        RefKind::DecisionOrReceipt,
                        "Decision or receipt",
                        &reference,
                    );
                }
                for reference in trace.task_refs {
                    reference_link(
                        ui,
                        surface,
                        &navigation.feature_uid,
                        RefKind::Task,
                        "Task",
                        &reference,
                    );
                }
                for reference in trace.test_refs {
                    reference_link(
                        ui,
                        surface,
                        &navigation.feature_uid,
                        RefKind::Test,
                        "Test",
                        &reference,
                    );
                }
                for reference in trace.pull_request_refs {
                    reference_link(
                        ui,
                        surface,
                        &navigation.feature_uid,
                        RefKind::PullRequestState,
                        "PR state",
                        &reference,
                    );
                }
                for reference in trace.evidence_refs {
                    let kind = if reference.starts_with("provider-pr-state:") {
                        RefKind::PullRequestState
                    } else {
                        RefKind::Evidence
                    };
                    reference_link(
                        ui,
                        surface,
                        &navigation.feature_uid,
                        kind,
                        "Evidence",
                        &reference,
                    );
                }
            });
        }
    });
}

fn reference_link(
    ui: &mut egui::Ui,
    surface: &mut dyn Surface,
    feature_uid: &str,
    kind: RefKind,
    label: &str,
    reference: &str,
) {
    if ui.link(format!("{label}: {reference}")).clicked() {
        surface.dispatch(ApplicationCommand::OpenSpecificationTrace {
            feature_uid: feature_uid.to_owned(),
            kind,
            reference: reference.to_owned(),
        });
    }
}

fn target_label(kind: crate::core::lifecycle::SpecTraceTargetKindV1) -> &'static str {
    use crate::core::lifecycle::SpecTraceTargetKindV1 as Kind;
    match kind {
        Kind::LifecycleEvent => "Lifecycle event",
        Kind::Decision => "Decision",
        Kind::CodeReview => "Code review",
        Kind::RuntimeQa => "Runtime QA",
        Kind::Task => "Task",
        Kind::TestReference => "Acceptance test",
        Kind::PullRequestState => "Pull request state",
        Kind::OperatorDecision => "Operator decision",
        Kind::EvidenceReference => "Evidence",
    }
}
