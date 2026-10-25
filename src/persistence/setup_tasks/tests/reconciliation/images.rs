use crate::app::setup_attention::catalog::FindingStatus;

use super::super::support::{scan, snapshot};
use crate::persistence::setup_tasks::{SetupTaskStateV1, SetupTaskStoreV1};

#[test]
fn image_evidence_verifies_only_the_exact_profile_and_architecture() {
    let mut store = SetupTaskStoreV1::default();
    scan(
        &mut store,
        &snapshot(
            &[
                (
                    "setup:images:general:amd64",
                    FindingStatus::NeedsAttention,
                    &[],
                ),
                (
                    "setup:images:rust:amd64",
                    FindingStatus::NeedsAttention,
                    &[],
                ),
            ],
            &[],
            1,
        ),
    )
    .unwrap();
    scan(&mut store, &snapshot(&[], &["images:rust:amd64"], 2)).unwrap();
    assert_eq!(
        store.tasks["setup:images:rust:amd64"].state,
        SetupTaskStateV1::Verified
    );
    assert_eq!(
        store.tasks["setup:images:general:amd64"].state,
        SetupTaskStateV1::ReadyToFix
    );
}

#[test]
fn legacy_docker_image_findings_require_the_current_general_image_architecture() {
    for id in [
        "setup:docker:image-unverified",
        "setup:docker:image-digest",
        "setup:docker:image-architecture",
    ] {
        let status = if id == "setup:docker:image-unverified" {
            FindingStatus::Unverified
        } else {
            FindingStatus::Unsupported
        };
        let mut store = SetupTaskStoreV1::default();
        scan(&mut store, &snapshot(&[(id, status, &[])], &[], 1)).unwrap();
        scan(&mut store, &snapshot(&[], &["docker"], 2)).unwrap();
        let prior_state = if status == FindingStatus::Unsupported {
            SetupTaskStateV1::Unsupported
        } else {
            SetupTaskStateV1::NeedsVerification
        };
        assert_eq!(store.tasks[id].state, prior_state, "{id}");

        let mut wrong_architecture = snapshot(&[], &["images:general:arm64"], 3);
        wrong_architecture
            .capability_matrix
            .observed_host_architecture =
            crate::app::setup_attention::catalog::ImageArchitectureV1::Amd64;
        scan(&mut store, &wrong_architecture).unwrap();
        assert_eq!(store.tasks[id].state, prior_state, "{id}");

        scan(&mut store, &snapshot(&[], &["images:general:amd64"], 4)).unwrap();
        assert_eq!(store.tasks[id].state, SetupTaskStateV1::Verified, "{id}");
    }
}
