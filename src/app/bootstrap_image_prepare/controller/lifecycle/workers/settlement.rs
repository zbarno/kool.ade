use std::sync::mpsc;

use crate::persistence::setup_tasks::FailureCode;

use super::super::super::AttemptContext;
use super::super::super::Controller;
use super::super::super::WorkKind;
use super::super::Completion;
use super::super::RecoveryAction;
use super::super::recovery::{recover_await_verification_write, settle_attempt_transition};

impl Controller {
    pub(crate) fn begin_cancel(&mut self) {
        if self.receiver.is_some() {
            return;
        }
        let Some(attempt) = self.attempt.clone() else {
            return;
        };
        self.recovery_action = None;
        self.spawn_settlement(attempt, RecoveryAction::Cancel);
    }

    pub(in crate::app::bootstrap_image_prepare::controller) fn begin_recovery(
        &mut self,
        action: RecoveryAction,
    ) {
        if self.receiver.is_some() {
            return;
        }
        let Some(attempt) = self.attempt.clone() else {
            self.recovery_action = None;
            return;
        };
        self.recovery_action = None;
        if action == RecoveryAction::ReconcileVerification {
            self.spawn_verification_recovery(attempt);
            return;
        }
        self.spawn_settlement(attempt, action);
    }

    pub(in crate::app::bootstrap_image_prepare::controller) fn record_failure_async(
        &mut self,
        attempt: AttemptContext,
        failure: FailureCode,
    ) {
        self.spawn_settlement(attempt, RecoveryAction::Fail(failure));
    }

    fn spawn_settlement(&mut self, attempt: AttemptContext, action: RecoveryAction) {
        let (sender, receiver) = mpsc::channel();
        self.receiver = Some(receiver);
        self.work_kind = Some(match action {
            RecoveryAction::Cancel => WorkKind::Cancel,
            RecoveryAction::Fail(_) => WorkKind::Prepare,
            RecoveryAction::ReconcileVerification => WorkKind::Prepare,
        });
        std::thread::spawn(move || {
            let settled = settle_attempt_transition(
                &attempt,
                action,
                crate::persistence::setup_tasks::apply_transition,
                crate::persistence::setup_tasks::load,
            );
            let completion = match action {
                RecoveryAction::Cancel => Completion::Cancelled {
                    store_update: settled.store_update,
                    resolution: settled.resolution,
                },
                RecoveryAction::Fail(_) => Completion::FailureRecorded {
                    store_update: settled.store_update,
                    resolution: settled.resolution,
                },
                RecoveryAction::ReconcileVerification => {
                    unreachable!("verification recovery uses its dedicated worker")
                }
            };
            let _ = sender.send(completion);
        });
    }

    fn spawn_verification_recovery(&mut self, attempt: AttemptContext) {
        let (sender, receiver) = mpsc::channel();
        self.receiver = Some(receiver);
        self.work_kind = Some(WorkKind::Prepare);
        std::thread::spawn(move || {
            let (succeeded, settled) = recover_await_verification_write(
                &attempt,
                crate::persistence::setup_tasks::load,
                crate::persistence::setup_tasks::apply_transition,
            );
            let _ = sender.send(Completion::VerificationRecovered {
                succeeded,
                store_update: settled.store_update,
                resolution: settled.resolution,
            });
        });
    }
}
