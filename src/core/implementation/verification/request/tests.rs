use super::*;

struct Host;

impl crate::computer_use::mcp::ComputerUseMcpHostV1 for Host {
    fn handle(
        &self,
        _caller: &crate::computer_use::mcp::ComputerUseCallerV1,
        _request: crate::computer_use::mcp::ComputerUseMcpRequestV1,
        _cancelled: &std::sync::atomic::AtomicBool,
    ) -> crate::computer_use::mcp::ComputerUseMcpResponseV1 {
        crate::computer_use::mcp::ComputerUseMcpResponseV1 {
            schema_version: crate::computer_use::mcp::COMPUTER_USE_MCP_VERSION_V1,
            status: crate::computer_use::mcp::ComputerUseMcpStatusV1::Blocked,
            summary: "synthetic unavailable result".into(),
            reason_code: Some("test_unavailable".into()),
            session_id: None,
            capabilities: Vec::new(),
            observation: None,
            action_outcome: None,
        }
    }
}

fn context() -> crate::core::implementation::ComputerUseRunContext {
    crate::core::implementation::ComputerUseRunContext {
        project_id: "00000000-0000-4000-8000-000000000001".into(),
        user_id: "00000000-0000-4000-8000-000000000003".into(),
        session_id: "00000000-0000-4000-8000-000000000004".into(),
        host: std::sync::Arc::new(Host),
    }
}

#[test]
fn implementation_access_is_app_bound_to_task_and_exact_role() {
    let context = context();
    let implementer = access_for_role(
        &context,
        Some("00000000-0000-4000-8000-000000000002"),
        crate::harness::prompt_catalog::PromptRole::Implementer,
    )
    .unwrap();
    assert_eq!(implementer.caller.identity.project_id, context.project_id);
    assert_eq!(
        implementer.caller.identity.task_id,
        "00000000-0000-4000-8000-000000000002"
    );
    assert_eq!(implementer.caller.identity.user_id, context.user_id);
    assert_eq!(implementer.caller.identity.session_id, context.session_id);
    assert_eq!(implementer.caller.role_id, "implementer/v1");

    let repair = access_for_role(
        &context,
        Some("00000000-0000-4000-8000-000000000002"),
        crate::harness::prompt_catalog::PromptRole::RepairWorker,
    )
    .unwrap();
    assert_eq!(repair.caller.role_id, "repair/v1");
}

#[test]
fn non_worker_roles_and_invalid_task_ids_receive_no_computer_use_access() {
    let context = context();
    assert!(
        access_for_role(
            &context,
            Some("00000000-0000-4000-8000-000000000002"),
            crate::harness::prompt_catalog::PromptRole::Planner,
        )
        .is_none()
    );
    assert!(
        access_for_role(
            &context,
            None,
            crate::harness::prompt_catalog::PromptRole::Implementer,
        )
        .is_none()
    );
    assert!(
        access_for_role(
            &context,
            Some("ticket-path-not-a-task-uuid"),
            crate::harness::prompt_catalog::PromptRole::Implementer,
        )
        .is_none()
    );
}
