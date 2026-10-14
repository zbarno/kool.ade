use super::*;

#[test]
fn application_session_rejects_tool_requests_with_any_mismatched_binding() {
    let binding = SessionBinding {
        session_id: "11111111-1111-4111-8111-111111111111".into(),
        capability_id: "22222222-2222-4222-8222-222222222222".into(),
        task_id: "33333333-3333-4333-8333-333333333333".into(),
        workspace_id: format!(
            "{}:{}:{}",
            "44444444-4444-4444-8444-444444444444",
            "33333333-3333-4333-8333-333333333333",
            "a".repeat(40)
        ),
        access: "implementation".into(),
        target_id: "local".into(),
        target_arch: Some("x86_64".into()),
    };
    let parts = || {
        (
            binding.session_id.as_str(),
            binding.capability_id.as_str(),
            binding.task_id.as_str(),
            binding.workspace_id.as_str(),
            binding.access.as_str(),
        )
    };
    assert!(
        verify_binding(
            parts().0,
            parts().1,
            parts().2,
            parts().3,
            parts().4,
            &binding
        )
        .is_ok()
    );

    for field in ["session", "capability", "task", "workspace", "access"] {
        let (session, capability, task, workspace, access) = parts();
        let (session, capability, task, workspace, access) = match field {
            "session" => (
                "55555555-5555-4555-8555-555555555555",
                capability,
                task,
                workspace,
                access,
            ),
            "capability" => (
                session,
                "55555555-5555-4555-8555-555555555555",
                task,
                workspace,
                access,
            ),
            "task" => (
                session,
                capability,
                "55555555-5555-4555-8555-555555555555",
                workspace,
                access,
            ),
            "workspace" => (session, capability, task, "mismatched-workspace", access),
            "access" => (session, capability, task, workspace, "read_only"),
            _ => unreachable!(),
        };
        assert!(
            verify_binding(session, capability, task, workspace, access, &binding).is_err(),
            "mismatched {field} must fail closed"
        );
    }
}
