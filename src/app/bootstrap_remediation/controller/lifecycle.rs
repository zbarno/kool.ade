use std::sync::mpsc::{self, SyncSender, TryRecvError};

use crate::persistence::setup_tasks::{self, FailureCode, SetupTaskStoreV1, SetupTaskTransitionV1};

use super::super::{
    ApprovalGate, ApprovalPreview, ApprovalRequest, AttemptContext, RemediationPlan,
};
use super::{Controller, WorkKind};

mod workers;

pub(in crate::app::bootstrap_remediation) enum Completion {
    Preview {
        result: Box<Result<(RemediationPlan, AttemptContext, ApprovalPreview), ()>>,
        store_update: Option<SetupTaskStoreV1>,
    },
    AuthorizationPersisted {
        store_update: SetupTaskStoreV1,
        acknowledgement: SyncSender<bool>,
    },
    Executed {
        gate: ApprovalGate,
        result: Result<(), ()>,
        store_update: Option<SetupTaskStoreV1>,
    },
    Cancelled {
        succeeded: bool,
        store_update: Option<SetupTaskStoreV1>,
    },
}

impl Controller {
    pub(crate) fn is_busy(&self) -> bool {
        self.receiver.is_some() || self.preview.is_some() || self.pending_approval.is_some()
    }

    pub(crate) fn request_review(&mut self, task_id: &str) {
        if !super::proposal::is_supported_task(task_id) || self.is_busy() {
            return;
        }
        self.result = None;
        self.attempt = None;
        self.plan = None;
        self.preview = None;
        self.pending_approval = None;
        workers::begin_preview(self, task_id.to_owned());
    }

    pub(crate) fn take_approval(&mut self, task_id: &str) -> Option<ApprovalRequest> {
        let request = self.pending_approval.take()?;
        (request.plan.prerequisite_id == task_id).then_some(request)
    }

    pub(crate) fn approve(&mut self, request: ApprovalRequest) {
        if self.receiver.is_some() {
            return;
        }
        let matches_preview = self.plan.as_ref().is_some_and(|plan| plan == &request.plan)
            && self
                .attempt
                .as_ref()
                .is_some_and(|attempt| attempt == &request.attempt);
        if !matches_preview {
            self.result = Some(Err("The repair preview changed. Review it again.".into()));
            return;
        }
        workers::begin_execution(self, request);
    }

    pub(crate) fn cancel_review(&mut self) {
        if self.receiver.is_some() {
            return;
        }
        self.preview = None;
        self.pending_approval = None;
        self.plan = None;
        if let Some(attempt) = self.attempt.clone() {
            workers::begin_cancel(self, attempt);
        }
    }

    pub(crate) fn take_store_update(&mut self) -> Option<SetupTaskStoreV1> {
        self.store_update.take()
    }

    pub(crate) fn acknowledge_authorization_published(
        &mut self,
        published: bool,
        tasks: &[crate::persistence::setup_tasks::LocalTask],
    ) {
        if let Some(acknowledgement) = self.authorization_ack.take() {
            let is_current = self.attempt.as_ref().is_some_and(|attempt| {
                tasks
                    .iter()
                    .find(|task| task.id == attempt.task_id())
                    .is_some_and(|task| {
                        task.state == crate::persistence::setup_tasks::State::InProgress
                            && task.attempts.iter().any(|record| {
                                record.operation_id == attempt.operation_id()
                                    && record.state
                                        == crate::persistence::setup_tasks::AttemptState::InProgress
                            })
                    })
            });
            let _ = acknowledgement.send(published && is_current);
        }
    }

    pub(crate) fn poll(&mut self, ctx: &egui::Context) -> bool {
        self.poll_with(ctx, |attempt| {
            settle_disconnected_attempt(
                attempt,
                crate::persistence::harness_settings::current_unix_ms(),
                setup_tasks::apply_transition,
                setup_tasks::load,
            )
        })
    }

    fn poll_with(
        &mut self,
        ctx: &egui::Context,
        settle: impl FnOnce(Option<&AttemptContext>) -> Option<SetupTaskStoreV1>,
    ) -> bool {
        let Some(receiver) = &self.receiver else {
            return false;
        };
        match receiver.try_recv() {
            Ok(Completion::Preview {
                result,
                store_update,
            }) => {
                self.store_update = store_update;
                match *result {
                    Ok((plan, attempt, preview)) => {
                        self.plan = Some(plan);
                        self.attempt = Some(attempt);
                        self.preview = Some(preview);
                    }
                    Err(()) => {
                        self.result = Some(Err(
                            "Could not prepare a safe repair preview. Check setup history and retry the setup check."
                                .into(),
                        ));
                        self.attempt = None;
                        self.plan = None;
                    }
                }
                self.receiver = None;
                self.work = None;
                ctx.request_repaint();
                false
            }
            Ok(Completion::AuthorizationPersisted {
                store_update,
                acknowledgement,
            }) => {
                self.store_update = Some(store_update);
                self.authorization_ack = Some(acknowledgement);
                ctx.request_repaint();
                false
            }
            Ok(Completion::Executed {
                gate,
                result,
                store_update,
            }) => {
                self.gate = Some(gate);
                self.store_update = store_update;
                self.result = Some(if result.is_ok() {
                    Ok(())
                } else {
                    Err("The approved repair did not complete. Check setup history, then retry the setup check.".into())
                });
                self.attempt = None;
                self.plan = None;
                self.preview = None;
                self.pending_approval = None;
                self.receiver = None;
                self.work = None;
                ctx.request_repaint();
                true
            }
            Ok(Completion::Cancelled {
                succeeded,
                store_update,
            }) => {
                self.store_update = store_update;
                self.attempt = None;
                self.plan = None;
                self.preview = None;
                self.pending_approval = None;
                self.receiver = None;
                self.work = None;
                if !succeeded {
                    self.result = Some(Err(
                        "Could not save the review cancellation. Retry the setup check.".into(),
                    ));
                }
                ctx.request_repaint();
                true
            }
            Err(TryRecvError::Empty) => false,
            Err(TryRecvError::Disconnected) => {
                let attempt = self.attempt.take();
                self.store_update = settle(attempt.as_ref());
                if let Some(acknowledgement) = self.authorization_ack.take() {
                    let _ = acknowledgement.send(false);
                }
                self.gate.get_or_insert_with(ApprovalGate::default);
                self.plan = None;
                self.preview = None;
                self.pending_approval = None;
                self.receiver = None;
                self.work = None;
                self.result = Some(Err(
                    "The repair stopped before reporting an outcome. Check setup history and retry the setup check."
                        .into(),
                ));
                ctx.request_repaint();
                true
            }
        }
    }

    pub(super) fn set_receiver(&mut self, receiver: mpsc::Receiver<Completion>, kind: WorkKind) {
        self.receiver = Some(receiver);
        self.work = Some(kind);
    }
}

fn settle_disconnected_attempt(
    attempt: Option<&AttemptContext>,
    at_unix_ms: u64,
    apply: impl FnOnce(SetupTaskTransitionV1) -> anyhow::Result<SetupTaskStoreV1>,
    load: impl FnOnce() -> anyhow::Result<SetupTaskStoreV1>,
) -> Option<SetupTaskStoreV1> {
    let attempt = attempt?;
    let transition = SetupTaskTransitionV1::FailAttempt {
        task_id: attempt.task_id().to_owned(),
        operation_id: attempt.operation_id().to_owned(),
        failure: FailureCode::Unknown,
        at_unix_ms,
    };
    apply(transition).ok().or_else(|| load().ok())
}

#[cfg(test)]
mod tests;
