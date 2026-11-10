use super::*;

#[test]
fn enabled_personal_policy_is_offline_and_human_scoped() {
    let owner = "00000000-0000-4000-8000-000000000001";
    let profile = "00000000-0000-4000-8000-000000000002";
    let target = super::super::super::super::personal_profiles::personal_session_target(
        owner,
        profile,
        "00000000-0000-4000-8000-000000000003",
    );
    let choice = ComputerUsePersonalVmChoiceV1::Unlock {
        profile_id: profile.into(),
    };
    assert_eq!(
        authorize_personal_start(&choice, &target, owner, false),
        Err("personal_vm_r3_security_gate_disabled")
    );
    authorize_personal_start(&choice, &target, owner, true).unwrap();
    validate_target(&choice, &target, owner).unwrap();
    assert!(validate_target(&choice, &target, "00000000-0000-4000-8000-000000000099").is_err());
    let mut networked_target = target.clone();
    networked_target.network_profile = ComputerNetworkProfileV1::PersonalInternet;
    assert!(validate_target(&choice, &networked_target, owner).is_err());
    let policy = ComputerPolicyV1 {
        schema_version: 1,
        identity: target.identity,
        session_class: ComputerSessionClassV1::PersonalPersistent,
        capability: ComputerCapabilityV1::PersistentPersonal,
        network_profile: ComputerNetworkProfileV1::Disabled,
        allowed_apps: PERSONAL_APPS.iter().map(|app| (*app).to_owned()).collect(),
        authorized_operations: PERSONAL_OPERATIONS.into_iter().collect(),
        approval_generation: 1,
        resource_budget: ComputerResourceBudgetV1 {
            cpu_millis: 2_000,
            memory_bytes: 1_073_741_824,
            disk_bytes: 2_147_483_648,
            time_limit_ms: PERSONAL_SESSION_MAX_MS,
            action_limit: 10_000,
        },
    };
    policy.validate().unwrap();
    let mut networked = policy;
    networked.network_profile = ComputerNetworkProfileV1::PersonalInternet;
    assert!(networked.validate().is_err());
}
