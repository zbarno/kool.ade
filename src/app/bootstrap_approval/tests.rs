use super::*;
use crate::app::bootstrap_repair_policy::{Host, Operation, Proposal};

fn binding() -> binding::MachineBinding {
    binding::MachineBinding::from_facts(
        Host::Linux,
        "synthetic-machine-id",
        "synthetic-principal",
        &"c".repeat(64),
        1_000,
    )
    .unwrap()
}

fn proposal() -> Proposal {
    Proposal {
        schema_version: 1,
        operation: Operation::PrepareLockedImage,
        host: Host::Linux,
        prerequisite_id: format!("setup:images:general:amd64:{}", "e".repeat(64)),
        image_profile: Some("general".into()),
    }
}

fn gate() -> (tempfile::TempDir, ApprovalGate) {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let gate = ApprovalGate::at_path(root.join("machine").join("audit.json"));
    (directory, gate)
}

fn preview() -> ApprovalPreview {
    ApprovalPreview {
        proposal: proposal(),
        binding: binding(),
    }
}

#[test]
fn exact_approval_is_one_use_and_does_not_survive_restart() {
    let (_, mut gate) = gate();
    let proposal = proposal();
    let binding = binding();
    let id = gate.approve_for_test(&proposal, &binding, 1_100).unwrap();
    let approved = gate
        .consume_for_test(id, &proposal, &binding, 1_200)
        .unwrap();
    assert_eq!(approved.operation, Operation::PrepareLockedImage);
    assert!(
        gate.consume_for_test(id, &proposal, &binding, 1_201)
            .is_err()
    );
}

#[test]
fn changed_machine_probe_or_proposal_cannot_replay_consent() {
    let (_, mut gate) = gate();
    let id = gate
        .approve_for_test(&proposal(), &binding(), 1_100)
        .unwrap();
    let mut changed_binding = binding();
    changed_binding.probe_hash = "d".repeat(64);
    assert!(
        gate.consume_for_test(id, &proposal(), &changed_binding, 1_200)
            .is_err()
    );
    assert!(
        gate.consume_for_test(id, &proposal(), &binding(), 1_200)
            .is_err()
    );

    let id = gate
        .approve_for_test(&proposal(), &binding(), 1_300)
        .unwrap();
    let mut changed_proposal = proposal();
    changed_proposal.image_profile = Some("rust".into());
    assert!(
        gate.consume_for_test(id, &changed_proposal, &binding(), 1_400)
            .is_err()
    );
}

#[test]
fn fresh_probe_time_does_not_invalidate_matching_machine_consent() {
    let (_, mut gate) = gate();
    let id = gate
        .approve_for_test(&proposal(), &binding(), 1_100)
        .unwrap();
    let mut fresh_probe = binding();
    fresh_probe.probed_at_unix_ms = 1_150;
    let approved = gate
        .consume_for_test(id, &proposal(), &fresh_probe, 1_200)
        .unwrap();
    assert_eq!(approved.operation, Operation::PrepareLockedImage);
}

#[test]
fn approved_action_rejects_changed_daemon_or_image_facts() {
    let (_, mut gate) = gate();
    let proposal = proposal();
    let original = binding();
    let id = gate.approve_for_test(&proposal, &original, 1_100).unwrap();
    let action = gate
        .consume_for_test(id, &proposal, &original, 1_200)
        .unwrap();
    action
        .validate_binding(&proposal, &original, 1_201)
        .unwrap();

    let mut changed = binding();
    changed.probe_hash = "d".repeat(64);
    assert!(action.validate_binding(&proposal, &changed, 1_201).is_err());
    changed = binding();
    changed.image_digest = format!("sha256:{}", "e".repeat(64));
    assert!(action.validate_binding(&proposal, &changed, 1_201).is_err());
    let mut changed_proposal = proposal;
    changed_proposal.prerequisite_id = format!("setup:images:rust:amd64:{}", "e".repeat(64));
    assert!(
        action
            .validate_binding(&changed_proposal, &original, 1_201)
            .is_err()
    );
}

#[test]
fn stale_and_read_only_proposals_cannot_grant_a_ticket() {
    let (_, mut gate) = gate();
    assert!(
        gate.approve_for_test(&proposal(), &binding(), 1_000 + 5 * 60 * 1000 + 1)
            .is_err()
    );
    let mut read_only = proposal();
    read_only.operation = Operation::RecheckPrerequisites;
    read_only.image_profile = None;
    assert!(
        gate.approve_for_test(&read_only, &binding(), 1_100)
            .is_err()
    );
}

#[test]
fn confirmation_control_does_not_grant_consent_without_a_user_click() {
    let context = egui::Context::default();
    let (_, gate) = gate();
    let preview = preview();
    let mut request = None;
    let mut output = context.run_ui(Default::default(), |ui| {
        request = preview.show_confirmation(ui);
    });
    output.textures_delta.clear();
    assert!(request.is_none());
    assert!(gate.approvals.is_empty());
}

#[test]
fn audit_is_redacted_and_corruption_fails_closed() {
    let (directory, mut gate) = gate();
    let approval = gate
        .approve_for_test(&proposal(), &binding(), 1_100)
        .unwrap();
    let content = std::fs::read_to_string(directory.path().join("machine/audit.json")).unwrap();
    assert!(content.contains("approved"));
    assert!(content.contains("setup:images:general:amd64"));
    assert!(!content.contains("synthetic-machine-id"));
    gate.cancel(approval).unwrap();
    assert!(
        std::fs::read_to_string(directory.path().join("machine/audit.json"))
            .unwrap()
            .contains("cancelled")
    );

    let audit = directory.path().join("machine/audit.json");
    std::fs::write(&audit, "{broken").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&audit, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    assert!(
        gate.approve_for_test(&proposal(), &binding(), 1_200)
            .is_err()
    );
}
