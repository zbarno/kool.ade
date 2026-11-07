use super::*;
use crate::persistence::setup_tasks::SetupTaskEventV1;

fn task(id: &str, state: State) -> LocalTask {
    LocalTask {
        schema_version: 1,
        id: id.into(),
        title: "Synthetic readiness task".into(),
        explanation: "Synthetic fixture.".into(),
        next_step: "Rescan after the prerequisite changes.".into(),
        dependencies: Vec::new(),
        blocked_by: Vec::new(),
        state,
        created_at_unix_ms: 1,
        observed_at_unix_ms: 1,
        scan_generation: 1,
        verified_at_unix_ms: None,
        verified_result: None,
        attempts: Vec::new(),
        events: vec![SetupTaskEventV1 {
            observed_at_unix_ms: 1,
            scan_generation: 1,
            state,
        }],
        superseded_by: Vec::new(),
    }
}

#[test]
fn ready_provider_and_docker_do_not_mark_a_missing_vm_ready() {
    let tasks = vec![task("setup:cli:codex", State::Verified)];
    let matrix = abilities(&tasks, &[], Some(true), true);
    assert_eq!(matrix[0].availability, Availability::Available);
    assert!(
        matrix[1..]
            .iter()
            .all(|ability| ability.availability == Availability::Blocked)
    );
}

#[test]
fn vm_unsupported_state_does_not_disable_docker_coding() {
    let tasks = vec![
        task("setup:computer:kvm", State::Unsupported),
        task("setup:computer:qemu", State::NeedsVerification),
    ];
    let matrix = abilities(&tasks, &[], Some(true), true);
    assert_eq!(matrix[0].availability, Availability::Available);
    assert_eq!(matrix[1].availability, Availability::Unsupported);
    assert_eq!(matrix[2].availability, Availability::Unsupported);
}

#[test]
fn network_personal_and_viewer_modes_require_independent_verified_tasks() {
    let mut tasks = VM_BASE
        .iter()
        .map(|id| task(id, State::Verified))
        .collect::<Vec<_>>();
    tasks.push(task("setup:computer:personal-encryption", State::Verified));
    tasks.push(task("setup:computer:viewer", State::NeedsVerification));
    let matrix = abilities(&tasks, &[], Some(false), true);
    assert_eq!(matrix[0].availability, Availability::Blocked);
    assert_eq!(matrix[1].availability, Availability::Available);
    assert_eq!(matrix[2].availability, Availability::Blocked);
    assert_eq!(matrix[3].availability, Availability::Available);
    assert_eq!(matrix[4].availability, Availability::Blocked);
}

#[test]
fn internet_mode_requires_a_verified_restricted_gateway() {
    let mut tasks = VM_BASE
        .iter()
        .map(|id| task(id, State::Verified))
        .collect::<Vec<_>>();
    tasks.push(task("setup:computer:network", State::Verified));
    assert_eq!(
        abilities(&tasks, &[], Some(false), true)[2].availability,
        Availability::Available
    );
    tasks[4].state = State::NeedsVerification;
    assert_eq!(
        abilities(&tasks, &[], Some(false), true)[2].availability,
        Availability::Blocked
    );
}

#[test]
fn current_positive_kvm_scan_proves_kvm_when_no_blocker_task_is_needed() {
    let tasks = VM_BASE
        .iter()
        .filter(|id| **id != "setup:computer:kvm")
        .map(|id| task(id, State::Verified))
        .collect::<Vec<_>>();
    assert_eq!(
        abilities(&tasks, &["computer:kvm".into()], Some(false), true)[1].availability,
        Availability::Available
    );
    assert_eq!(
        abilities(&tasks, &[], Some(false), true)[1].availability,
        Availability::Blocked
    );
}

#[test]
fn stale_scan_cannot_keep_any_capability_available() {
    let tasks = VM_BASE
        .iter()
        .map(|id| task(id, State::Verified))
        .collect::<Vec<_>>();
    let matrix = abilities(&tasks, &["computer:kvm".into()], Some(true), false);
    assert!(
        matrix
            .iter()
            .all(|ability| ability.availability == Availability::Blocked)
    );
}
