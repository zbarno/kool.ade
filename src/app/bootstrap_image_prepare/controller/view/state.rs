#[cfg(test)]
use std::sync::mpsc;
use std::sync::mpsc::TryRecvError;

#[cfg(test)]
use crate::app::bootstrap_approval::ApprovalGate;
use crate::app::bootstrap_approval::ApprovalRequest;
use crate::persistence::setup_tasks::{AttemptState, SetupTaskStateV1, SetupTaskStoreV1};

use super::super::lifecycle::{AttemptResolution, Completion, RecoveryAction};
use super::super::{AttemptContext, Controller, WorkKind};

impl Controller {
    pub(crate) fn request_review(&mut self, prerequisite_id: &str) {
        if !prerequisite_id.starts_with("setup:images:general:") {
            self.result = Some(Err(
                "There is no approved repair for this setup task.".into()
            ));
            return;
        }
        if self.receiver.is_some() || self.recovery_action.is_some() {
            return;
        }
        self.pending_approval = None;
        self.result = None;
        if let Some(attempt) = self.attempt.clone()
            && attempt.task_id == prerequisite_id
        {
            self.begin_preview(attempt, false);
            return;
        }
        let attempt = AttemptContext {
            task_id: prerequisite_id.to_owned(),
            operation_id: uuid::Uuid::new_v4().to_string(),
            version: 1,
            requested_scopes: vec![prerequisite_id.to_owned()],
        };
        self.attempt = Some(attempt.clone());
        self.begin_preview(attempt, true);
    }

    pub(crate) fn approve(&mut self, request: ApprovalRequest) {
        self.begin_prepare(request);
    }

    pub(crate) fn take_approval(&mut self, prerequisite_id: &str) -> Option<ApprovalRequest> {
        let request = self.pending_approval.take()?;
        (request.proposal().prerequisite_id == prerequisite_id).then_some(request)
    }

    pub(crate) fn cancel_review(&mut self) {
        if self.receiver.is_some() {
            return;
        }
        self.preview = None;
        self.proposal = None;
        self.pending_approval = None;
        self.begin_cancel();
    }

    pub(crate) fn take_store_update(&mut self) -> Option<SetupTaskStoreV1> {
        self.store_update.take()
    }

    pub(crate) fn acknowledge_authorization_published(
        &mut self,
        published: bool,
        tasks: &[crate::persistence::setup_tasks::LocalTask],
    ) {
        if let Some(acknowledgement) = self.authorization_publication_ack.take() {
            let is_current = self.attempt.as_ref().is_some_and(|attempt| {
                tasks
                    .iter()
                    .find(|task| task.id == attempt.task_id)
                    .is_some_and(|task| {
                        task.state == SetupTaskStateV1::InProgress
                            && task.attempts.iter().any(|record| {
                                record.operation_id == attempt.operation_id
                                    && record.state == AttemptState::InProgress
                            })
                    })
            });
            let _ = acknowledgement.send(published && is_current);
        }
    }

    pub(crate) fn poll(&mut self, ctx: &egui::Context) -> bool {
        let Some(receiver) = &self.receiver else {
            return false;
        };
        match receiver.try_recv() {
            Ok(Completion::Preview {
                result,
                store_update,
                resolution,
            }) => {
                self.set_store_update(store_update);
                if let Some(resolution) = resolution {
                    self.apply_resolution(resolution);
                }
                match result {
                    Ok(preview) => {
                        let (proposal, preview) = *preview;
                        self.proposal = Some(proposal);
                        self.preview = Some(preview);
                    }
                    Err(_) => {
                        self.result = Some(Err(
                            "Could not prepare the repair preview. Check setup history and try again."
                                .into(),
                        ));
                        self.preview = None;
                        self.proposal = None;
                    }
                }
                self.receiver = None;
                self.work_kind = None;
                ctx.request_repaint();
                false
            }
            Ok(Completion::AuthorizationPersisted {
                store_update,
                acknowledgement,
            }) => {
                self.store_update = Some(store_update);
                self.authorization_publication_ack = Some(acknowledgement);
                self.work_kind = Some(WorkKind::Prepare);
                ctx.request_repaint();
                false
            }
            Ok(Completion::Prepared {
                gate,
                succeeded,
                store_update,
                resolution,
            }) => {
                self.gate = Some(gate);
                self.set_store_update(store_update);
                if let Some(resolution) = resolution {
                    self.apply_resolution(resolution);
                }
                self.result = Some(if succeeded {
                    Ok(())
                } else {
                    Err("The approved repair did not complete. Check setup history, then retry the setup check.".into())
                });
                self.receiver = None;
                self.work_kind = None;
                crate::execution::docker::environment::DockerEnvironmentStatus::invalidate();
                crate::harness::runtime_capabilities::RuntimeCapabilities::invalidate();
                ctx.request_repaint();
                succeeded
            }
            Ok(Completion::VerificationRecovered {
                succeeded,
                store_update,
                resolution,
            }) => {
                self.set_store_update(store_update);
                self.apply_resolution(resolution);
                self.result = Some(if succeeded {
                    Ok(())
                } else {
                    Err("The repair history could not be reconciled. Retry checking it before continuing.".into())
                });
                self.receiver = None;
                self.work_kind = None;
                crate::execution::docker::environment::DockerEnvironmentStatus::invalidate();
                crate::harness::runtime_capabilities::RuntimeCapabilities::invalidate();
                ctx.request_repaint();
                succeeded
            }
            Ok(Completion::Cancelled {
                store_update,
                resolution,
            })
            | Ok(Completion::FailureRecorded {
                store_update,
                resolution,
            }) => {
                self.set_store_update(store_update);
                self.apply_resolution(resolution);
                self.result = match self.recovery_action {
                    Some(RecoveryAction::Cancel) => Some(Err(
                        "Could not save the cancellation to setup history. Retry the cancellation."
                            .into(),
                    )),
                    Some(RecoveryAction::Fail(_)) => Some(Err(
                        "Could not save the repair outcome. Retry saving it before continuing."
                            .into(),
                    )),
                    Some(RecoveryAction::ReconcileVerification) => Some(Err(
                        "Could not confirm repair history. Retry checking repair history.".into(),
                    )),
                    None => None,
                };
                self.receiver = None;
                self.work_kind = None;
                ctx.request_repaint();
                false
            }
            Err(TryRecvError::Empty) => false,
            Err(TryRecvError::Disconnected) => {
                self.receiver = None;
                self.work_kind = None;
                super::super::lifecycle::record_unexpected_failure(self);
                ctx.request_repaint();
                false
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn queue_success_for_test(&mut self) {
        let (sender, receiver) = mpsc::channel();
        self.receiver = Some(receiver);
        self.work_kind = Some(WorkKind::Prepare);
        sender
            .send(Completion::Prepared {
                gate: ApprovalGate::new(),
                succeeded: true,
                store_update: None,
                resolution: Some(AttemptResolution::Settled),
            })
            .unwrap();
    }

    fn set_store_update(&mut self, store_update: Option<SetupTaskStoreV1>) {
        if let Some(store) = store_update {
            self.store_update = Some(store);
        }
    }

    pub(in crate::app::bootstrap_image_prepare::controller) fn apply_resolution(
        &mut self,
        resolution: AttemptResolution,
    ) {
        match resolution {
            AttemptResolution::Settled => {
                self.attempt = None;
                self.recovery_action = None;
                self.preview = None;
                self.proposal = None;
                self.pending_approval = None;
            }
            AttemptResolution::Active(action) => {
                self.recovery_action = Some(action);
                self.preview = None;
                self.proposal = None;
                self.pending_approval = None;
            }
        }
    }
}
