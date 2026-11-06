use std::path::Path;

use super::commands::command_args;
use crate::computer_use::{
    ComputerBackendErrorV1, ComputerCapabilityV1, ComputerControllerProofV1, ComputerIdentityV1,
    ComputerLeaseV1, ComputerLifecycleV1, ComputerNetworkProfileV1, ComputerOperationV1,
    ComputerPolicyV1, ComputerResourceBudgetV1, ComputerSessionClassV1, ComputerSessionV1,
    ComputerTargetV1,
};

fn session() -> ComputerSessionV1 {
    let identity = ComputerIdentityV1 {
        project_id: "00000000-0000-4000-8000-000000000001".into(),
        task_id: "00000000-0000-4000-8000-000000000002".into(),
        user_id: "00000000-0000-4000-8000-000000000003".into(),
        session_id: "00000000-0000-4000-8000-000000000004".into(),
    };
    let allowed_apps = std::collections::BTreeSet::from(["text-editor".to_owned()]);
    let policy = ComputerPolicyV1 {
        schema_version: 1,
        identity: identity.clone(),
        session_class: ComputerSessionClassV1::Disposable,
        capability: ComputerCapabilityV1::ApprovedGuiTask,
        network_profile: ComputerNetworkProfileV1::Disabled,
        allowed_apps: allowed_apps.clone(),
        authorized_operations: std::collections::BTreeSet::from([
            ComputerOperationV1::ObserveScreen,
        ]),
        approval_generation: 1,
        resource_budget: ComputerResourceBudgetV1 {
            cpu_millis: 1_500,
            memory_bytes: 1024 * 1024 * 1024,
            disk_bytes: 4 * 1024 * 1024 * 1024,
            time_limit_ms: 60_000,
            action_limit: 20,
        },
    };
    ComputerSessionV1 {
        schema_version: 1,
        identity,
        session_class: ComputerSessionClassV1::Disposable,
        target: ComputerTargetV1::LocalKvm {
            machine_id: "00000000-0000-4000-8000-000000000005".into(),
        },
        image_digest: format!("sha256:{}", "a".repeat(64)),
        image_manifest_digest: format!("sha256:{}", "b".repeat(64)),
        image_signer_key_id: "00000000-0000-4000-8000-000000000006".into(),
        network_profile: ComputerNetworkProfileV1::Disabled,
        allowed_apps,
        policy,
        lease: ComputerLeaseV1 {
            schema_version: 1,
            owner_id: "00000000-0000-4000-8000-000000000003".into(),
            lease_id: "00000000-0000-4000-8000-000000000007".into(),
            generation: 1,
            issued_at_unix_ms: 1,
            expires_at_unix_ms: 60_001,
            cancellation_generation: 1,
        },
        lifecycle: ComputerLifecycleV1::Preparing,
        action_sequence: 0,
        approval_generation: 1,
        cancellation_generation: 1,
        evidence_refs: vec![],
        controller_proof: ComputerControllerProofV1 {
            signer_key_id: "00000000-0000-4000-8000-000000000008".into(),
            signature_hex: "0".repeat(128),
        },
    }
}

#[test]
fn launch_arguments_have_no_network_monitor_host_mount_or_guest_shell() {
    let session = session();
    let args = command_args(
        &session,
        Path::new("/private/overlay.qcow2"),
        Path::new("/private/guest-control.sock"),
        Path::new("/private/display.sock"),
    )
    .unwrap();
    let args = args
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    assert!(args.windows(2).any(|pair| pair == ["-nic", "none"]));
    assert!(args.windows(2).any(|pair| pair == ["-monitor", "none"]));
    assert!(args.windows(2).any(|pair| pair == ["-serial", "none"]));
    assert!(args.iter().any(|arg| arg.contains("spawn=deny")));
    assert!(
        args.iter()
            .any(|arg| arg.contains("elevateprivileges=deny"))
    );
    assert!(args.iter().any(|arg| arg.contains("resourcecontrol=deny")));
    assert!(!args.iter().any(|arg| arg.contains("hostfwd")));
    assert!(!args.iter().any(|arg| arg.contains("virtio-9p")));
    assert!(!args.iter().any(|arg| arg.contains("-kernel")));
    assert!(!args.iter().any(|arg| arg.contains("-append")));
    assert!(args.iter().any(|arg| arg.contains("clipboard=off")));
    assert!(args.iter().any(|arg| arg.contains("koolade:")));
}

#[test]
fn launch_argument_paths_reject_qemu_option_delimiters() {
    let session = session();
    let result = command_args(
        &session,
        Path::new("/private/overlay.qcow2,readonly=off"),
        Path::new("/private/guest-control.sock"),
        Path::new("/private/display.sock"),
    );
    assert_eq!(result, Err(ComputerBackendErrorV1::InvalidOverlay));
}

#[test]
fn command_configuration_clears_inherited_credentials() {
    use super::commands::configure_command;
    use std::process::Command;
    let mut command = Command::new("/usr/bin/qemu-system-x86_64");
    configure_command(&mut command, &[], Path::new("/private/session"));
    assert!(command.get_envs().any(|(key, value)| {
        key == "HOME" && value == Some(std::ffi::OsStr::new("/nonexistent"))
    }));
    assert!(
        !command
            .get_envs()
            .any(|(key, _)| key == "AWS_SECRET_ACCESS_KEY")
    );
}
