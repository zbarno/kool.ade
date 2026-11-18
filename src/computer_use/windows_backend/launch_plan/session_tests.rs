use std::{fs, path::Path};

use ed25519_dalek::SigningKey;

use super::*;
use crate::computer_use::{
    ComputerCapabilityV1, ComputerControllerProofV1, ComputerHypervisorV1, ComputerIdentityV1,
    ComputerLeaseV1, ComputerLifecycleV1, ComputerNetworkProfileV1, ComputerOperationV1,
    ComputerPolicyV1, ComputerResourceBudgetV1, ComputerSessionClassV1, ComputerSessionV1,
    ComputerTargetV1, guest_image::GuestImageOverlayV1,
};

const KEY_ID: &str = "00000000-0000-4000-8000-000000000008";
const HOST_ID: &str = "00000000-0000-4000-8000-000000000005";
const SESSION_ID: &str = "00000000-0000-4000-8000-000000000004";
const DISK_BYTES: u64 = 2 * 1024 * 1024 * 1024;

fn signer() -> SigningKey {
    SigningKey::from_bytes(&[29; 32])
}

fn session(key: &SigningKey) -> ComputerSessionV1 {
    let identity = ComputerIdentityV1 {
        project_id: "00000000-0000-4000-8000-000000000001".into(),
        task_id: "00000000-0000-4000-8000-000000000002".into(),
        user_id: "00000000-0000-4000-8000-000000000003".into(),
        session_id: SESSION_ID.into(),
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
            cpu_millis: 2_000,
            memory_bytes: 1024 * 1024 * 1024,
            disk_bytes: DISK_BYTES,
            time_limit_ms: 60_000,
            action_limit: 20,
        },
    };
    let mut session = ComputerSessionV1 {
        schema_version: 1,
        session_started_at_unix_ms: 1,
        identity,
        session_class: ComputerSessionClassV1::Disposable,
        target: ComputerTargetV1::LocalVm {
            machine_id: HOST_ID.into(),
            hypervisor: ComputerHypervisorV1::QemuWhpx,
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
            signer_key_id: KEY_ID.into(),
            signature_hex: "0".repeat(128),
        },
    };
    session
        .attest(KEY_ID.into(), key)
        .expect("sign fixture session");
    session
}

fn launch_plan(
    session: &ComputerSessionV1,
    overlay: &GuestImageOverlayV1,
    storage: &crate::computer_use::windows_backend::storage::WindowsVmStorage,
    qemu: &Path,
    boot: &Path,
    host_id: &str,
    key: &SigningKey,
) -> Result<WindowsWpxLaunchPlan, WindowsLaunchBlocker> {
    WindowsWpxLaunchPlan::from_authorized_session(
        session,
        overlay,
        storage,
        qemu,
        boot,
        host_id,
        8,
        2,
        KEY_ID,
        &key.verifying_key(),
    )
}

#[test]
fn authorized_session_constructor_binds_controller_target_image_and_limits() {
    let temporary = tempfile::tempdir().expect("temporary Windows app data");
    let storage =
        crate::computer_use::windows_backend::storage::WindowsVmStorage::open_at(temporary.path())
            .expect("private test storage");
    let key = signer();
    let session = session(&key);
    let overlay_path = storage.overlay_root().join("session.qcow2");
    fs::write(&overlay_path, b"fixture overlay").expect("create fixture overlay");
    let overlay = GuestImageOverlayV1::for_launch_plan_test(
        SESSION_ID.into(),
        session.image_digest.clone(),
        session.image_manifest_digest.clone(),
        session.image_signer_key_id.clone(),
        overlay_path,
        storage.overlay_root().to_path_buf(),
        DISK_BYTES,
    );
    let qemu = temporary.path().join("qemu-system-x86_64.exe");
    fs::write(&qemu, b"test executable placeholder").expect("create QEMU path fixture");
    let session_directory = storage
        .session_directory(SESSION_ID)
        .expect("create private session directory");
    let boot_config = session_directory.join("boot.json");
    fs::write(&boot_config, b"{}").expect("create boot config");

    let plan = launch_plan(
        &session,
        &overlay,
        &storage,
        &qemu,
        &boot_config,
        HOST_ID,
        &key,
    )
    .expect("valid signed WHPX session");
    assert_eq!(plan.resource_summary(), (2, 1024 * 1024 * 1024, DISK_BYTES));
    assert!(plan.arguments().iter().any(|arg| arg == "-nic"));
    assert!(!plan.arguments().iter().any(|arg| arg == "-netdev"));
}

#[test]
fn authorized_session_constructor_rejects_bad_attestation_target_and_overlay() {
    let temporary = tempfile::tempdir().expect("temporary Windows app data");
    let storage =
        crate::computer_use::windows_backend::storage::WindowsVmStorage::open_at(temporary.path())
            .expect("private test storage");
    let key = signer();
    let valid = session(&key);
    let overlay = GuestImageOverlayV1::for_launch_plan_test(
        SESSION_ID.into(),
        valid.image_digest.clone(),
        valid.image_manifest_digest.clone(),
        valid.image_signer_key_id.clone(),
        storage.overlay_root().join("session.qcow2"),
        storage.overlay_root().to_path_buf(),
        DISK_BYTES,
    );
    let qemu = temporary.path().join("qemu-system-x86_64.exe");
    fs::write(&qemu, b"test executable placeholder").expect("create QEMU path fixture");
    let session_directory = storage
        .session_directory(SESSION_ID)
        .expect("create private session directory");
    let boot_config = session_directory.join("boot.json");
    fs::write(&boot_config, b"{}").expect("create boot config");

    let mut tampered = valid.clone();
    tampered.policy.resource_budget.memory_bytes += 1024 * 1024;
    assert_eq!(
        launch_plan(
            &tampered,
            &overlay,
            &storage,
            &qemu,
            &boot_config,
            HOST_ID,
            &key
        )
        .unwrap_err(),
        WindowsLaunchBlocker::InvalidSession
    );

    let mut wrong_target = valid.clone();
    wrong_target.target = ComputerTargetV1::LocalVm {
        machine_id: HOST_ID.into(),
        hypervisor: ComputerHypervisorV1::QemuHvf,
    };
    wrong_target.attest(KEY_ID.into(), &key).unwrap();
    assert_eq!(
        launch_plan(
            &wrong_target,
            &overlay,
            &storage,
            &qemu,
            &boot_config,
            HOST_ID,
            &key
        )
        .unwrap_err(),
        WindowsLaunchBlocker::UnsupportedTarget
    );
    assert_eq!(
        launch_plan(
            &valid,
            &overlay,
            &storage,
            &qemu,
            &boot_config,
            "other-host",
            &key,
        )
        .unwrap_err(),
        WindowsLaunchBlocker::UnsupportedTarget
    );

    let wrong_overlay = GuestImageOverlayV1::for_launch_plan_test(
        "00000000-0000-4000-8000-000000000099".into(),
        valid.image_digest.clone(),
        valid.image_manifest_digest.clone(),
        valid.image_signer_key_id.clone(),
        overlay.path().to_path_buf(),
        storage.overlay_root().to_path_buf(),
        DISK_BYTES,
    );
    assert_eq!(
        launch_plan(
            &valid,
            &wrong_overlay,
            &storage,
            &qemu,
            &boot_config,
            HOST_ID,
            &key
        )
        .unwrap_err(),
        WindowsLaunchBlocker::InvalidOverlay
    );
}
