use super::*;

fn permission() -> ComputerUsePermissionView {
    ComputerUsePermissionView {
        project_id: "project-001".into(),
        task_id: "task-012".into(),
        user_id: "operator-003".into(),
        session_id: "session-345".into(),
        role_id: "implementer/v1".into(),
        machine_id: "vm-678".into(),
        allowed_operations: vec!["observe_screen".into(), "mouse_input".into()],
        network_profile: crate::computer_use::ComputerNetworkProfileV1::Disabled,
        approved_destination: None,
        resource_budget: crate::computer_use::MINIMUM_DISPOSABLE_VM_BUDGET_V1,
        expires_at_unix_ms: 1_800_000_000_000,
        revoked: false,
        expired: false,
    }
}

fn rendered_text(mut output: egui::FullOutput) -> Vec<String> {
    let text = output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) => Some(text.galley.text().to_owned()),
            _ => None,
        })
        .collect();
    output.textures_delta.clear();
    text
}

#[test]
fn active_permission_view_shows_scope_and_expiry_without_revoking() {
    let context = egui::Context::default();
    let output = context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 600.0),
            )),
            ..Default::default()
        },
        |ui| {
            paint(
                ui,
                &[permission()],
                &[],
                &mut |_| Ok(()),
                &mut |_, _| Ok(()),
            )
        },
    );
    let text = rendered_text(output);
    for expected in [
        "Active Computer Use permissions",
        "Task task-012",
        "Project: project-001",
        "Operator: operator-003",
        "Role: implementer/v1",
        "Session: session-345",
        "Virtual machine: vm-678",
        "Allowed actions: observe_screen, mouse_input",
        "Network access: disabled",
        "Approved destination: none",
        "Resource budget:",
        "Status: Active",
        "Revoke permission",
    ] {
        assert!(
            text.iter().any(|line| line.contains(expected)),
            "missing {expected}"
        );
    }
}

#[test]
fn revoke_requires_confirmation_and_expired_grants_cannot_be_revoked() {
    let context = egui::Context::default();
    context.data_mut(|data| data.insert_temp(egui::Id::new(PENDING_ID), "session-345".to_owned()));
    let calls = std::cell::Cell::new(0);
    let mut expired = permission();
    expired.expired = true;
    let output = context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 600.0),
            )),
            ..Default::default()
        },
        |ui| {
            paint(
                ui,
                &[permission()],
                &[],
                &mut |_| {
                    calls.set(calls.get() + 1);
                    Ok(())
                },
                &mut |_, _| Ok(()),
            );
        },
    );
    let text = rendered_text(output);
    assert!(text.iter().any(|line| line.contains("Confirm revoke")));
    assert_eq!(calls.get(), 0);

    let output = context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 600.0),
            )),
            ..Default::default()
        },
        |ui| {
            paint(
                ui,
                std::slice::from_ref(&expired),
                &[],
                &mut |_| {
                    calls.set(calls.get() + 1);
                    Ok(())
                },
                &mut |_, _| Ok(()),
            )
        },
    );
    let text = rendered_text(output);
    assert!(text.iter().any(|line| line.contains("Status: Expired")));
    assert!(!text.iter().any(|line| line.contains("Confirm revoke")));
    assert_eq!(calls.get(), 0);
}

#[test]
fn pending_consent_shows_identity_and_purpose_without_deciding_automatically() {
    let context = egui::Context::default();
    let request = crate::computer_use::mcp::ComputerUseConsentRequestV1 {
        request_id: "request-900".into(),
        identity: crate::computer_use::ComputerIdentityV1 {
            project_id: "project-100".into(),
            task_id: "task-101".into(),
            user_id: "user-102".into(),
            session_id: "session-103".into(),
        },
        role_id: "implementer/v1".into(),
        purpose: "inspect the synthetic test application".into(),
        machine_id: "machine-104".into(),
        approved_destination: None,
        allowed_operations: vec![crate::computer_use::ComputerOperationV1::ObserveScreen],
        allowed_apps: vec!["test-gui-fixture".into()],
        network_profile: crate::computer_use::ComputerNetworkProfileV1::Disabled,
        resource_budget: crate::computer_use::MINIMUM_DISPOSABLE_VM_BUDGET_V1,
    };
    let decisions = std::cell::RefCell::new(Vec::new());
    let output = context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 600.0),
            )),
            ..Default::default()
        },
        |ui| {
            paint(
                ui,
                &[],
                std::slice::from_ref(&request),
                &mut |_| Ok(()),
                &mut |request_id, approve| {
                    decisions
                        .borrow_mut()
                        .push((request_id.to_owned(), approve));
                    Ok(())
                },
            );
        },
    );
    let text = rendered_text(output);
    for expected in [
        "Pending Computer Use consent",
        "Project ID: project-100",
        "Task ID: task-101",
        "Session ID: session-103",
        "User ID: user-102",
        "Role: implementer/v1",
        "Purpose: inspect the synthetic test application",
        "Target VM: machine-104",
        "Proposed operations: observe screen",
        "Allowed applications: test-gui-fixture",
        "Network access: disabled",
        "Resource budget:",
        "Deny request",
        "Approve request",
    ] {
        assert!(
            text.iter().any(|line| line.contains(expected)),
            "missing {expected}"
        );
    }
    assert!(decisions.borrow().is_empty());
}
