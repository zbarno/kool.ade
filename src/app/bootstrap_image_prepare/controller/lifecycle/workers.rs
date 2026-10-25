use std::sync::mpsc;

use crate::app::bootstrap_approval::ApprovalRequest;
use crate::persistence::setup_tasks::{
    AuthorizationBindingV1, FailureCode, SetupTaskTransitionV1, authorization_scope_digest,
};

use super::super::AttemptContext;
use super::super::Controller;
use super::super::WorkKind;
use super::super::lifecycle::Completion;
use super::recovery::{AttemptResolution, recover_await_verification_write, settle_failed_attempt};
use super::steps::{PipelineFailureKind, authorize_publish_execute};

mod settlement;

impl Controller {
    pub(in crate::app::bootstrap_image_prepare::controller) fn begin_preview(
        &mut self,
        attempt: AttemptContext,
        persist_request: bool,
    ) {
        if self.receiver.is_some() {
            return;
        }
        self.result = None;
        self.preview = None;
        self.proposal = None;
        self.pending_approval = None;
        let (sender, receiver) = mpsc::channel();
        self.receiver = Some(receiver);
        self.work_kind = Some(WorkKind::Preview);
        std::thread::spawn(move || {
            let request_store = if persist_request {
                crate::persistence::setup_tasks::apply_transition(
                    SetupTaskTransitionV1::RequestAttempt {
                        task_id: attempt.task_id.clone(),
                        operation_id: attempt.operation_id.clone(),
                        version: attempt.version,
                        requested_scopes: attempt.requested_scopes.clone(),
                        at_unix_ms: now_ms(),
                    },
                )
                .ok()
            } else {
                None
            };

            let request_persisted = !persist_request || request_store.is_some();
            let result = if request_persisted {
                super::super::super::executor::preview(&attempt.task_id).map(Box::new)
            } else {
                Err("request persistence failed".into())
            };
            let (store_update, resolution) = if result.is_ok() {
                (request_store, None)
            } else {
                let settled = settle_failed_attempt(
                    &attempt,
                    if request_persisted {
                        FailureCode::OperationFailed
                    } else {
                        FailureCode::Unknown
                    },
                    crate::persistence::setup_tasks::apply_transition,
                    crate::persistence::setup_tasks::load,
                );
                (
                    settled.store_update.or(request_store),
                    Some(settled.resolution),
                )
            };
            let _ = sender.send(Completion::Preview {
                result,
                store_update,
                resolution,
            });
        });
    }

    pub(crate) fn begin_prepare(&mut self, request: ApprovalRequest) {
        if self.receiver.is_some() {
            return;
        }
        let Some(attempt) = self.attempt.clone() else {
            self.result = Some(Err("The repair request expired. Review it again.".into()));
            return;
        };
        let Some(proposal) = self.proposal.clone() else {
            self.result = Some(Err("The repair preview expired. Review it again.".into()));
            self.cancel_review();
            return;
        };
        if request.proposal() != &proposal || proposal.prerequisite_id != attempt.task_id {
            self.result = Some(Err("The repair preview changed. Review it again.".into()));
            self.cancel_review();
            return;
        }
        let Some(mut gate) = self.gate.take() else {
            self.result = Some(Err("Another image preparation is already active.".into()));
            self.record_failure_async(attempt, FailureCode::Unknown);
            return;
        };
        self.preview = None;
        self.result = None;
        self.recovery_action = None;
        let (sender, receiver) = mpsc::channel();
        self.receiver = Some(receiver);
        self.work_kind = Some(WorkKind::Prepare);
        std::thread::spawn(move || {
            let runtime = super::super::super::executor::runtime();
            let mut ticket = None;
            let result = match runtime {
                Ok(runtime) => match gate.grant(request) {
                    Ok(approval_ticket) => {
                        ticket = Some(approval_ticket);
                        let binding = AuthorizationBindingV1 {
                            receipt_id: approval_ticket.to_string(),
                            scope_digest: authorization_scope_digest(
                                &attempt.task_id,
                                &attempt.operation_id,
                                attempt.version,
                                &attempt.requested_scopes,
                            ),
                            authorized_at_unix_ms: now_ms(),
                        };
                        let pipeline = authorize_publish_execute(
                            &attempt,
                            binding,
                            crate::persistence::setup_tasks::apply_transition,
                            |store_update| {
                                let (acknowledgement, await_publication) = mpsc::sync_channel(1);
                                if sender
                                    .send(Completion::AuthorizationPersisted {
                                        store_update,
                                        acknowledgement,
                                    })
                                    .is_err()
                                {
                                    return false;
                                }
                                await_publication.recv().unwrap_or(false)
                            },
                            || {
                                let action = runtime
                                    .block_on(gate.consume(approval_ticket, &proposal))
                                    .map_err(|_| ())?;
                                super::super::super::executor::prepare_approved(action, &proposal)
                                    .map_err(|_| ())
                            },
                        );
                        drop(runtime);
                        pipeline
                    }
                    Err(_) => {
                        drop(runtime);
                        Err(super::steps::PipelineFailure {
                            kind: PipelineFailureKind::OperationFailed,
                            store_update: None,
                        })
                    }
                },
                Err(_) => Err(super::steps::PipelineFailure {
                    kind: PipelineFailureKind::OperationFailed,
                    store_update: None,
                }),
            };

            let completion = match result {
                Ok(store) => Completion::Prepared {
                    gate,
                    succeeded: true,
                    store_update: Some(store),
                    resolution: Some(AttemptResolution::Settled),
                },
                Err(failure) => {
                    if let Some(ticket) = ticket {
                        let _ = gate.cancel(ticket);
                    }
                    let failure_code = match failure.kind {
                        PipelineFailureKind::OperationFailed => FailureCode::OperationFailed,
                        PipelineFailureKind::AuthorizationPersistence
                        | PipelineFailureKind::AuthorizationNotPublished
                        | PipelineFailureKind::AwaitVerificationPersistence => FailureCode::Unknown,
                    };
                    let (succeeded, settled) =
                        if failure.kind == PipelineFailureKind::AwaitVerificationPersistence {
                            recover_await_verification_write(
                                &attempt,
                                crate::persistence::setup_tasks::load,
                                crate::persistence::setup_tasks::apply_transition,
                            )
                        } else {
                            let settled = settle_failed_attempt(
                                &attempt,
                                failure_code,
                                crate::persistence::setup_tasks::apply_transition,
                                crate::persistence::setup_tasks::load,
                            );
                            (false, settled)
                        };
                    Completion::Prepared {
                        gate,
                        succeeded,
                        store_update: settled.store_update.or(failure.store_update),
                        resolution: Some(settled.resolution),
                    }
                }
            };
            let _ = sender.send(completion);
        });
    }
}

fn now_ms() -> u64 {
    crate::persistence::harness_settings::current_unix_ms()
}
