use crate::persistence::setup_tasks::{AttemptState, LocalTask};

pub(super) fn paint(ui: &mut egui::Ui, task: &LocalTask) {
    ui.label(crate::ui::theme::section_heading("Setup history"));
    if task.events.is_empty() && task.attempts.is_empty() {
        ui.label(crate::ui::theme::helper_text(
            "No setup changes have been recorded yet.",
        ));
        return;
    }

    for event in task.events.iter().rev().take(4).rev() {
        ui.horizontal_wrapped(|ui| {
            ui.label(event_state_label(event.state));
            ui.label(crate::ui::theme::metadata_text(format!(
                "{} · check {}",
                timestamp(event.observed_at_unix_ms),
                event.scan_generation
            )));
        });
    }
    for attempt in task.attempts.iter().rev().take(3) {
        ui.horizontal_wrapped(|ui| {
            ui.label(attempt_label(attempt.state));
            ui.label(crate::ui::theme::metadata_text(timestamp(
                attempt.updated_at_unix_ms,
            )));
        });
        if let Some(failure) = attempt.failure {
            ui.label(crate::ui::theme::helper_text(failure_label(failure)));
        }
    }
}

pub(super) fn technical(ui: &mut egui::Ui, task: &LocalTask) {
    ui.collapsing("Technical details and redacted logs", |ui| {
        ui.label(crate::ui::theme::metadata_text(format!("Task key: {}", task.id)));
        if task.id == "setup:docker:rootless" {
            ui.label(crate::ui::theme::metadata_text(
                "Docker Engine mode: rootless daemon with a user-owned socket.",
            ));
        }
        ui.label(crate::ui::theme::metadata_text(format!(
            "Readiness check: {}",
            task.scan_generation
        )));
        if let Some(result) = &task.verified_result {
            ui.label(crate::ui::theme::metadata_text(format!(
                "Verified evidence: {} · check {} · {}",
                result.evidence_domain,
                result.scan_generation,
                timestamp(result.verified_at_unix_ms)
            )));
        }
        for attempt in task.attempts.iter().rev().take(3) {
            ui.label(crate::ui::theme::metadata_text(format!(
                "Operation {} · requested scope count {}",
                attempt.operation_id,
                attempt.requested_scopes.len()
            )));
        }
        ui.label(crate::ui::theme::helper_text(
            "The setup store keeps typed status only. It does not save host command output, credentials, or unredacted logs.",
        ));
    });
}

fn attempt_label(state: AttemptState) -> &'static str {
    match state {
        AttemptState::NeedsApproval => "Waiting for approval",
        AttemptState::InProgress => "Repair in progress",
        AttemptState::AwaitingVerification => "Waiting for verification",
        AttemptState::Verified => "Repair verified",
        AttemptState::Failed => "Repair did not finish",
        AttemptState::Cancelled => "Repair cancelled",
        AttemptState::Paused => "Repair paused",
        AttemptState::Interrupted => "Repair interrupted",
    }
}

fn event_state_label(state: crate::persistence::setup_tasks::State) -> &'static str {
    use crate::persistence::setup_tasks::State;
    match state {
        State::Detected => "Detected",
        State::ReadyToFix => "Ready to fix",
        State::BlockedByPrerequisite => "Blocked",
        State::NeedsApproval => "Needs approval",
        State::InProgress => "In progress",
        State::AwaitingVerification => "Awaiting verification",
        State::NeedsVerification => "Needs verification",
        State::Verified => "Verified",
        State::Failed => "Failed",
        State::Unsupported => "Unavailable",
        State::Cancelled => "Cancelled",
        State::Paused => "Paused",
        State::Interrupted => "Interrupted",
        State::Superseded => "Replaced",
    }
}

fn failure_label(code: crate::persistence::setup_tasks::FailureCode) -> &'static str {
    use crate::persistence::setup_tasks::FailureCode;
    match code {
        FailureCode::OperationFailed => "The repair could not finish.",
        FailureCode::VerificationFailed => "The follow-up check did not confirm the repair.",
        FailureCode::PermissionDenied => "The required permission was not granted.",
        FailureCode::CancelledByUser => "You cancelled this repair.",
        FailureCode::InterruptedByRestart => "The app closed before the repair finished.",
        FailureCode::SupersededByReadiness => "A newer readiness check replaced this repair.",
        FailureCode::PrerequisiteRegressed => "A required setup step needs attention again.",
        FailureCode::Unknown => "The repair stopped for an unclassified reason.",
    }
}

fn timestamp(milliseconds: u64) -> String {
    chrono::DateTime::from_timestamp_millis(milliseconds.min(i64::MAX as u64) as i64)
        .map(|time| {
            time.with_timezone(&chrono::Local)
                .format("%b %-d, %H:%M")
                .to_string()
        })
        .unwrap_or_else(|| "time unavailable".into())
}
