use std::{collections::BTreeSet, path::PathBuf};

use ed25519_dalek::SigningKey;

use crate::computer_use::{
    ComputerBackend, ComputerCapabilityV1, ComputerControllerProofV1, ComputerHostPlatformV1,
    ComputerHypervisorV1, ComputerIdentityV1, ComputerLeaseV1, ComputerLifecycleV1,
    ComputerNetworkProfileV1, ComputerOperationV1, ComputerPolicyV1, ComputerResourceBudgetV1,
    ComputerSessionClassV1, ComputerSessionV1, ComputerTargetV1, guest_image::GuestImageOverlayV1,
};

use super::{
    WindowsBackendBlockerV1 as Blocker, WindowsBackendEvidenceV1 as Evidence,
    WindowsComputerBackend,
};
use crate::computer_use::windows_backend::assess_windows_wpx;

fn ready_windows_host() -> super::super::WindowsWpxReadiness {
    assess_windows_wpx(
        ComputerHostPlatformV1::WindowsX64,
        Some(22_000),
        Some(true),
        Some(true),
    )
}

fn session() -> ComputerSessionV1 {
    let identity = ComputerIdentityV1 {
        project_id: "00000000-0000-4000-8000-000000000001".into(),
        task_id: "00000000-0000-4000-8000-000000000002".into(),
        user_id: "00000000-0000-4000-8000-000000000003".into(),
        session_id: "00000000-0000-4000-8000-000000000004".into(),
    };
    let allowed_apps = BTreeSet::from(["text-editor".to_owned()]);
    let policy = ComputerPolicyV1 {
        schema_version: 1,
        identity: identity.clone(),
        session_class: ComputerSessionClassV1::Disposable,
        capability: ComputerCapabilityV1::ApprovedGuiTask,
        network_profile: ComputerNetworkProfileV1::Disabled,
        allowed_apps: allowed_apps.clone(),
        authorized_operations: BTreeSet::from([ComputerOperationV1::ObserveScreen]),
        approval_generation: 1,
        resource_budget: ComputerResourceBudgetV1 {
            cpu_millis: 2_000,
            memory_bytes: 1024 * 1024 * 1024,
            disk_bytes: 2 * 1024 * 1024 * 1024,
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
            machine_id: "00000000-0000-4000-8000-000000000005".into(),
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
            signer_key_id: "00000000-0000-4000-8000-000000000008".into(),
            signature_hex: "0".repeat(128),
        },
    };
    session
        .attest(
            "00000000-0000-4000-8000-000000000008".into(),
            &SigningKey::from_bytes(&[31; 32]),
        )
        .expect("fixture session is valid");
    session
}

#[test]
fn ready_whpx_does_not_make_the_adapter_probe_supported() {
    let backend = WindowsComputerBackend::with_platform_readiness(
        ComputerHostPlatformV1::WindowsX64,
        ready_windows_host(),
    );
    let probe = backend.probe().expect("probe is diagnostic");

    assert!(probe.hypervisor_accessible);
    assert!(!probe.qemu_available);
    assert!(!probe.delegated_resource_controls);
    assert_eq!(probe.max_overlay_bytes, 0);
    assert!(
        !probe.supports_target(&crate::computer_use::ComputerTargetV1::LocalVm {
            machine_id: "00000000-0000-4000-8000-000000000005".into(),
            hypervisor: crate::computer_use::ComputerHypervisorV1::QemuWhpx,
        })
    );
    assert_eq!(
        backend.unavailable_error(),
        crate::computer_use::ComputerBackendErrorV1::QemuUnavailable
    );
}

#[test]
fn absent_trusted_runtime_and_disk_quota_are_distinct_hard_blockers() {
    let no_runtime = Evidence::default();
    assert_eq!(
        no_runtime.blockers().first(),
        Some(&Blocker::TrustedQemuRuntimeMissing)
    );

    let runtime_without_quota = Evidence {
        trusted_qemu_runtime: true,
        ..Evidence::default()
    };
    assert_eq!(
        runtime_without_quota.blockers().first(),
        Some(&Blocker::OverlayGrowthQuotaMissing)
    );

    let bounded_but_unverified = Evidence {
        trusted_qemu_runtime: true,
        overlay_growth_quota: true,
        guest_transport: true,
        durable_recovery: true,
        display_control: true,
        release_enabled: false,
    };
    assert_eq!(
        bounded_but_unverified.blockers(),
        vec![Blocker::ReleaseGateClosed]
    );
}

#[test]
fn lifecycle_adapter_reports_each_missing_runtime_boundary_and_stays_fail_closed() {
    let backend = WindowsComputerBackend::with_platform_readiness(
        ComputerHostPlatformV1::WindowsX64,
        ready_windows_host(),
    );
    let blockers = backend.blockers();

    assert_eq!(
        blockers,
        vec![
            Blocker::TrustedQemuRuntimeMissing,
            Blocker::OverlayGrowthQuotaMissing,
            Blocker::GuestTransportEvidenceMissing,
            Blocker::DurableRecoveryMissing,
            Blocker::DisplayControlMissing,
            Blocker::ReleaseGateClosed,
        ]
    );
    let diagnostics = backend.diagnostics();
    assert!(
        diagnostics
            .iter()
            .any(|line| line.contains("provenance-verified"))
    );
    assert!(
        diagnostics
            .iter()
            .any(|line| line.contains("storage quota"))
    );
    assert!(
        diagnostics
            .iter()
            .any(|line| line.contains("crash recovery"))
    );
}

#[test]
fn missing_wpx_prerequisites_are_reported_before_runtime_evidence() {
    let backend = WindowsComputerBackend::with_platform_readiness(
        ComputerHostPlatformV1::WindowsX64,
        assess_windows_wpx(ComputerHostPlatformV1::WindowsX64, None, None, None),
    );

    assert_eq!(
        backend.blockers().first(),
        Some(&Blocker::HostPrerequisitesUnavailable)
    );
    assert_eq!(
        backend.unavailable_error(),
        crate::computer_use::ComputerBackendErrorV1::HostNotReady
    );
}

#[test]
fn prepare_and_recovery_do_not_launch_or_claim_state_without_runtime_evidence() {
    let backend = WindowsComputerBackend::with_platform_readiness(
        ComputerHostPlatformV1::WindowsX64,
        ready_windows_host(),
    );
    let session = session();
    let overlay = GuestImageOverlayV1::for_launch_plan_test(
        session.identity.session_id.clone(),
        session.image_digest.clone(),
        session.image_manifest_digest.clone(),
        session.image_signer_key_id.clone(),
        PathBuf::from("C:\\app\\overlays\\session.qcow2"),
        PathBuf::from("C:\\app\\overlays"),
        2 * 1024 * 1024 * 1024,
    );

    assert_eq!(
        backend.prepare(&session, overlay, 2),
        Err(crate::computer_use::ComputerBackendErrorV1::QemuUnavailable)
    );
    assert_eq!(
        backend.recover(),
        Err(crate::computer_use::ComputerBackendErrorV1::QemuUnavailable)
    );
}
