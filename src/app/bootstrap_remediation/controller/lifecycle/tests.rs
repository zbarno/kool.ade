use super::*;
use crate::app::bootstrap_repair_policy::{Host, Operation, Proposal};

fn attempt() -> AttemptContext {
    AttemptContext {
        task_id: "setup:linux:rootless:packages".into(),
        operation_id: "00000000-0000-4000-8000-000000000001".into(),
        owner_instance_id: "synthetic-owner".into(),
        requested_scopes: vec!["setup:linux:rootless:packages".into()],
        scope_digest: "a".repeat(64),
    }
}

fn plan() -> RemediationPlan {
    let identity = crate::app::bootstrap_remediation::identity::HostIdentity::from_facts(
        Host::Linux,
        "ubuntu-24.04-amd64-kernel-6.8.0",
        "synthetic-machine",
        "synthetic-user",
    )
    .unwrap();
    let proposal = Proposal {
        schema_version: 1,
        operation: Operation::InstallLinuxRootlessPackages,
        host: Host::Linux,
        prerequisite_id: "setup:linux:rootless:packages".into(),
        image_profile: None,
    };
    let request = crate::app::bootstrap_remediation::RemediationRequest::from_proposal_for_test(
        &proposal, &identity,
    )
    .unwrap();
    RemediationPlan::build_for_test(&request, &identity).unwrap()
}

#[test]
fn disconnected_worker_fails_attempt_and_clears_controller_work() {
    let (sender, receiver) = mpsc::channel();
    drop(sender);
    let mut controller = Controller {
        attempt: Some(attempt()),
        gate: None,
        work: Some(WorkKind::Execute),
        plan: Some(plan()),
        ..Controller::default()
    };
    controller.set_receiver(receiver, WorkKind::Execute);
    let mut persisted_transition = None;
    let ctx = egui::Context::default();

    let completed = controller.poll_with(&ctx, |attempt| {
        settle_disconnected_attempt(
            attempt,
            42,
            |transition| {
                persisted_transition = Some(transition);
                Ok(SetupTaskStoreV1::default())
            },
            || unreachable!("a successful transition does not need a reload"),
        )
    });

    assert!(completed);
    assert!(controller.attempt.is_none());
    assert!(controller.plan.is_none());
    assert!(controller.preview.is_none());
    assert!(controller.pending_approval.is_none());
    assert!(controller.receiver.is_none());
    assert!(controller.work.is_none());
    assert!(controller.store_update.is_some());
    assert!(controller.gate.is_some());
    assert!(matches!(
        persisted_transition,
        Some(SetupTaskTransitionV1::FailAttempt {
            task_id,
            operation_id,
            failure: FailureCode::Unknown,
            at_unix_ms: 42,
        }) if task_id == "setup:linux:rootless:packages"
            && operation_id == "00000000-0000-4000-8000-000000000001"
    ));
}
