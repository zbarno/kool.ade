use super::*;
use crate::execution::docker::images::RuntimeProfile;

#[test]
fn workload_requirement_creates_only_its_missing_runtime_profile_task() {
    let engine = docker(DockerEnvironmentState::MissingApprovedImage, true);
    let snapshot = assess_with_images(
        &engine,
        [RuntimeProfile::Rust].into_iter().collect(),
        std::collections::BTreeSet::new(),
        31,
    );
    let identity = engine.engine_identity_fingerprint().unwrap();
    for profile in ["general", "rust"] {
        assert!(
            snapshot.findings.iter().any(|finding| {
                finding.id == format!("setup:images:{profile}:amd64:{identity}")
            })
        );
    }
    assert!(!snapshot.findings.iter().any(|finding| {
        ["node", "python", "go"]
            .iter()
            .any(|profile| finding.id.starts_with(&format!("setup:images:{profile}:")))
    }));
}
