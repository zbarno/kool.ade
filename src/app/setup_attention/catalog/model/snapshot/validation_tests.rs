use super::{
    ImageArchitectureV1, ImageProfileV1, PrerequisiteSubject, rootless_smoke_subject_matches,
};

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
