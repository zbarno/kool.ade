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
            target: crate::computer_use::ComputerTargetV1::LocalKvm {
                machine_id: "00000000-0000-4000-8000-000000000006".into(),
            },
            approved_destination: None,
            allowed_operations: vec![crate::computer_use::ComputerOperationV1::ObserveScreen],
            allowed_apps: vec!["chromium".into()],
            network_profile: ComputerNetworkProfileV1::Disabled,
            available_network_profiles: vec![
                ComputerNetworkProfileV1::Disabled,
                ComputerNetworkProfileV1::InternetResearch,
            ],
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

#[test]
fn task_session_views_are_scoped_to_the_exact_task_and_local_user() {
    let controller = Arc::new(super::ComputerUseMcpControllerV1::for_current_host(
        Arc::new(ComputerUseAuthorityRegistryV1::default()),
    ));
    let base_view = |session_id: &str, task_id: &str, user_id: &str| {
        crate::computer_use::mcp::ComputerUseTaskSessionViewV1 {
            session_id: session_id.into(),
            task_id: task_id.into(),
            user_id: user_id.into(),
            agent_role: "implementer/v1".into(),
            session_class: crate::computer_use::ComputerSessionClassV1::Disposable,
            allowed_apps: vec!["chromium".into()],
            guest_apps: Vec::new(),
            guest_app_catalog_status: "signed_app_catalog_unavailable".into(),
            app_management_available: false,
            resource_budget: crate::computer_use::MINIMUM_DISPOSABLE_VM_BUDGET_V1,
            actions_remaining: 100,
            lifecycle: crate::computer_use::ComputerLifecycleV1::Delegated,
            status: crate::computer_use::mcp::ComputerUseTaskSessionStatusV1::Active,
            queue_position: None,
            control_owner: crate::computer_use::mcp::ComputerUseControlOwnerV1::Agent,
            control_generation: 1,
            active_app: None,
            remaining_ms: 60_000,
            expires_at_unix_ms: super::unix_time_ms() + 60_000,
            frame_revision: 0,
            frame_width: 0,
            frame_height: 0,
            frame_png: None,
            frame_error: false,
            private_input: false,
            control_transition_pending: false,
            control_error: None,
            control_notice: None,
            action_log: Vec::new(),
            pending_action_intent: None,
        }
    };
    controller.view_cache.lock().unwrap().extend([
        (
            "session-a".into(),
            base_view("session-a", "task-a", "user-a"),
        ),
        (
            "session-b".into(),
            base_view("session-b", "task-a", "user-b"),
        ),
        (
            "session-c".into(),
            base_view("session-c", "task-b", "user-a"),
        ),
    ]);
    let visible = controller.task_session_views("task-a", "user-a");
    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0].session_id, "session-a");
}
