use super::{
    ImageArchitectureV1, ImageProfileV1, PrerequisiteSubject, rootless_smoke_subject_matches,
    valid_macos_version,
};

#[test]
fn macos_release_versions_are_bounded_numeric_components() {
    assert!(valid_macos_version("15.4.1"));
    assert!(!valid_macos_version(""));
    assert!(!valid_macos_version("15..1"));
    assert!(!valid_macos_version("15.4-beta"));
    assert!(!valid_macos_version(&"1".repeat(33)));
}

#[test]
fn rootless_smoke_receipt_subject_must_be_general_and_match_its_platform() {
    assert!(rootless_smoke_subject_matches(
        &PrerequisiteSubject::Image {
            profile: ImageProfileV1::General,
            architecture: ImageArchitectureV1::Amd64,
        },
        "linux/amd64",
    ));
    assert!(rootless_smoke_subject_matches(
        &PrerequisiteSubject::Image {
            profile: ImageProfileV1::General,
            architecture: ImageArchitectureV1::Arm64,
        },
        "linux/arm64",
    ));
    assert!(!rootless_smoke_subject_matches(
        &PrerequisiteSubject::Image {
            profile: ImageProfileV1::Rust,
            architecture: ImageArchitectureV1::Amd64,
        },
        "linux/amd64",
    ));
    assert!(!rootless_smoke_subject_matches(
        &PrerequisiteSubject::Image {
            profile: ImageProfileV1::General,
            architecture: ImageArchitectureV1::Amd64,
        },
        "linux/arm64",
    ));
}
