use super::activation_evidence;

fn capability() -> super::capability_probe::ActiveCapabilityProbeEvidence {
    super::capability_probe::ActiveCapabilityProbeEvidence {
        capability_id: "self-improvement-lifecycle".into(),
        readiness_probe_id: "self-improvement-lifecycle-v1".into(),
        manifest_sha256: "c".repeat(64),
    }
}

#[test]
fn active_build_probe_is_bound_to_the_run_and_merged_commit() {
    let run_uid = uuid::Uuid::new_v4().to_string();
    let feature_uid = uuid::Uuid::new_v4().to_string();
    let merge_sha = "a".repeat(40);
    let evidence = activation_evidence(
        &run_uid,
        &feature_uid,
        &capability(),
        &merge_sha,
        "0.2.0",
        &merge_sha,
    )
    .unwrap();
    assert_eq!(evidence.feature_uid, feature_uid);
    assert_eq!(evidence.merged_sha, merge_sha);
    assert_eq!(evidence.build_commit_sha, merge_sha);
    assert_eq!(
        evidence.readiness_probe_uid,
        activation_evidence(
            &run_uid,
            &evidence.feature_uid,
            &capability(),
            &merge_sha,
            "0.2.0",
            &merge_sha
        )
        .unwrap()
        .readiness_probe_uid
    );
}

#[test]
fn active_build_probe_rejects_unmerged_and_invalid_build_identities() {
    let run_uid = uuid::Uuid::new_v4().to_string();
    let feature_uid = uuid::Uuid::new_v4().to_string();
    let merge_sha = "a".repeat(40);
    assert!(
        activation_evidence(
            &run_uid,
            &feature_uid,
            &capability(),
            &merge_sha,
            "0.2.0",
            &"b".repeat(40)
        )
        .is_err()
    );
    assert!(
        activation_evidence(
            &run_uid,
            &feature_uid,
            &capability(),
            &merge_sha,
            "0.2.0",
            "unknown"
        )
        .is_err()
    );
}
