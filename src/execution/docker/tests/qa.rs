use crate::execution::docker::{
    discovery::PrerequisiteCode,
    images::{ImageLock, ImageObservation, RuntimeProfile, verify_qa_image_observation},
};
use std::collections::BTreeMap;

#[test]
fn experimental_qa_image_requires_locked_digest_linux_amd64_and_non_root_user() {
    use crate::persistence::experimental_features::{
        ExperimentalFeature, ExperimentalFeatureSettingsV1,
    };

    let lock = ImageLock::bundled().unwrap();
    let settings = ExperimentalFeatureSettingsV1 {
        enabled: true,
        confirmed_at_unix_ms: Some(1),
        enabled_features: [ExperimentalFeature::BrowserQa].into(),
        ..ExperimentalFeatureSettingsV1::default()
    };
    let selected = lock
        .select_experimental_qa(RuntimeProfile::BrowserQa, "amd64", &settings)
        .unwrap();
    let observation = ImageObservation {
        image_id: format!("sha256:{}", "1".repeat(64)),
        repo_digests: vec![selected.reference.clone()],
        os: "linux".into(),
        architecture: "amd64".into(),
        configured_user: selected.runtime_user.clone(),
        labels: BTreeMap::new(),
    };
    verify_qa_image_observation(&selected, &observation).unwrap();

    let mut root_user = observation.clone();
    root_user.configured_user = "0:0".into();
    assert_eq!(
        verify_qa_image_observation(&selected, &root_user)
            .unwrap_err()
            .code,
        PrerequisiteCode::ImageUnavailable
    );

    let mut wrong_digest = observation;
    wrong_digest.repo_digests = vec!["ghcr.io/example/unrelated@sha256:ffffffff".into()];
    assert_eq!(
        verify_qa_image_observation(&selected, &wrong_digest)
            .unwrap_err()
            .code,
        PrerequisiteCode::ImageDigestMismatch
    );
}
