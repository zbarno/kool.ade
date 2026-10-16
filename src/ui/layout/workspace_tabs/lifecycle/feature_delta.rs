use crate::{
    core::lifecycle::SpecDeltaRecordV1,
    ui::{ApplicationCommand, theme},
};

pub(super) fn paint(
    ui: &mut egui::Ui,
    _surface: &dyn crate::ui::Surface,
    feature_uid: Option<&str>,
    revision: Option<u64>,
    delta: SpecDeltaRecordV1,
) -> Option<ApplicationCommand> {
    ui.separator();
    ui.label(format!(
        "{} · {:?}",
        delta.requirement_id, delta.classification
    ));
    if let Some(decision) = &delta.operator_decision {
        ui.label(match decision.decision {
            crate::core::lifecycle::SpecDeltaOperatorChoiceV1::Adopted => {
                "Explicitly adopted by the user"
            }
            crate::core::lifecycle::SpecDeltaOperatorChoiceV1::Declined => "Declined by the user",
        });
    } else if delta.needs_user_reapproval {
        ui.label(egui::RichText::new("Adoption requires user approval").color(theme::WARNING));
    } else {
        ui.label("Nonblocking proposal; requires explicit adoption to change the target");
    }
    if let Some(target) = delta.proposed_target.as_deref() {
        ui.label(format!("Proposed target: {target}"));
    }
    if let Some(old) = delta.old_target.as_deref() {
        ui.small(format!("Current approved wording: {old}"));
    }
    if let Some(observed) = delta.observed_current.as_deref() {
        ui.small(format!("Observed current state: {observed}"));
    }
    use crate::core::lifecycle::SpecTraceReferenceKindV1 as RefKind;
    let mut trace_references = vec![("Event", RefKind::LifecycleEvent, delta.event_id.as_str())];
    trace_references.extend(delta.evidence_refs.iter().map(|reference| {
        let kind = if reference.starts_with("provider-pr-state:") {
            RefKind::PullRequestState
        } else {
            RefKind::Evidence
        };
        ("Evidence or PR state", kind, reference.as_str())
    }));
    trace_references.extend(
        delta
            .test_refs
            .iter()
            .map(|reference| ("Test", RefKind::Test, reference.as_str())),
    );
    trace_references.extend(delta.decision_refs.iter().map(|reference| {
        (
            "Decision or review/QA receipt",
            RefKind::DecisionOrReceipt,
            reference.as_str(),
        )
    }));
    trace_references.extend(
        delta
            .affected_task_ids
            .iter()
            .map(|reference| ("Task", RefKind::Task, reference.as_str())),
    );
    trace_references.extend(
        delta
            .followup_ids
            .iter()
            .map(|reference| ("Follow-up task", RefKind::FollowupTask, reference.as_str())),
    );
    if let Some(decision) = &delta.operator_decision {
        trace_references.push((
            "Operator decision",
            RefKind::OperatorDecision,
            decision.receipt_uid.as_str(),
        ));
    }
    let mut navigation = None;
    for (label, kind, reference) in trace_references {
        if ui.link(format!("{label} trace: {reference}")).clicked()
            && let Some(feature_uid) = feature_uid
        {
            navigation = Some(ApplicationCommand::OpenSpecificationTrace {
                feature_uid: feature_uid.to_owned(),
                kind,
                reference: reference.to_owned(),
            });
        }
    }
    if navigation.is_some() {
        return navigation;
    }
    if !delta.affected_task_ids.is_empty() || delta.operator_decision.is_some() {
        return None;
    }
    let mut decision = None;
    ui.horizontal(|ui| {
        if ui.button("Adopt target").clicked() {
            decision = Some(true);
        }
        if ui.button("Decline").clicked() {
            decision = Some(false);
        }
    });
    decision.map(|adopt| ApplicationCommand::DecideFeatureSpecDelta {
        feature_uid: feature_uid.unwrap_or_default().to_owned(),
        requirement_id: delta.requirement_id,
        expected_sequence: revision.unwrap_or_default(),
        adopt,
    })
}

#[cfg(test)]
#[path = "feature_delta/tests.rs"]
mod tests;
