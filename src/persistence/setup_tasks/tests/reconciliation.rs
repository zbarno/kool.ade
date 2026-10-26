use crate::app::setup_attention::catalog::FindingStatus;

use super::super::{DependencyResolution, SetupTaskStateV1, SetupTaskStoreV1, ordered_task_ids};
use super::support::{scan, snapshot};

#[path = "reconciliation/attempt_verification.rs"]
mod attempt_verification;
#[path = "reconciliation/git.rs"]
mod git;
#[path = "reconciliation/image_superseding.rs"]
mod image_superseding;
#[path = "reconciliation/images.rs"]
mod images;
#[path = "reconciliation/legacy.rs"]
mod legacy;

#[test]
fn multiple_findings_have_stable_ids_and_bounded_transition_history() {
    let mut store = SetupTaskStoreV1::default();
    let first = snapshot(
        &[
            ("setup:cli:codex:auth", FindingStatus::NeedsAttention, &[]),
            ("setup:docker:rootless", FindingStatus::NeedsAttention, &[]),
        ],
        &["cli:codex:auth", "docker:rootless"],
        10,
    );
    scan(&mut store, &first).unwrap();
    assert_eq!(store.tasks.len(), 2);
    assert!(
        store
            .tasks
            .values()
            .all(|task| task.state == SetupTaskStateV1::ReadyToFix)
    );

    let event_count = store.tasks["setup:docker:rootless"].events.len();
    let repeated = snapshot(
        &[
            ("setup:cli:codex:auth", FindingStatus::NeedsAttention, &[]),
            ("setup:docker:rootless", FindingStatus::NeedsAttention, &[]),
        ],
        &["cli:codex:auth", "docker:rootless"],
        20,
    );
    scan(&mut store, &repeated).unwrap();
    assert_eq!(store.tasks.len(), 2);
    assert_eq!(
        store.tasks["setup:docker:rootless"].events.len(),
        event_count
    );
    assert_eq!(
        ordered_task_ids(&store).unwrap(),
        vec![
            "setup:cli:codex:auth".to_owned(),
            "setup:docker:rootless".to_owned(),
        ]
    );
}

#[test]
fn only_current_exact_positive_evidence_verifies_and_regression_reopens() {
    let mut store = SetupTaskStoreV1::default();
    scan(
        &mut store,
        &snapshot(
            &[("setup:docker:rootless", FindingStatus::NeedsAttention, &[])],
            &["docker:rootless"],
            1,
        ),
    )
    .unwrap();

    scan(&mut store, &snapshot(&[], &["cli:codex:auth"], 2)).unwrap();
    assert_eq!(
        store.tasks["setup:docker:rootless"].state,
        SetupTaskStateV1::ReadyToFix
    );

    scan(&mut store, &snapshot(&[], &["docker:rootless"], 3)).unwrap();
    let task = &store.tasks["setup:docker:rootless"];
    assert_eq!(task.state, SetupTaskStateV1::Verified);
    assert_eq!(
        task.verified_result.as_ref().unwrap().evidence_domain,
        "docker"
    );

    scan(&mut store, &snapshot(&[], &["cli:codex:auth"], 4)).unwrap();
    let task = &store.tasks["setup:docker:rootless"];
    assert_eq!(task.state, SetupTaskStateV1::NeedsVerification);
    assert!(task.verified_result.is_none());
    assert_eq!(task.verified_at_unix_ms, Some(3));

    scan(
        &mut store,
        &snapshot(
            &[("setup:docker:rootless", FindingStatus::NeedsAttention, &[])],
            &["docker:rootless"],
            5,
        ),
    )
    .unwrap();
    assert_eq!(
        store.tasks["setup:docker:rootless"].state,
        SetupTaskStateV1::ReadyToFix
    );
    assert!(
        store.tasks["setup:docker:rootless"]
            .verified_result
            .is_none()
    );
}

#[test]
fn changed_docker_failure_does_not_verify_an_unrelated_task() {
    let mut store = SetupTaskStoreV1::default();
    scan(
        &mut store,
        &snapshot(
            &[("setup:docker:rootless", FindingStatus::NeedsAttention, &[])],
            &[],
            1,
        ),
    )
    .unwrap();
    scan(
        &mut store,
        &snapshot(
            &[("setup:docker:start", FindingStatus::NeedsAttention, &[])],
            &[],
            2,
        ),
    )
    .unwrap();
    assert_eq!(
        store.tasks["setup:docker:rootless"].state,
        SetupTaskStateV1::ReadyToFix
    );
}

#[test]
fn a_dependency_blocks_children_then_unblocks_from_current_parent_evidence() {
    let mut store = SetupTaskStoreV1::default();
    scan(
        &mut store,
        &snapshot(
            &[
                (
                    "setup:images:general:amd64",
                    FindingStatus::NeedsAttention,
                    &["setup:docker:rootless"],
                ),
                ("setup:docker:rootless", FindingStatus::NeedsAttention, &[]),
            ],
            &["docker:rootless"],
            1,
        ),
    )
    .unwrap();
    let child = &store.tasks["setup:images:general:amd64"];
    assert_eq!(child.state, SetupTaskStateV1::BlockedByPrerequisite);
    assert_eq!(child.blocked_by, vec!["setup:docker:rootless"]);
    assert_eq!(
        child.dependencies[0].resolution,
        DependencyResolution::Pending
    );

    scan(
        &mut store,
        &snapshot(
            &[(
                "setup:images:general:amd64",
                FindingStatus::NeedsAttention,
                &["setup:docker:rootless"],
            )],
            &["docker:rootless"],
            2,
        ),
    )
    .unwrap();
    let parent = &store.tasks["setup:docker:rootless"];
    let child = &store.tasks["setup:images:general:amd64"];
    assert_eq!(parent.state, SetupTaskStateV1::Verified);
    assert_eq!(child.state, SetupTaskStateV1::ReadyToFix);
    assert_eq!(
        child.dependencies[0].resolution,
        DependencyResolution::SatisfiedByTask { scan_generation: 2 }
    );
}

#[test]
fn cycle_and_duplicate_findings_reject_the_whole_snapshot() {
    let mut store = SetupTaskStoreV1 {
        latest_scan_generation: 1,
        ..SetupTaskStoreV1::default()
    };
    let cycle = snapshot(
        &[
            (
                "setup:test:a",
                FindingStatus::NeedsAttention,
                &["setup:test:b"],
            ),
            (
                "setup:test:b",
                FindingStatus::NeedsAttention,
                &["setup:test:a"],
            ),
        ],
        &[],
        1,
    );
    assert!(store.reconcile(&cycle, 1).is_err());
    assert!(
        store.tasks.is_empty(),
        "failed graph reconciliation must be atomic"
    );

    let duplicate = snapshot(
        &[
            ("setup:docker:rootless", FindingStatus::NeedsAttention, &[]),
            ("setup:docker:rootless", FindingStatus::NeedsAttention, &[]),
        ],
        &[],
        2,
    );
    assert!(store.reconcile(&duplicate, 1).is_err());
    assert!(store.tasks.is_empty());
}

#[test]
fn image_inventory_alone_keeps_synthetic_smoke_blocked() {
    let mut store = SetupTaskStoreV1::default();
    scan(
        &mut store,
        &snapshot(
            &[
                (
                    "setup:images:general:amd64",
                    FindingStatus::NeedsAttention,
                    &["setup:docker:rootless"],
                ),
                (
                    "setup:smoke:general:amd64",
                    FindingStatus::NeedsAttention,
                    &["setup:images:general:amd64"],
                ),
                ("setup:docker:rootless", FindingStatus::NeedsAttention, &[]),
            ],
            &[],
            1,
        ),
    )
    .unwrap();
    assert_eq!(
        store.tasks["setup:smoke:general:amd64"].state,
        SetupTaskStateV1::BlockedByPrerequisite
    );

    scan(
        &mut store,
        &snapshot(
            &[(
                "setup:smoke:general:amd64",
                FindingStatus::NeedsAttention,
                &["setup:images:general:amd64"],
            )],
            &["docker:rootless", "images:general:amd64"],
            2,
        ),
    )
    .unwrap();
    let order = ordered_task_ids(&store).unwrap();
    assert!(
        order
            .iter()
            .position(|id| id == "setup:images:general:amd64")
            < order
                .iter()
                .position(|id| id == "setup:smoke:general:amd64")
    );
    assert_eq!(
        store.tasks["setup:smoke:general:amd64"].state,
        SetupTaskStateV1::BlockedByPrerequisite
    );
}

#[test]
fn stale_generation_cannot_reopen_or_verify_a_task() {
    let mut store = SetupTaskStoreV1 {
        latest_scan_generation: 3,
        ..SetupTaskStoreV1::default()
    };
    let result = store.reconcile(&snapshot(&[], &["docker:rootless"], 4), 2);
    assert!(result.is_err());
    assert!(store.tasks.is_empty());
}
