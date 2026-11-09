use std::sync::{Arc, atomic::AtomicBool};

use crate::computer_use::{
    ComputerActionActorV1, ComputerIdentityV1, ComputerNetworkProfileV1, ComputerResourceBudgetV1,
    guest_protocol::ComputerUseAuthorityRegistryV1,
    mcp::{
        ComputerUseCallerV1, ComputerUseConsentGrantV1, ComputerUseConsentRequestV1,
        ComputerUseMcpOperationsV1, ComputerUseMcpStatusV1,
    },
};

fn caller() -> ComputerUseCallerV1 {
    ComputerUseCallerV1 {
        identity: ComputerIdentityV1 {
            project_id: "00000000-0000-4000-8000-000000000004".into(),
            user_id: "00000000-0000-4000-8000-000000000001".into(),
            session_id: "00000000-0000-4000-8000-000000000002".into(),
            task_id: "00000000-0000-4000-8000-000000000003".into(),
        },
        actor: ComputerActionActorV1::Agent,
        role_id: "implementer/v1".into(),
    }
}

fn grant(caller: &ComputerUseCallerV1) -> ComputerUseConsentGrantV1 {
    ComputerUseConsentGrantV1 {
        request: ComputerUseConsentRequestV1 {
            request_id: "00000000-0000-4000-8000-000000000005".into(),
            identity: caller.identity.clone(),
            role_id: caller.role_id.clone(),
            purpose: "inspect UI".into(),
            machine_id: "00000000-0000-4000-8000-000000000006".into(),
            approved_destination: None,
            allowed_operations: vec![crate::computer_use::ComputerOperationV1::ObserveScreen],
            allowed_apps: vec!["chromium".into()],
            network_profile: ComputerNetworkProfileV1::Disabled,
            resource_budget: crate::computer_use::MINIMUM_DISPOSABLE_VM_BUDGET_V1,
        },
        approved_at_unix_ms: 10,
        expires_at_unix_ms: 20,
    }
}

#[test]
fn unavailable_controller_blocks_capabilities_and_scope() {
    let controller = super::ComputerUseMcpControllerV1::for_current_host(Arc::new(
        ComputerUseAuthorityRegistryV1::default(),
    ));
    let caller = caller();
    let response = controller.capabilities(&caller);
    assert_eq!(response.status, ComputerUseMcpStatusV1::Blocked);
    assert_eq!(
        response.reason_code.as_deref(),
        Some("trusted_host_configuration_unavailable")
    );
    assert!(response.capabilities.is_empty());
    assert_eq!(
        controller
            .consent_scope(&caller)
            .unwrap_err()
            .reason_code
            .as_deref(),
        Some("trusted_host_configuration_unavailable")
    );
}

#[test]
fn session_start_is_blocked_without_runtime_and_cancellation_is_denied() {
    let controller = super::ComputerUseMcpControllerV1::for_current_host(Arc::new(
        ComputerUseAuthorityRegistryV1::default(),
    ));
    let caller = caller();
    let response = controller.request_session(&caller, &grant(&caller), &AtomicBool::new(false));
    assert_eq!(response.status, ComputerUseMcpStatusV1::Blocked);
    assert_eq!(
        response.reason_code.as_deref(),
        Some("trusted_host_configuration_unavailable")
    );
    let response = controller.request_session(&caller, &grant(&caller), &AtomicBool::new(true));
    assert_eq!(response.status, ComputerUseMcpStatusV1::Denied);
    assert_eq!(response.reason_code.as_deref(), Some("cancelled"));
}

#[test]
fn policy_budget_never_exceeds_fixed_runtime_ceiling() {
    let requested = ComputerResourceBudgetV1 {
        cpu_millis: 4_000,
        memory_bytes: 2_147_483_648,
        disk_bytes: 4_294_967_296,
        time_limit_ms: 1_200_000,
        action_limit: 200,
    };
    let bounded = super::session::bounded_budget(&requested);
    let ceiling = crate::computer_use::MINIMUM_DISPOSABLE_VM_BUDGET_V1;
    assert_eq!(bounded.cpu_millis, ceiling.cpu_millis);
    assert_eq!(bounded.memory_bytes, ceiling.memory_bytes);
    assert_eq!(bounded.disk_bytes, ceiling.disk_bytes);
    assert_eq!(bounded.time_limit_ms, ceiling.time_limit_ms);
    assert_eq!(bounded.action_limit, ceiling.action_limit);
}

#[test]
fn action_payloads_map_to_authorized_operation_categories() {
    use crate::computer_use::{ComputerActionPayloadV1 as P, ComputerOperationV1 as O};
    assert_eq!(
        super::dispatch::operation(&P::ObserveScreen),
        O::ObserveScreen
    );
    assert_eq!(
        super::dispatch::operation(&P::ObserveWindows),
        O::ObserveWindows
    );
    assert_eq!(super::dispatch::operation(&P::QueryState), O::QueryState);
    assert_eq!(
        super::dispatch::operation(&P::LaunchApp {
            app_id: "chromium".into()
        }),
        O::LaunchAllowedApp
    );
}

#[test]
fn revoke_requires_the_app_bound_session_identity() {
    let controller = super::ComputerUseMcpControllerV1::for_current_host(Arc::new(
        ComputerUseAuthorityRegistryV1::default(),
    ));
    let caller = caller();
    let wrong_session = "00000000-0000-4000-8000-000000000007";
    let response = controller.revoke_session(&caller, wrong_session);
    assert_eq!(response.status, ComputerUseMcpStatusV1::Denied);
    assert_eq!(
        response.reason_code.as_deref(),
        Some("authorization_failed")
    );
}
