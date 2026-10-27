use std::sync::{Arc, mpsc};

use egui::Context;

use crate::{
    app::{
        bootstrap_remediation::{
            ApprovalGate, ApprovalPreview, ApprovalRequest, AttemptContext,
            ClaudeCodeReadinessVerifier, ClaudeInstallProgress, RemediationPlan,
            RemediationRequest,
            supervisor::{
                Cancellation, ClaudeCodeOperationRegistry, LinuxReadinessVerifier,
                NoNativeElevation, RemediationResult, RemediationSupervisor, SystemCommandRunner,
            },
        },
        bootstrap_repair_policy::{Host, Operation, Proposal},
    },
    persistence::setup_tasks::{
        FailureCode, SetupTaskStoreV1, SetupTaskTransitionV1, apply_transition,
    },
};

mod accessors;
mod attempt;
mod progress;
#[cfg(test)]
mod tests;
use attempt::fail_unapproved;
use progress::advance_progress;

pub(crate) struct ClaudeInstallController {
    gate: ApprovalGate,
    review: Option<Review>,
    worker: Option<Worker>,
    cancellation: Option<Arc<Cancellation>>,
    progress_receiver: Option<mpsc::Receiver<ClaudeInstallProgress>>,
    progress: Option<ClaudeInstallProgress>,
    status: ClaudeInstallStatus,
    store_update: Option<SetupTaskStoreV1>,
    completion: Option<Result<(), String>>,
}

struct Review {
    plan: RemediationPlan,
    attempt: AttemptContext,
    preview: ApprovalPreview,
}

struct Worker {
    result: mpsc::Receiver<anyhow::Result<RemediationResult>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ClaudeInstallStatus {
    Idle,
    NeedsApproval,
    Running,
    Finished(Result<(), String>),
}

impl Default for ClaudeInstallController {
    fn default() -> Self {
        Self {
            gate: ApprovalGate::default(),
            review: None,
            worker: None,
            cancellation: None,
            progress_receiver: None,
            progress: None,
            status: ClaudeInstallStatus::Idle,
            store_update: None,
            completion: None,
        }
    }
}

impl ClaudeInstallController {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn supports_current_host() -> bool {
        super::recipe::supports_current_host()
    }

    pub(crate) fn request_review(&mut self, task_id: &str) -> anyhow::Result<()> {
        anyhow::ensure!(
            Self::supports_current_host(),
            "this host has no reviewed Claude install recipe"
        );
        anyhow::ensure!(
            task_id == "setup:cli:claude:install",
            "unsupported Claude install task"
        );
        anyhow::ensure!(self.worker.is_none(), "a Claude install is already running");
        let proposal = Proposal {
            schema_version: 1,
            operation: Operation::InstallClaudeCode,
            host: Host::Linux,
            prerequisite_id: task_id.to_owned(),
            image_profile: None,
        };
        let request = RemediationRequest::from_proposal(&proposal)?;
        let plan = RemediationPlan::build(&request)?;
        let attempt = AttemptContext::begin(&plan)?;
        let preview = match ApprovalPreview::new(plan.clone(), attempt.clone()) {
            Ok(preview) => preview,
            Err(error) => {
                fail_unapproved(&attempt, FailureCode::OperationFailed)?;
                self.store_update = crate::persistence::setup_tasks::load().ok();
                return Err(error);
            }
        };
        let store = match crate::persistence::setup_tasks::load() {
            Ok(store) => store,
            Err(error) => {
                let _ = fail_unapproved(&attempt, FailureCode::OperationFailed);
                self.store_update = crate::persistence::setup_tasks::load().ok();
                return Err(error);
            }
        };
        self.store_update = Some(store);
        self.status = ClaudeInstallStatus::NeedsApproval;
        self.progress = None;
        self.progress_receiver = None;
        self.completion = None;
        self.review = Some(Review {
            plan,
            attempt,
            preview,
        });
        Ok(())
    }

    pub(crate) fn preview(&self) -> Option<&ApprovalPreview> {
        self.review.as_ref().map(|review| &review.preview)
    }

    pub(crate) fn approve(&mut self, request: ApprovalRequest) -> anyhow::Result<SetupTaskStoreV1> {
        let review = self
            .review
            .take()
            .ok_or_else(|| anyhow::anyhow!("Claude install review is no longer active"))?;
        let binding = match self.gate.grant(request) {
            Ok(binding) => binding,
            Err(error) => {
                self.review = Some(review);
                return Err(error);
            }
        };
        let store = match review.attempt.persist_authorization(binding.clone()) {
            Ok(store) => store,
            Err(error) => {
                self.gate.cancel(&binding.receipt_id);
                let _ = fail_unapproved(&review.attempt, FailureCode::OperationFailed);
                self.store_update = crate::persistence::setup_tasks::load().ok();
                return Err(error);
            }
        };
        let approved = match self.gate.consume(&binding, &review.plan, &review.attempt) {
            Ok(approved) => approved,
            Err(error) => {
                let _ = fail_unapproved(&review.attempt, FailureCode::OperationFailed);
                self.store_update = crate::persistence::setup_tasks::load().ok();
                return Err(error);
            }
        };
        self.store_update = Some(store.clone());
        let (send, receive) = mpsc::channel();
        let (progress_send, progress_receive) = mpsc::channel();
        let registry = ClaudeCodeOperationRegistry::new(progress_send.clone());
        let preconditions = LinuxReadinessVerifier;
        let verifier = ClaudeCodeReadinessVerifier::new(progress_send);
        let plan = review.plan;
        let cancellation = Arc::new(Cancellation::new());
        let worker_cancellation = cancellation.clone();
        let spawned = std::thread::Builder::new()
            .name("koolade-claude-install".into())
            .spawn(move || {
                let result = RemediationSupervisor::execute(
                    approved,
                    &plan,
                    &registry,
                    &preconditions,
                    &SystemCommandRunner,
                    &NoNativeElevation,
                    &verifier,
                    &worker_cancellation,
                );
                let _ = send.send(result);
            });
        if let Err(error) = spawned {
            let _ = apply_transition(SetupTaskTransitionV1::FailAttempt {
                task_id: review.attempt.task_id().to_owned(),
                operation_id: review.attempt.operation_id().to_owned(),
                failure: FailureCode::OperationFailed,
                at_unix_ms: super::super::approval::now_unix_ms()?,
            });
            self.store_update = crate::persistence::setup_tasks::load().ok();
            return Err(error.into());
        }
        self.cancellation = Some(cancellation);
        self.worker = Some(Worker { result: receive });
        self.progress_receiver = Some(progress_receive);
        self.progress = Some(ClaudeInstallProgress::Download);
        self.status = ClaudeInstallStatus::Running;
        self.store_update = Some(store.clone());
        Ok(store)
    }

    pub(crate) fn cancel_review(&mut self) -> anyhow::Result<Option<SetupTaskStoreV1>> {
        self.cancellation
            .as_ref()
            .inspect(|cancellation| cancellation.cancel());
        if let Some(review) = self.review.take() {
            fail_unapproved(&review.attempt, FailureCode::CancelledByUser)?;
            self.status = ClaudeInstallStatus::Idle;
            self.progress = None;
            self.progress_receiver = None;
            self.store_update = Some(crate::persistence::setup_tasks::load()?);
        }
        Ok(self.store_update.clone())
    }

    pub(crate) fn poll(&mut self, ctx: &Context) -> ClaudeInstallStatus {
        self.drain_progress();
        if let Some(worker) = &self.worker {
            match worker.result.try_recv() {
                Ok(result) => {
                    self.drain_progress();
                    self.worker = None;
                    self.cancellation = None;
                    self.progress_receiver = None;
                    self.status = ClaudeInstallStatus::Finished(result.map(|_| ()).map_err(|e| {
                        crate::app::bootstrap_remediation::redact_keyed_values(
                            &crate::error::redact_secrets(&e.to_string()),
                        )
                    }));
                    if let ClaudeInstallStatus::Finished(result) = &self.status {
                        self.completion = Some(result.clone());
                    }
                    self.store_update = crate::persistence::setup_tasks::load().ok();
                    ctx.request_repaint();
                }
                Err(mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(100));
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.worker = None;
                    self.cancellation = None;
                    self.status = ClaudeInstallStatus::Finished(Err(
                        "The Claude install worker stopped before reporting a result.".into(),
                    ));
                    self.completion = Some(Err(
                        "The Claude install worker stopped before reporting a result.".into(),
                    ));
                    self.store_update = crate::persistence::setup_tasks::load().ok();
                    ctx.request_repaint();
                }
            }
        }
        self.status.clone()
    }

    pub(crate) fn take_store_update(&mut self) -> Option<SetupTaskStoreV1> {
        self.store_update.take()
    }

    pub(crate) fn take_completion(&mut self) -> Option<Result<(), String>> {
        self.completion.take()
    }

    fn drain_progress(&mut self) {
        loop {
            let next = self
                .progress_receiver
                .as_ref()
                .map(mpsc::Receiver::try_recv);
            match next {
                Some(Ok(progress)) => advance_progress(&mut self.progress, progress),
                Some(Err(mpsc::TryRecvError::Empty | mpsc::TryRecvError::Disconnected)) | None => {
                    break;
                }
            }
        }
    }
}

impl Drop for ClaudeInstallController {
    fn drop(&mut self) {
        if let Some(cancellation) = &self.cancellation {
            cancellation.cancel();
        }
    }
}
