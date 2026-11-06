use std::path::PathBuf;

use crate::computer_use::ComputerBackendErrorV1 as Error;

use super::{MachineOwnerRecordV1, validate_recovery_owner};

fn owner(overlay_path: PathBuf) -> MachineOwnerRecordV1 {
    MachineOwnerRecordV1 {
        schema_version: 1,
        owner_id: "00000000-0000-4000-8000-000000000001".into(),
        project_id: "00000000-0000-4000-8000-000000000002".into(),
        task_id: "00000000-0000-4000-8000-000000000003".into(),
        session_id: "00000000-0000-4000-8000-000000000004".into(),
        machine_id: "00000000-0000-4000-8000-000000000005".into(),
        lease_id: "00000000-0000-4000-8000-000000000006".into(),
        lease_generation: 1,
        cancellation_generation: 1,
        policy_fingerprint: format!("sha256:{}", "a".repeat(64)),
        image_digest: format!("sha256:{}", "b".repeat(64)),
        image_manifest_digest: format!("sha256:{}", "c".repeat(64)),
        image_signer_key_id: "00000000-0000-4000-8000-000000000007".into(),
        catalog_generation: 1,
        overlay_path,
        state: "booting".into(),
        event_sequence: 2,
        guest_boot_id: None,
        guest_transcript_sha256: None,
        guest_ready_sequence: None,
        pid: Some(1234),
        process_start_ticks: Some(99),
        exit_code: None,
    }
}

/// Validates the restart ownership boundary before /proc scanning or signaling.
/// This directly exercises the owner-record validator and needs only temp dirs.
#[test]
fn recovery_requires_session_bound_overlay_and_machine_identity() {
    let temp = tempfile::tempdir().unwrap();
    let directory = temp
        .path()
        .join("sessions/00000000-0000-4000-8000-000000000004");
    std::fs::create_dir_all(&directory).unwrap();
    let session_id = "00000000-0000-4000-8000-000000000004";
    let overlay_root = temp.path().join("overlays");
    let overlay = overlay_root.join(format!(
        "{session_id}-00000000-0000-4000-8000-000000000008.qcow2"
    ));
    assert!(
        validate_recovery_owner(
            &owner(overlay.clone()),
            &directory,
            &overlay_root,
            "00000000-0000-4000-8000-000000000005",
        )
        .is_ok()
    );
    assert_eq!(
        validate_recovery_owner(
            &owner(overlay),
            &directory,
            &overlay_root,
            "00000000-0000-4000-8000-00000000000a",
        ),
        Err(Error::OwnershipMismatch)
    );

    let other_session_overlay = overlay_root
        .join("00000000-0000-4000-8000-000000000009-00000000-0000-4000-8000-000000000008.qcow2");
    assert_eq!(
        validate_recovery_owner(
            &owner(other_session_overlay),
            &directory,
            &overlay_root,
            "00000000-0000-4000-8000-000000000005",
        ),
        Err(Error::OwnershipMismatch)
    );
}
