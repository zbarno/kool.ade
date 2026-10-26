use std::sync::mpsc::SyncSender;

use crate::app::bootstrap_approval::{ApprovalGate, ApprovalPreview};
use crate::app::bootstrap_repair_policy::Proposal;
use crate::persistence::setup_tasks::{FailureCode, SetupTaskStoreV1};

mod recovery;
mod steps;
mod workers;

#[cfg(test)]
mod tests;

pub(super) use recovery::AttemptResolution;

pub(super) enum Completion {
    Preview {
        result: Result<Box<(Proposal, ApprovalPreview)>, String>,
        store_update: Option<SetupTaskStoreV1>,
        resolution: Option<AttemptResolution>,
    },
    AuthorizationPersisted {
        store_update: SetupTaskStoreV1,
        acknowledgement: SyncSender<bool>,
    },
    Prepared {
        gate: ApprovalGate,
        succeeded: bool,
        store_update: Option<SetupTaskStoreV1>,
        resolution: Option<AttemptResolution>,
    },
    VerificationRecovered {
        succeeded: bool,
        store_update: Option<SetupTaskStoreV1>,
        resolution: AttemptResolution,
    },
    Cancelled {
        store_update: Option<SetupTaskStoreV1>,
        resolution: AttemptResolution,
    },
    FailureRecorded {
        store_update: Option<SetupTaskStoreV1>,
        resolution: AttemptResolution,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum RecoveryAction {
    Cancel,
    Fail(FailureCode),
    ReconcileVerification,
}

pub(super) fn record_unexpected_failure(controller: &mut super::Controller) {
    controller.gate.get_or_insert_with(ApprovalGate::new);
    controller.preview = None;
    controller.proposal = None;
    controller.pending_approval = None;
    controller.recovery_action = None;
    if let Some(acknowledgement) = controller.authorization_publication_ack.take() {
        let _ = acknowledgement.send(false);
    }
    if let Some(attempt) = controller.attempt.clone() {
        controller.record_failure_async(attempt, FailureCode::Unknown);
    } else {
        controller.result = Some(Err(
            "The repair stopped before reporting an outcome. Check setup history and retry the setup check."
                .into(),
        ));
    }
}
