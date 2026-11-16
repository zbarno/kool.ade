use super::super::{
    ComputerGuestArchitectureV1 as Architecture, GuestImageEntryV1, GuestImageStatusV1,
};
use crate::computer_use::guest_image::ImageCacheErrorV1;

#[test]
fn guest_architecture_schema_has_stable_debian_and_artifact_names() {
    let cases = [
        (
            Architecture::X86_64,
            "x86_64",
            "amd64",
            "koolade-computer-use-guest-x86_64.qcow2",
        ),
        (
            Architecture::Aarch64,
            "aarch64",
            "arm64",
            "koolade-computer-use-guest-aarch64.qcow2",
        ),
    ];
    for (architecture, wire_name, debian_arch, artifact_name) in cases {
        assert_eq!(
            serde_json::to_string(&architecture).unwrap(),
            format!("\"{wire_name}\"")
        );
        assert_eq!(architecture.debian_architecture(), debian_arch);
        assert_eq!(architecture.artifact_name(), artifact_name);
        assert_eq!(
            serde_json::from_str::<Architecture>(&format!("\"{wire_name}\"")).unwrap(),
            architecture
        );
    }
    assert!(serde_json::from_str::<Architecture>("\"arm64\"").is_err());
}

#[test]
fn image_entry_binds_artifact_name_to_architecture() {
    let entry = |architecture, artifact_name: &str| GuestImageEntryV1 {
        architecture,
        image_digest: format!("sha256:{}", "0".repeat(64)),
        image_size_bytes: 2 * 1024 * 1024 * 1024,
        artifact_size_bytes: 1,
        artifact_name: artifact_name.into(),
        guest_release: "debian-bookworm-test".into(),
        source_revision: "a".repeat(40),
        build_recipe_sha256: format!("sha256:{}", "0".repeat(64)),
        sbom_sha256: format!("sha256:{}", "0".repeat(64)),
        build_attestation_sha256: format!("sha256:{}", "0".repeat(64)),
        companion_protocol_version: 1,
        companion_sha256: format!("sha256:{}", "0".repeat(64)),
        status: GuestImageStatusV1::Revoked,
    };

    for architecture in [Architecture::X86_64, Architecture::Aarch64] {
        assert!(
            entry(architecture, architecture.artifact_name())
                .validate()
                .is_ok()
        );
        if architecture == Architecture::X86_64 {
            assert!(
                entry(architecture, Architecture::X86_64.artifact_name())
                    .validate()
                    .is_ok()
            );
        } else {
            assert!(matches!(
                entry(architecture, Architecture::X86_64.artifact_name()).validate(),
                Err(ImageCacheErrorV1::InvalidManifest(
                    "invalid artifact metadata"
                ))
            ));
        }
    }
}
