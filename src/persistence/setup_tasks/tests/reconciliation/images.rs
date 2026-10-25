use super::super::*;

#[test]
fn image_evidence_verifies_only_the_exact_profile_and_architecture() {
    let mut store = Store::default();
    store
        .reconcile(&snapshot(
            &[
                ("setup:images:general:amd64", FindingStatus::NeedsAttention),
                ("setup:images:rust:amd64", FindingStatus::NeedsAttention),
            ],
            &[],
            1,
        ))
        .unwrap();
    store
        .reconcile(&snapshot(&[], &["images:rust:amd64"], 2))
        .unwrap();
    assert_eq!(
        store.tasks["setup:images:rust:amd64"].state,
        State::Verified
    );
    assert_eq!(
        store.tasks["setup:images:general:amd64"].state,
        State::NeedsAttention
    );
}

#[test]
fn legacy_docker_image_findings_resolve_only_after_exact_general_image_evidence() {
    for id in [
        "setup:docker:image-unverified",
        "setup:docker:image-digest",
        "setup:docker:image-architecture",
    ] {
        let mut store = Store::default();
        store
            .reconcile(&snapshot(&[(id, FindingStatus::NeedsAttention)], &[], 1))
            .unwrap();
        store.reconcile(&snapshot(&[], &["docker"], 2)).unwrap();
        assert_eq!(store.tasks[id].state, State::NeedsAttention, "{id}");
        let current = snapshot(&[], &["images:general:arm64"], 3);
        assert_eq!(
            current.capability_matrix.observed_host_architecture,
            ImageArchitectureV1::Amd64,
            "the controller host architecture differs from the daemon image evidence"
        );
        store.reconcile(&current).unwrap();
        assert_eq!(store.tasks[id].state, State::Verified, "{id}");
    }
}
