use std::{fs, io::ErrorKind, path::Path};

use crate::{
    core::lifecycle::{LifecycleStore, TaskRecord},
    runtime_qa::{
        QaFindingDispositionKind, QaFindingDispositionStatus, QaFindingDispositionV1, QaOutcome,
        QaRunRequestV1, RuntimeQaStore, validate_current_report,
    },
};

mod clarification;

pub(super) fn record_persisted_report(
    planning_store: &crate::artifacts::planning_store::PlanningStore,
    lifecycle: &LifecycleStore<'_>,
    qa_store: &RuntimeQaStore,
    task: &TaskRecord,
) -> Result<Option<String>, String> {
    let Some(request) = task
        .qa_execution
        .as_ref()
        .and_then(|state| state.request.as_ref())
    else {
        return Ok(None);
    };
    let Ok((persisted_request, scenarios, evidence, result)) =
        qa_store.load_run(&request.qa_run_id)
    else {
        return Ok(None);
    };
    validate_request_identity(request, &persisted_request)?;
    validate_current_report(&persisted_request, &scenarios, &evidence, &result)?;
    if matches!(result.outcome, QaOutcome::Pass | QaOutcome::Fail) {
        // Validate the reviewer attestation before creating durable Manager
        // decisions. LifecycleStore repeats this check when it records the
        // event, but that happens after decision persistence below.
        qa_store.load_gate_receipt(&request.qa_run_id)?;
    }
    // Create clarification cards and persist Manager-owned dispositions before
    // advancing lifecycle state. A replay recovers the same decision IDs.
    let mut dispositions = result
        .findings
        .iter()
        .map(|finding| disposition_for_finding(task, request, finding))
        .collect::<Vec<_>>();
    let decision_ids = clarification::persist_user_decisions(
        planning_store,
        task,
        request,
        &result,
        &dispositions,
    )?;
    for disposition in &mut dispositions {
        disposition.decision_id = decision_ids.get(&disposition.finding_id).cloned();
        qa_store.save_disposition(disposition)?;
    }
    let namespace = uuid::Uuid::parse_str(&task.task_uid)
        .map_err(|_| "runtime QA lifecycle task identity is invalid")?;
    let event_uid = uuid::Uuid::new_v5(
        &namespace,
        format!("runtime-qa-report-v1:{}", request.qa_run_id).as_bytes(),
    )
    .hyphenated()
    .to_string();
    let updated = lifecycle
        .record_runtime_qa_report(&task.task_uid, task.revision, &event_uid, qa_store)
        .map_err(|error| error.to_string())?;
    let label = task
        .planning_item_uid
        .as_deref()
        .unwrap_or(task.task_uid.as_str());
    let summary = match result.outcome {
        QaOutcome::Pass => format!(
            "Runtime QA passed for task {label}. Manager may proceed with exact specification reconciliation; this report does not itself reconcile the specification."
        ),
        QaOutcome::Fail => format!(
            "Runtime QA found blocking acceptance mismatches for task {label}. Manager should route the observed findings into implementation repair and require fresh independent review and QA for the repaired revision."
        ),
        QaOutcome::Incomplete | QaOutcome::InfrastructureBlocked => format!(
            "Runtime QA for task {label} did not complete. The lifecycle remains blocked; resolve the recorded runtime prerequisite before a fresh request."
        ),
    };
    let findings = result
        .findings
        .iter()
        .zip(&dispositions)
        .map(|(finding, disposition)| {
            format!(
                "Finding {} ({:?}): Manager disposition {:?}/{:?}; owner {}; expected {:?}; observed {:?}; evidence screenshots [{}]; steps: {}",
                finding.finding_uid,
                finding.severity,
                disposition.kind,
                disposition.status,
                disposition.owner_item_uid.as_deref().unwrap_or("user decision"),
                finding.expected,
                finding.actual,
                finding.screenshot_ids.join(", "),
                finding.reproducible_steps.join(" -> ")
            )
        })
        .collect::<Vec<_>>();
    let phase = format!("lifecycle phase is {:?}", updated.phase);
    let mut message = format!("{summary} The {phase}.");
    if !findings.is_empty() {
        message.push('\n');
        message.push_str(&findings.join("\n"));
    }
    Ok(Some(message))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PersistedReportState {
    Missing,
    Recordable,
    Invalid,
}

pub(super) fn persisted_report_state(
    qa_store: &RuntimeQaStore,
    expected: &QaRunRequestV1,
    running_restart: bool,
) -> PersistedReportState {
    let Ok((persisted, scenarios, evidence, result)) = qa_store.load_run(&expected.qa_run_id)
    else {
        let request_present = path_exists_or_unreadable(
            &qa_store
                .root()
                .join(&expected.qa_run_id)
                .join("request.json"),
        );
        // Applicability records may legitimately store the request before a
        // pending QA dispatch. Without that validated record, request.json is
        // ambiguous on upgraded Pending tasks and Running restarts.
        let valid_pending_applicability =
            !running_restart && qa_store.load_applicability_for_request(expected).is_ok();
        let legacy_request = request_present && !valid_pending_applicability;
        return if running_restart
            || legacy_request
            || partial_report_artifacts_present(qa_store, &expected.qa_run_id)
        {
            PersistedReportState::Invalid
        } else {
            PersistedReportState::Missing
        };
    };
    if &persisted != expected {
        return PersistedReportState::Invalid;
    }
    if validate_current_report(&persisted, &scenarios, &evidence, &result).is_err() {
        return PersistedReportState::Invalid;
    }
    match result.outcome {
        QaOutcome::Pass | QaOutcome::Fail
            if qa_store.load_gate_receipt(&expected.qa_run_id).is_err() =>
        {
            PersistedReportState::Invalid
        }
        QaOutcome::Pass
        | QaOutcome::Fail
        | QaOutcome::Incomplete
        | QaOutcome::InfrastructureBlocked => PersistedReportState::Recordable,
    }
}

fn partial_report_artifacts_present(qa_store: &RuntimeQaStore, qa_run_id: &str) -> bool {
    if uuid::Uuid::parse_str(qa_run_id).is_err() {
        return true;
    }

    let run_dir = qa_store.root().join(qa_run_id);
    for artifact in [
        "report-write.json",
        "scenarios.json",
        "evidence.json",
        "result.json",
        "gate-receipt.json",
        "dispositions",
    ] {
        if path_exists_or_unreadable(&run_dir.join(artifact)) {
            return true;
        }
    }

    let evidence_dir = qa_store.root().join("qa-evidence").join(qa_run_id);
    path_exists_or_unreadable(&evidence_dir)
}

fn path_exists_or_unreadable(path: &Path) -> bool {
    match fs::symlink_metadata(path) {
        Ok(_) => true,
        Err(error) => error.kind() != ErrorKind::NotFound,
    }
}

fn disposition_for_finding(
    task: &TaskRecord,
    request: &QaRunRequestV1,
    finding: &crate::runtime_qa::QaFindingV1,
) -> QaFindingDispositionV1 {
    let (kind, status, owner_item_uid, reason) = if finding.blocking {
        (
            QaFindingDispositionKind::RepairInScope,
            QaFindingDispositionStatus::Assigned,
            Some(task.task_uid.clone()),
            format!(
                "Blocking mismatch is tied to approved requirement(s) {}; repair the exact reviewed task scope. Smallest fix: {}",
                finding.requirement_uids.join(", "),
                finding.smallest_fix
            ),
        )
    } else {
        (
            QaFindingDispositionKind::AskUser,
            QaFindingDispositionStatus::AwaitingAnswer,
            None,
            format!(
                "Nonblocking QA observation needs the user's direction before Manager accepts risk or expands scope. Requirement(s): {}. Smallest fix: {}",
                finding.requirement_uids.join(", "),
                finding.smallest_fix
            ),
        )
    };
    QaFindingDispositionV1 {
        schema_version: 1,
        finding_id: finding.finding_uid.clone(),
        qa_run_id: request.qa_run_id.clone(),
        parent_task_id: task.task_uid.clone(),
        code_sha: request.code_head_sha.clone(),
        kind,
        status,
        reason,
        owner_item_uid,
        impacted_requirements: finding.requirement_uids.clone(),
        decision_id: None,
        created_revision: task.revision,
        expected_revision: task.revision,
    }
}

fn validate_request_identity(
    expected: &QaRunRequestV1,
    persisted: &QaRunRequestV1,
) -> Result<(), String> {
    if expected != persisted {
        return Err("persisted runtime QA report does not match its queued request".into());
    }
    Ok(())
}

#[cfg(test)]
#[path = "reports/tests.rs"]
mod tests;
