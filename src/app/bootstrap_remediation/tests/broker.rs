use super::super::*;
use super::support::*;
use crate::{app::bootstrap_repair_policy::Host, persistence::setup_tasks::AttemptState};

mod launch;
mod recovery;

// Production boundary: exercises plan-bound consent, attempt journaling,
// runner dispatch, and independent probe outcomes with fake platform adapters.
// Native machine behavior is left to #346–#351; setup is in-memory and needs no
// Docker, SSH, network, or privileged host service.
#[test]
fn broker_dispatches_only_plan_bound_work_for_each_host_contract() {
    for host in [Host::Linux, Host::MacOs, Host::Windows] {
        let now = now();
        let plan = plan(host, None);
        let identity =
            HostIdentity::from_facts(host, "10.2.0", "synthetic-machine", "synthetic-user")
                .unwrap();
        let mut store = ready_store(&plan.prerequisite_id);
        let mut gate = ApprovalGate::default();
        let (_attempt, approval) = approved(&plan, &identity, &mut store, &mut gate, now);
        let result = RemediationSupervisor::execute_for_test(
            approval,
            &plan,
            &identity,
            &mut store,
            &FakeRegistry(host),
            &FakeRunner(host),
            &FakeElevation { declined: false },
            &FreshReadiness,
            &Cancellation::new(),
            now + 2,
        )
        .unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.verification.ready);
        let attempt = &store.tasks[&plan.prerequisite_id].attempts[0];
        assert_eq!(attempt.state, AttemptState::AwaitingVerification);
        assert!(attempt.authorization_binding.is_some());
        assert!(attempt.authorization_history.is_some());
        assert_eq!(
            attempt
                .remediation_result
                .as_ref()
                .unwrap()
                .verification
                .as_ref()
                .unwrap()
                .probe_ids,
            plan.verification_probe_ids
        );
    }
}

#[test]
fn plan_digest_and_consent_bind_parameters_host_and_operation_version() {
    let linux = plan(Host::Linux, None);
    let now = now();
    let another_host =
        HostIdentity::from_facts(Host::MacOs, "10.2.0", "synthetic-machine", "synthetic-user")
            .unwrap();
    let same_host =
        HostIdentity::from_facts(Host::Linux, "10.2.0", "synthetic-machine", "synthetic-user")
            .unwrap();
    assert_ne!(same_host.digest().unwrap(), another_host.digest().unwrap());
    assert_ne!(
        linux.digest().unwrap(),
        plan(Host::Windows, None).digest().unwrap()
    );

    let mut store = ready_store(&linux.prerequisite_id);
    let mut gate = ApprovalGate::default();
    let attempt = AttemptContext::begin_for_test(
        &mut store,
        &linux,
        &same_host,
        "c4d870dd-20a1-44ef-9d3f-73702d5f2222",
        now,
    )
    .unwrap();
    let preview =
        ApprovalPreview::new_for_test(linux.clone(), same_host.clone(), attempt.clone()).unwrap();
    let binding = gate
        .grant_at(preview.request_for_test(), &same_host, now)
        .unwrap();
    let changed = plan(Host::Linux, None);
    assert!(
        gate.consume_at(&binding, &changed, &another_host, &attempt, now + 1)
            .is_err()
    );
    assert!(
        gate.consume_at(&binding, &linux, &same_host, &attempt, now + 1)
            .is_err()
    );
}

#[test]
fn approval_receipt_is_one_use() {
    let plan = plan(Host::Linux, None);
    let identity =
        HostIdentity::from_facts(Host::Linux, "10.2.0", "synthetic-machine", "synthetic-user")
            .unwrap();
    let now = now();
    let mut store = ready_store(&plan.prerequisite_id);
    let attempt = AttemptContext::begin_for_test(
        &mut store,
        &plan,
        &identity,
        "c4d870dd-20a1-44ef-9d3f-73702d5f2222",
        now,
    )
    .unwrap();
    let preview =
        ApprovalPreview::new_for_test(plan.clone(), identity.clone(), attempt.clone()).unwrap();
    let mut gate = ApprovalGate::default();
    let binding = gate
        .grant_at(preview.request_for_test(), &identity, now)
        .unwrap();
    attempt
        .persist_authorization_for_test(&mut store, binding.clone(), now + 1)
        .unwrap();
    gate.consume_at(&binding, &plan, &identity, &attempt, now + 2)
        .unwrap();
    assert!(
        gate.consume_at(&binding, &plan, &identity, &attempt, now + 3)
            .is_err()
    );
}
