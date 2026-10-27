use std::{sync::mpsc, thread};

use crate::persistence::setup_tasks::{FailureCode, SetupTaskTransitionV1, apply_transition, load};

use super::super::super::{
    ApprovalGate, ApprovalPreview, ApprovalRequest, AttemptContext, Cancellation,
    LinuxNativeElevation, LinuxReadinessVerifier, ProductionOperationRegistry, RemediationPlan,
    RemediationRequest, RemediationSupervisor, SystemCommandRunner,
};
use super::super::{Controller, WorkKind};
use super::Completion;

pub(super) fn begin_preview(controller: &mut Controller, task_id: String) {
    let (sender, receiver) = mpsc::channel();
    controller.set_receiver(receiver, WorkKind::Preview);
    thread::spawn(move || {
        let mut attempt = None;
        let result: anyhow::Result<_> = (|| {
            let proposal = super::super::proposal::proposal_for_task(&task_id)
                .ok_or_else(|| anyhow::anyhow!("unsupported setup task"))?;
            let request = RemediationRequest::from_proposal(&proposal)?;
            let plan = RemediationPlan::build(&request)?;
            let started = AttemptContext::begin(&plan)?;
            attempt = Some(started.clone());
            let preview = ApprovalPreview::new(plan.clone(), started.clone())?;
            Ok((plan, started, preview))
        })();
        if result.is_err()
            && let Some(attempt) = &attempt
        {
            fail_attempt(attempt, FailureCode::OperationFailed);
        }
        let _ = sender.send(Completion::Preview {
            result: Box::new(result.map_err(|_| ())),
            store_update: load().ok(),
        });
    });
}

pub(super) fn begin_execution(controller: &mut Controller, request: ApprovalRequest) {
    let Some(gate) = controller.gate.take() else {
        controller.result = Some(Err("Another machine repair is already active.".into()));
        return;
    };
    let Some(plan) = controller.plan.clone() else {
        controller.gate = Some(gate);
        controller.result = Some(Err("The repair preview expired. Review it again.".into()));
        return;
    };
    let Some(attempt) = controller.attempt.clone() else {
        controller.gate = Some(gate);
        controller.result = Some(Err("The repair request expired. Review it again.".into()));
        return;
    };
    controller.preview = None;
    controller.pending_approval = None;
    controller.result = None;
    let (sender, receiver) = mpsc::channel();
    controller.set_receiver(receiver, WorkKind::Execute);
    thread::spawn(move || {
        let (gate, result) = execute_approved(gate, request, &plan, &attempt, &sender);
        let _ = sender.send(Completion::Executed {
            gate,
            result,
            store_update: load().ok(),
        });
    });
}

fn execute_approved(
    mut gate: ApprovalGate,
    request: ApprovalRequest,
    plan: &RemediationPlan,
    attempt: &AttemptContext,
    sender: &mpsc::Sender<Completion>,
) -> (ApprovalGate, Result<(), ()>) {
    let binding = match gate.grant(request) {
        Ok(binding) => binding,
        Err(_) => {
            fail_attempt(attempt, FailureCode::OperationFailed);
            return (gate, Err(()));
        }
    };
    let authorized = match attempt.persist_authorization(binding.clone()) {
        Ok(store) => store,
        Err(_) => {
            gate.cancel(&binding.receipt_id);
            fail_attempt(attempt, FailureCode::Unknown);
            return (gate, Err(()));
        }
    };
    let (acknowledge, await_publication) = mpsc::sync_channel(1);
    if sender
        .send(Completion::AuthorizationPersisted {
            store_update: authorized,
            acknowledgement: acknowledge,
        })
        .is_err()
        || !await_publication.recv().unwrap_or(false)
    {
        gate.cancel(&binding.receipt_id);
        fail_attempt(attempt, FailureCode::Unknown);
        return (gate, Err(()));
    }
    let approved = match gate.consume(&binding, plan, attempt) {
        Ok(approved) => approved,
        Err(_) => {
            fail_attempt(attempt, FailureCode::PermissionDenied);
            return (gate, Err(()));
        }
    };
    let result = execute_supervised(approved, plan)
        .map(|_| ())
        .map_err(|_| ());
    (gate, result)
}

fn execute_supervised(
    approved: super::super::super::ApprovedRemediation,
    plan: &RemediationPlan,
) -> anyhow::Result<super::super::super::RemediationResult> {
    let readiness = LinuxReadinessVerifier;
    RemediationSupervisor::execute(
        approved,
        plan,
        &ProductionOperationRegistry,
        &readiness,
        &SystemCommandRunner,
        &LinuxNativeElevation,
        &readiness,
        &Cancellation::new(),
    )
}

pub(super) fn begin_cancel(controller: &mut Controller, attempt: AttemptContext) {
    let (sender, receiver) = mpsc::channel();
    controller.set_receiver(receiver, WorkKind::Cancel);
    thread::spawn(move || {
        let result = apply_transition(SetupTaskTransitionV1::CancelAttempt {
            task_id: attempt.task_id().to_owned(),
            operation_id: attempt.operation_id().to_owned(),
            at_unix_ms: now_ms(),
        });
        let _ = sender.send(Completion::Cancelled {
            succeeded: result.is_ok(),
            store_update: result.ok().or_else(|| load().ok()),
        });
    });
}

fn fail_attempt(attempt: &AttemptContext, failure: FailureCode) {
    let _ = apply_transition(SetupTaskTransitionV1::FailAttempt {
        task_id: attempt.task_id().to_owned(),
        operation_id: attempt.operation_id().to_owned(),
        failure,
        at_unix_ms: now_ms(),
    });
}

fn now_ms() -> u64 {
    crate::persistence::harness_settings::current_unix_ms()
}
