//! Cross-tier release-gate regression: unverified modes stay unavailable
//! together even when their component implementations are present.

use crate::computer_use::{
    ComputerCapabilityV1, ComputerHostPlatformV1, ComputerIdentityV1, ComputerNetworkProfileV1,
    ComputerPolicyV1, ComputerResourceBudgetV1, ComputerSessionClassV1,
};

#[test]
fn unverified_research_personal_transaction_remote_and_host_modes_stay_gated() {
    assert!(!super::super::session::internet_research_available(
        "implementer/v1",
        2
    ));
    assert!(!std::hint::black_box(
        super::super::session::startup::PERSONAL_VM_R3_ENABLED
    ));
    assert!(!crate::computer_use::remote_worker::remote_computer_use_release_gate_open_v1());

    let policy = ComputerPolicyV1 {
        schema_version: 1,
        identity: ComputerIdentityV1 {
            project_id: "00000000-0000-4000-8000-000000000001".into(),
            task_id: "00000000-0000-4000-8000-000000000002".into(),
            user_id: "00000000-0000-4000-8000-000000000003".into(),
            session_id: "00000000-0000-4000-8000-000000000004".into(),
        },
        session_class: ComputerSessionClassV1::Disposable,
        capability: ComputerCapabilityV1::HighImpactExternalAction,
        network_profile: ComputerNetworkProfileV1::Disabled,
        allowed_apps: Default::default(),
        authorized_operations: Default::default(),
        approval_generation: 1,
        resource_budget: ComputerResourceBudgetV1 {
            cpu_millis: 2_000,
            memory_bytes: 1_073_741_824,
            disk_bytes: 2_147_483_648,
            time_limit_ms: 60_000,
            action_limit: 10,
        },
    };
    assert_eq!(
        policy.validate(),
        Err("high-impact actions are unsupported until exact transaction grants exist")
    );

    for platform in [
        ComputerHostPlatformV1::MacOsIntel,
        ComputerHostPlatformV1::MacOsAppleSilicon,
    ] {
        let readiness = crate::computer_use::macos_backend::assess_macos_hvf(platform, Some(true));
        assert!(!readiness.release_enabled);
    }
    let windows = crate::computer_use::windows_backend::assess_windows_wpx(
        ComputerHostPlatformV1::WindowsX64,
        Some(19_041),
        Some(true),
        Some(true),
    );
    assert!(!windows.release_enabled);
}
