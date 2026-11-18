use std::path::Path;

use ed25519_dalek::VerifyingKey;

use super::qemu_plan::{
    MAX_VCPUS, MacOsQemuLaunchPlanV1, MacOsQemuLaunchRequestV1, build_macos_qemu_launch_plan,
    safe_qemu_path,
};
use crate::computer_use::{
    ComputerHostPlatformV1, ComputerHypervisorV1, ComputerNetworkProfileV1, ComputerSessionClassV1,
    ComputerSessionV1, ComputerTargetV1, guest_image::GuestImageOverlayV1,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MacOsQemuLaunchBlockerV1 {
    InvalidSession,
    UnsupportedSessionClass,
    UnsupportedTarget,
    NetworkEnabled,
    InvalidOverlay,
    UnsafePath,
    InvalidResourceBudget,
}

pub(crate) struct MacOsQemuLaunchEnvironmentV1<'a> {
    pub(crate) platform: ComputerHostPlatformV1,
    pub(crate) session_directory: &'a Path,
    pub(crate) host_machine_id: &'a str,
    pub(crate) now_unix_ms: u64,
    pub(crate) controller_key_id: &'a str,
    pub(crate) controller_key: &'a VerifyingKey,
}

impl MacOsQemuLaunchPlanV1 {
    /// Bind a macOS launch plan to the controller-attested session and the
    /// image overlay that was verified for that session. The caller must
    /// revalidate the overlay immediately before boot and resolve QEMU from
    /// the app-trusted runtime package.
    pub(crate) fn from_attested_session(
        session: &ComputerSessionV1,
        overlay: &GuestImageOverlayV1,
        environment: MacOsQemuLaunchEnvironmentV1<'_>,
    ) -> Result<Self, MacOsQemuLaunchBlockerV1> {
        let MacOsQemuLaunchEnvironmentV1 {
            platform,
            session_directory,
            host_machine_id,
            now_unix_ms,
            controller_key_id,
            controller_key,
        } = environment;
        session
            .validate_for_host(now_unix_ms, controller_key_id, controller_key)
            .map_err(|_| MacOsQemuLaunchBlockerV1::InvalidSession)?;
        if session.session_class != ComputerSessionClassV1::Disposable {
            return Err(MacOsQemuLaunchBlockerV1::UnsupportedSessionClass);
        }
        if !matches!(
            &session.target,
            ComputerTargetV1::LocalVm {
                machine_id,
                hypervisor: ComputerHypervisorV1::QemuHvf,
            } if machine_id == host_machine_id
        ) {
            return Err(MacOsQemuLaunchBlockerV1::UnsupportedTarget);
        }
        if session.network_profile != ComputerNetworkProfileV1::Disabled
            || session.policy.network_profile != ComputerNetworkProfileV1::Disabled
        {
            return Err(MacOsQemuLaunchBlockerV1::NetworkEnabled);
        }
        if overlay.session_id() != session.identity.session_id
            || overlay.image_digest() != session.image_digest
            || overlay.manifest_digest() != session.image_manifest_digest
            || overlay.signer_key_id() != session.image_signer_key_id
            || overlay.path().parent() != Some(overlay.directory())
        {
            return Err(MacOsQemuLaunchBlockerV1::InvalidOverlay);
        }
        if session_directory.file_name().and_then(|name| name.to_str())
            != Some(session.identity.session_id.as_str())
            || !safe_qemu_path(session_directory)
            || overlay.directory() == session_directory
        {
            return Err(MacOsQemuLaunchBlockerV1::UnsafePath);
        }
        let budget = &session.policy.resource_budget;
        let vcpus = budget.cpu_millis.div_ceil(1_000).clamp(1, MAX_VCPUS);
        build_macos_qemu_launch_plan(
            platform,
            MacOsQemuLaunchRequestV1 {
                vcpus,
                memory_bytes: budget.memory_bytes,
                disk_bytes: budget.disk_bytes,
                overlay_virtual_size_bytes: overlay.virtual_size_bytes(),
                overlay: overlay.path(),
                control_socket: &session_directory.join("guest-control.sock"),
                display_socket: &session_directory.join("display.sock"),
                boot_config: &session_directory.join("guest-boot-config.json"),
                app_helper_config: &session_directory.join("guest-app-helper-config.json"),
            },
        )
        .ok_or(MacOsQemuLaunchBlockerV1::InvalidResourceBudget)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use ed25519_dalek::SigningKey;

    use super::*;
    use crate::computer_use::{
        ComputerCapabilityV1, ComputerControllerProofV1, ComputerIdentityV1, ComputerLeaseV1,
        ComputerLifecycleV1, ComputerOperationV1, ComputerPolicyV1, ComputerResourceBudgetV1,
    };

    const KEY_ID: &str = "00000000-0000-4000-8000-000000000008";
    const HOST_ID: &str = "00000000-0000-4000-8000-000000000005";
    const SESSION_ID: &str = "00000000-0000-4000-8000-000000000004";
    const DISK_BYTES: u64 = 4 * 1024 * 1024 * 1024;

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
                cpu_millis: 1_500,
                memory_bytes: 2 * 1024 * 1024 * 1024,
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
                hypervisor: ComputerHypervisorV1::QemuHvf,
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
        session.attest(KEY_ID.into(), key).unwrap();
        session
    }

    fn overlay(session: &ComputerSessionV1, virtual_size_bytes: u64) -> GuestImageOverlayV1 {
        GuestImageOverlayV1::for_launch_plan_test(
            session.identity.session_id.clone(),
            session.image_digest.clone(),
            session.image_manifest_digest.clone(),
            session.image_signer_key_id.clone(),
            PathBuf::from("/private/app/overlays/session.qcow2"),
            PathBuf::from("/private/app/overlays"),
            virtual_size_bytes,
        )
    }

    fn signer() -> SigningKey {
        SigningKey::from_bytes(&[19; 32])
    }

    fn build(
        session: &ComputerSessionV1,
        overlay: &GuestImageOverlayV1,
        key: &SigningKey,
        host_machine_id: &str,
    ) -> Result<MacOsQemuLaunchPlanV1, MacOsQemuLaunchBlockerV1> {
        let session_directory = Path::new("/private/app/sessions").join(SESSION_ID);
        MacOsQemuLaunchPlanV1::from_attested_session(
            session,
            overlay,
            MacOsQemuLaunchEnvironmentV1 {
                platform: ComputerHostPlatformV1::MacOsIntel,
                session_directory: &session_directory,
                host_machine_id,
                now_unix_ms: 2,
                controller_key_id: KEY_ID,
                controller_key: &key.verifying_key(),
            },
        )
    }

    #[test]
    fn session_constructor_binds_signature_target_policy_overlay_and_limits() {
        let key = signer();
        let session = session(&key);
        let overlay = overlay(&session, DISK_BYTES);
        let plan = build(&session, &overlay, &key, HOST_ID).unwrap();
        assert_eq!(
            plan.memory_bytes,
            session.policy.resource_budget.memory_bytes
        );
        assert_eq!(plan.disk_bytes, session.policy.resource_budget.disk_bytes);
        assert!(plan.arguments.iter().any(|arg| arg == "-nic"));
        assert!(!plan.release_enabled);
    }

    #[test]
    fn session_constructor_rejects_invalid_attestation_target_network_overlay_and_host() {
        let key = signer();
        let valid = session(&key);
        let valid_overlay = overlay(&valid, DISK_BYTES);

        let mut tampered = valid.clone();
        tampered.policy.resource_budget.memory_bytes += 1024 * 1024;
        assert_eq!(
            build(&tampered, &valid_overlay, &key, HOST_ID),
            Err(MacOsQemuLaunchBlockerV1::InvalidSession)
        );

        let mut wrong_target = session(&key);
        wrong_target.target = ComputerTargetV1::LocalKvm {
            machine_id: HOST_ID.into(),
        };
        wrong_target.attest(KEY_ID.into(), &key).unwrap();
        assert_eq!(
            build(&wrong_target, &valid_overlay, &key, HOST_ID),
            Err(MacOsQemuLaunchBlockerV1::UnsupportedTarget)
        );

        let mut wrong_host = session(&key);
        wrong_host.attest(KEY_ID.into(), &key).unwrap();
        assert_eq!(
            build(
                &wrong_host,
                &valid_overlay,
                &key,
                "00000000-0000-4000-8000-000000000009"
            ),
            Err(MacOsQemuLaunchBlockerV1::UnsupportedTarget)
        );

        let mut networked = session(&key);
        networked.network_profile = ComputerNetworkProfileV1::InternetResearch;
        networked.policy.network_profile = ComputerNetworkProfileV1::InternetResearch;
        networked.policy.capability = crate::computer_use::ComputerCapabilityV1::InternetResearch;
        networked.attest(KEY_ID.into(), &key).unwrap();
        let networked_overlay = overlay(&networked, DISK_BYTES);
        assert_eq!(
            build(&networked, &networked_overlay, &key, HOST_ID),
            Err(MacOsQemuLaunchBlockerV1::NetworkEnabled)
        );

        let wrong_image = GuestImageOverlayV1::for_launch_plan_test(
            SESSION_ID.into(),
            "sha256:unexpected".into(),
            valid.image_manifest_digest.clone(),
            valid.image_signer_key_id.clone(),
            PathBuf::from("/private/app/overlays/session.qcow2"),
            PathBuf::from("/private/app/overlays"),
            DISK_BYTES,
        );
        assert_eq!(
            build(&valid, &wrong_image, &key, HOST_ID),
            Err(MacOsQemuLaunchBlockerV1::InvalidOverlay)
        );

        let oversized_overlay = overlay(&valid, DISK_BYTES + 1);
        assert_eq!(
            build(&valid, &oversized_overlay, &key, HOST_ID),
            Err(MacOsQemuLaunchBlockerV1::InvalidResourceBudget)
        );
    }
}
