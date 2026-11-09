use super::*;

#[test]
fn exposes_one_fixed_eight_tool_schema_without_authority_arguments() {
    let tools = list();
    assert_eq!(tools.len(), 8);
    for tool in tools {
        assert_eq!(tool["inputSchema"]["additionalProperties"], false);
        let properties = tool["inputSchema"]["properties"].as_object().unwrap();
        for forbidden in [
            "task_id",
            "user_id",
            "role_id",
            "actor",
            "vm_id",
            "lease_generation",
            "policy",
        ] {
            assert!(
                !properties.contains_key(forbidden),
                "{} exposes {forbidden}",
                tool["name"]
            );
        }
    }
}

#[test]
fn parser_rejects_authority_injected_as_an_extra_provider_argument() {
    let error = parse(
        "koolade_computer_use_action",
        &json!({
            "session_id":"00000000-0000-4000-8000-000000000001",
            "payload":{"kind":"query_state"}, "task_id":"other-task"
        }),
    )
    .unwrap_err();
    assert!(error.contains("Unexpected"));
}

#[cfg(unix)]
#[test]
fn application_socket_dispatch_keeps_identity_app_owned_and_reports_blocked_attention() {
    // Production boundary: MCP tool call -> private app socket -> host dispatcher.
    // A parser unit test cannot prove the bridge carries the app-selected caller
    // identity across IPC or reports a blocked capability to task activity.
    // External dependencies: Unix sockets only; setup is one temporary directory.
    use crate::{
        computer_use::{ComputerActionActorV1, ComputerIdentityV1},
        harness::{ComputerUseMcpAccessV1, LiveProgress},
    };
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc,
    };

    struct Host(
        Mutex<Option<crate::computer_use::mcp::ComputerUseCallerV1>>,
        AtomicUsize,
    );
    impl crate::computer_use::mcp::ComputerUseMcpHostV1 for Host {
        fn handle(
            &self,
            caller: &crate::computer_use::mcp::ComputerUseCallerV1,
            _: Request,
            _: &AtomicBool,
        ) -> crate::computer_use::mcp::ComputerUseMcpResponseV1 {
            *self.0.lock().unwrap() = Some(caller.clone());
            self.1.fetch_add(1, Ordering::SeqCst);
            crate::computer_use::mcp::ComputerUseMcpResponseV1 {
                schema_version: crate::computer_use::mcp::COMPUTER_USE_MCP_VERSION_V1,
                status: crate::computer_use::mcp::ComputerUseMcpStatusV1::Blocked,
                summary: "Approval is required.".into(),
                reason_code: Some("approval_required".into()),
                session_id: None,
                capabilities: Vec::new(),
                observation: None,
                action_outcome: None,
            }
        }
    }

    let directory = tempfile::tempdir().unwrap();
    let caller = crate::computer_use::mcp::ComputerUseCallerV1 {
        identity: ComputerIdentityV1 {
            project_id: "00000000-0000-4000-8000-000000000001".into(),
            task_id: "00000000-0000-4000-8000-000000000002".into(),
            user_id: "00000000-0000-4000-8000-000000000003".into(),
            session_id: "00000000-0000-4000-8000-000000000004".into(),
        },
        actor: ComputerActionActorV1::Agent,
        role_id: "implementer/v1".into(),
    };
    let host = Arc::new(Host(Mutex::new(None), AtomicUsize::new(0)));
    let access = Arc::new(ComputerUseMcpAccessV1::new(caller.clone(), host.clone()).unwrap());
    let (progress_tx, progress_rx) = mpsc::channel::<LiveProgress>();
    let cancelled = Arc::new(AtomicBool::new(false));
    let bridge = crate::harness::execution_security::computer_use_bridge::ComputerUseBridge::start(
        directory.path(),
        access,
        cancelled.clone(),
        progress_tx,
    )
    .unwrap();
    let binding = crate::harness::execution_security::mcp_server::ComputerUseBridgeConfigV1 {
        socket_path: bridge.binding.socket_path.clone(),
        bridge_session_id: bridge.binding.bridge_session_id.clone(),
        capability_id: bridge.binding.capability_id.clone(),
        task_id: bridge.binding.task_id.clone(),
    };
    let config = crate::harness::execution_security::mcp_server::ServerConfig::computer_use_only(
        caller.identity.task_id.clone(),
        binding,
    );
    let response = super::call(
        &directory.path().join("server.json"),
        &config,
        "koolade_computer_use_capabilities",
        &json!({}),
    );
    assert_eq!(response.response["isError"], false);
    assert_eq!(host.0.lock().unwrap().as_ref(), Some(&caller));
    assert_eq!(host.1.load(Ordering::SeqCst), 1);
    assert!(
        progress_rx
            .try_recv()
            .unwrap()
            .activity
            .unwrap()
            .contains("blocked")
    );
    cancelled.store(true, Ordering::Release);
    let denied = crate::harness::execution_security::computer_use_bridge::call(
        &directory.path().join("server.json"),
        &bridge.binding,
        Request::Capabilities,
    )
    .unwrap();
    assert_eq!(
        denied.status,
        crate::computer_use::mcp::ComputerUseMcpStatusV1::Denied
    );
    assert_eq!(denied.reason_code.as_deref(), Some("cancelled"));
    assert_eq!(
        host.1.load(Ordering::SeqCst),
        1,
        "cancelled call must not reach the host"
    );
}
