pub(super) use std::collections::BTreeSet;

pub(super) use ed25519_dalek::{Signer, SigningKey};

pub(super) use crate::computer_use::{ComputerIdentityV1, ComputerSessionClassV1};

pub(super) use crate::computer_use::apps::{
    GuestAppArchitectureV1, GuestAppCapabilityV1 as Capability, GuestAppInstallRecipeV1,
    GuestAppManifestV1, GuestAppPackageSourceV1, GuestAppProvenanceV1, GuestAppRemovalV1,
    GuestAppRollbackV1, GuestApplicationCatalogTrustV1, GuestApplicationCatalogV1,
    VerifiedGuestApplicationCatalogV1,
};
pub(super) use crate::computer_use::apps::{
    GuestAppConsentGrantV1 as Approval, GuestAppOperationKindV1 as Kind,
    GuestAppOperationRequestV1 as Request, GuestAppProvisionerV1,
};

pub(super) const NOW: u64 = 10_000;
pub(super) const IMAGE: &str =
    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
pub(super) const APP_PACKAGE: &str =
    "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

pub(super) fn identity() -> ComputerIdentityV1 {
    ComputerIdentityV1 {
        project_id: uuid::Uuid::new_v4().to_string(),
        task_id: uuid::Uuid::new_v4().to_string(),
        user_id: uuid::Uuid::new_v4().to_string(),
        session_id: uuid::Uuid::new_v4().to_string(),
    }
}

pub(super) fn signed_catalog() -> VerifiedGuestApplicationCatalogV1 {
    let signer = SigningKey::from_bytes(&[7; 32]);
    let key_id = "00000000-0000-4000-8000-000000000001".to_owned();
    let mut catalog = GuestApplicationCatalogV1 {
        schema_version: 1,
        catalog_id: "00000000-0000-4000-8000-000000000002".into(),
        generation: 1,
        image_digest: IMAGE.into(),
        architecture: GuestAppArchitectureV1::X86_64,
        signer_key_id: key_id.clone(),
        apps: vec![GuestAppManifestV1 {
            schema_version: 1,
            app_id: "text-editor".into(),
            display_name: "Text Editor".into(),
            package: GuestAppPackageSourceV1 {
                source_id: "debian-bookworm-snapshot".into(),
                package_id: "mousepad".into(),
                publisher: "Debian Project".into(),
                provenance: GuestAppProvenanceV1::SignedDistributionRepository,
                version: "0.5.10-1".into(),
                package_sha256: APP_PACKAGE.into(),
                rollback_version: None,
                rollback_package_sha256: None,
            },
            supported_image_digests: BTreeSet::from([IMAGE.into()]),
            supported_architectures: BTreeSet::from([GuestAppArchitectureV1::X86_64]),
            executable_id: "text-editor".into(),
            required_capabilities: BTreeSet::from([Capability::GraphicalDisplay]),
            install_recipe: GuestAppInstallRecipeV1::VerifiedDebPackageV1,
            removal: GuestAppRemovalV1::RemoveExactPackageV1,
            rollback: GuestAppRollbackV1::RestoreSessionOverlayV1,
            integrity_evidence_sha256:
                "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc".into(),
        }],
        signature_hex: String::new(),
    };
    let mut file_manager = catalog.apps[0].clone();
    file_manager.app_id = "file-manager".into();
    file_manager.display_name = "File Manager".into();
    file_manager.package.package_id = "thunar".into();
    file_manager.package.version = "4.18.8-1".into();
    file_manager.package.package_sha256 =
        "sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd".into();
    file_manager.executable_id = "file-manager".into();
    catalog.apps.push(file_manager);
    catalog.signature_hex = signer
        .sign(&catalog.signing_bytes().unwrap())
        .to_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let trust = GuestApplicationCatalogTrustV1::from_application_pinned_keys([(
        key_id,
        signer.verifying_key(),
    )])
    .unwrap();
    VerifiedGuestApplicationCatalogV1::verify(catalog, &trust).unwrap()
}

pub(super) fn request(identity: ComputerIdentityV1, operation: Kind) -> Request {
    Request {
        schema_version: 1,
        operation_id: uuid::Uuid::new_v4().to_string(),
        identity,
        session_class: ComputerSessionClassV1::Disposable,
        app_id: "text-editor".into(),
        operation,
        catalog_digest: signed_catalog().digest().to_owned(),
        image_digest: IMAGE.into(),
        architecture: GuestAppArchitectureV1::X86_64,
        lease_generation: 3,
        approval_generation: 2,
        now_unix_ms: NOW,
        granted_capabilities: BTreeSet::from([Capability::GraphicalDisplay]),
        network_granted: false,
    }
}

pub(super) fn approval(request: &Request) -> Approval {
    Approval::approved_by_controller(
        request.identity.clone(),
        request.session_class,
        request.app_id.clone(),
        request.operation,
        request.catalog_digest.clone(),
        request.image_digest.clone(),
        request.architecture,
        request.lease_generation,
        request.approval_generation,
        NOW + 60_000,
        request.granted_capabilities.clone(),
        request.network_granted,
    )
}

pub(super) fn provisioner() -> GuestAppProvisionerV1 {
    GuestAppProvisionerV1::new(signed_catalog())
}
