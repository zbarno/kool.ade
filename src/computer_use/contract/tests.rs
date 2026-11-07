use super::*;
use std::collections::BTreeSet;

#[test]
fn capability_restriction_only_intersects_and_lowers_authority_and_quotas() {
    let mut broad = test_policy();
    broad.capability = ComputerCapabilityV1::InternetResearch;
    broad.network_profile = ComputerNetworkProfileV1::ApprovedInternet;
    let mut ceiling = broad.clone();
    ceiling.capability = ComputerCapabilityV1::ApprovedGuiTask;
    ceiling.network_profile = ComputerNetworkProfileV1::Disabled;
    ceiling.allowed_apps = BTreeSet::from(["text-editor".to_owned()]);
    ceiling.authorized_operations = BTreeSet::from([ComputerOperationV1::ObserveScreen]);
    ceiling.resource_budget.action_limit = 4;
    ceiling.resource_budget.cpu_millis = 1000;
    let result = broad.restrict_to(&ceiling).unwrap();
    assert_eq!(result.capability, ComputerCapabilityV1::ApprovedGuiTask);
    assert_eq!(result.network_profile, ComputerNetworkProfileV1::Disabled);
    assert_eq!(result.allowed_apps, ceiling.allowed_apps);
    assert_eq!(result.authorized_operations, ceiling.authorized_operations);
    assert_eq!(result.resource_budget.action_limit, 4);
    assert_eq!(result.resource_budget.cpu_millis, 1000);
}

#[test]
fn read_only_capability_cannot_include_input_and_restriction_can_revoke_all_operations() {
    let mut policy = test_policy();
    policy.capability = ComputerCapabilityV1::OfflineDisposableReadOnly;
    assert_eq!(
        policy.validate(),
        Err("read-only capability includes an input operation")
    );
    policy.authorized_operations = BTreeSet::from([ComputerOperationV1::ObserveScreen]);
    assert_eq!(policy.validate(), Ok(()));
    let mut no_authority = policy.clone();
    no_authority.authorized_operations.clear();
    assert!(
        policy
            .restrict_to(&no_authority)
            .unwrap()
            .authorized_operations
            .is_empty()
    );
}

#[test]
fn high_impact_authority_stays_unavailable_until_exact_transaction_grants_exist() {
    let mut policy = test_policy();
    policy.capability = ComputerCapabilityV1::HighImpactExternalAction;
    assert_eq!(
        policy.validate(),
        Err("high-impact actions are unsupported until exact transaction grants exist")
    );
}

#[test]
fn persistent_session_class_fails_closed_without_encrypted_storage_grants() {
    let mut policy = test_policy();
    policy.session_class = ComputerSessionClassV1::PersonalPersistent;
    assert_eq!(
        policy.validate(),
        Err("persistent sessions are unsupported until encrypted storage grants exist")
    );
}

#[test]
fn lifecycle_transitions_reject_spoofed_and_terminal_state_changes() {
    use ComputerLifecycleV1 as S;
    let states = [
        S::Preparing,
        S::Booting,
        S::Connecting,
        S::Ready,
        S::Delegated,
        S::HumanControl,
        S::Suspended,
        S::Interrupted,
        S::Failed,
        S::Recovering,
        S::Destroyed,
    ];
    let allowed = [
        (S::Preparing, S::Booting),
        (S::Preparing, S::Failed),
        (S::Preparing, S::Interrupted),
        (S::Preparing, S::Destroyed),
        (S::Booting, S::Connecting),
        (S::Booting, S::Failed),
        (S::Booting, S::Interrupted),
        (S::Booting, S::Destroyed),
        (S::Connecting, S::Ready),
        (S::Connecting, S::Failed),
        (S::Connecting, S::Interrupted),
        (S::Connecting, S::Destroyed),
        (S::Ready, S::Delegated),
        (S::Ready, S::HumanControl),
        (S::Ready, S::Suspended),
        (S::Ready, S::Interrupted),
        (S::Ready, S::Failed),
        (S::Ready, S::Destroyed),
        (S::Delegated, S::Ready),
        (S::Delegated, S::HumanControl),
        (S::Delegated, S::Suspended),
        (S::Delegated, S::Interrupted),
        (S::Delegated, S::Failed),
        (S::Delegated, S::Destroyed),
        (S::HumanControl, S::Ready),
        (S::HumanControl, S::Suspended),
        (S::HumanControl, S::Interrupted),
        (S::HumanControl, S::Failed),
        (S::HumanControl, S::Destroyed),
        (S::Suspended, S::Ready),
        (S::Suspended, S::Interrupted),
        (S::Suspended, S::Failed),
        (S::Suspended, S::Destroyed),
        (S::Interrupted, S::Recovering),
        (S::Interrupted, S::Destroyed),
        (S::Failed, S::Recovering),
        (S::Failed, S::Destroyed),
        (S::Recovering, S::Booting),
        (S::Recovering, S::Connecting),
        (S::Recovering, S::Ready),
        (S::Recovering, S::Interrupted),
        (S::Recovering, S::Failed),
        (S::Recovering, S::Destroyed),
    ];
    for from in states {
        for to in states {
            assert_eq!(
                can_transition(from, to),
                allowed.contains(&(from, to)),
                "{from:?} -> {to:?}"
            );
        }
    }
}

#[test]
fn resource_budgets_enforce_each_hard_limit() {
    let mut budget = test_policy().resource_budget;
    assert_eq!(budget.validate(), Ok(()));
    budget.memory_bytes = 1;
    assert_eq!(budget.validate(), Err("memory quota is outside bounds"));
    budget = test_policy().resource_budget;
    budget.action_limit = 0;
    assert_eq!(budget.validate(), Err("action quota is outside bounds"));
}

#[test]
fn lease_renewal_must_advance_identity_fence_and_expiry() {
    let previous = ComputerLeaseV1 {
        schema_version: 1,
        owner_id: "00000000-0000-4000-8000-000000000003".into(),
        lease_id: "00000000-0000-4000-8000-000000000005".into(),
        generation: 3,
        issued_at_unix_ms: 1000,
        expires_at_unix_ms: 61000,
        cancellation_generation: 4,
    };
    let mut next = previous.clone();
    next.generation = 4;
    next.issued_at_unix_ms = 50000;
    next.expires_at_unix_ms = 110000;
    assert_eq!(next.validate_renewal_from(&previous), Ok(()));
    next.generation = 3;
    assert_eq!(
        next.validate_renewal_from(&previous),
        Err("lease renewal did not advance monotonically")
    );
}

fn test_policy() -> ComputerPolicyV1 {
    let identity = ComputerIdentityV1 {
        project_id: "00000000-0000-4000-8000-000000000001".into(),
        task_id: "00000000-0000-4000-8000-000000000002".into(),
        user_id: "00000000-0000-4000-8000-000000000003".into(),
        session_id: "00000000-0000-4000-8000-000000000004".into(),
    };
    ComputerPolicyV1 {
        schema_version: 1,
        identity,
        session_class: ComputerSessionClassV1::Disposable,
        capability: ComputerCapabilityV1::ApprovedGuiTask,
        network_profile: ComputerNetworkProfileV1::Disabled,
        allowed_apps: BTreeSet::from(["text-editor".into(), "browser".into()]),
        authorized_operations: BTreeSet::from([
            ComputerOperationV1::ObserveScreen,
            ComputerOperationV1::KeyboardInput,
        ]),
        approval_generation: 1,
        resource_budget: MINIMUM_DISPOSABLE_VM_BUDGET_V1,
    }
}
