//! Scenario tests for the flow driver.
//!
//! Production boundary: no UI, no controllers, no filesystem, no provider —
//! the persisted task states plus injected slot/clamp signals stand in for a
//! machine, so the ordering, consent, retry, halt, and finish policies are
//! proven exhaustively and deterministically.

use super::super::{FlowAction, order, policy};
use super::*;
use crate::persistence::setup_tasks::{
    AttemptState, SetupAttemptV1, SetupFlowStateV1, SetupTaskV1, State, UserActionReasonV1,
};

const NOW: u64 = 1_000;
const GEN: u64 = 7;

fn flow(mut mutate: impl FnMut(&mut SetupFlowStateV1)) -> SetupFlowStateV1 {
    let mut state =
        SetupFlowStateV1::start_new("flow-unit-0001".to_string(), NOW).expect("valid flow id");
    state.last_scan_generation = GEN;
    mutate(&mut state);
    state
}

fn task(id: &str, state: State, blocked_by: &[&str], failed_attempts: usize) -> SetupTaskV1 {
    let attempts = (0..failed_attempts)
        .map(|_| SetupAttemptV1 {
            operation_id: "{operation}".to_string(),
            version: 1,
            owner_instance_id: None,
            requested_scopes: Vec::new(),
            authorization_binding: None,
            authorization_history: None,
            state: AttemptState::Failed,
            started_at_unix_ms: NOW,
            updated_at_unix_ms: NOW,
            verified_result: None,
            remediation_result: None,
            image_preparation_receipt: None,
            failure: Some(crate::persistence::setup_tasks::FailureCode::OperationFailed),
        })
        .collect();
    SetupTaskV1 {
        schema_version: 1,
        id: id.into(),
        title: id.into(),
        explanation: String::new(),
        next_step: String::new(),
        dependencies: Vec::new(),
        blocked_by: blocked_by.iter().map(|id| id.to_string()).collect(),
        state,
        created_at_unix_ms: 1,
        observed_at_unix_ms: 1,
        scan_generation: GEN,
        verified_at_unix_ms: None,
        verified_result: None,
        attempts,
        events: Vec::new(),
        superseded_by: Vec::new(),
    }
}

fn input(flow_state: &SetupFlowStateV1, tasks: Vec<SetupTaskV1>) -> BeatInput<'_> {
    input_ext(flow_state, tasks, BeatTune::default())
}

#[derive(Default, Clone)]
struct BeatTune {
    scan_generation: u64,
    needs_fresh_scan: bool,
    probe_busy: bool,
    last_rescan_at: Option<u64>,
    slots: &'static [SlotStatus],
}

fn input_ext(
    flow_state: &SetupFlowStateV1,
    tasks: Vec<SetupTaskV1>,
    tune: BeatTune,
) -> BeatInput<'_> {
    // Test-only: the returned input must outlive the call, so the snapshot is
    // parked for the process lifetime. Suites are finite, so the leak is bounded.
    let parked: &'static [SetupTaskV1] = Box::leak(tasks.into_boxed_slice());
    BeatInput {
        flow: Some(flow_state),
        tasks: parked,
        scan_generation: tune.scan_generation.max(GEN),
        probe_busy: tune.probe_busy,
        slots: tune.slots,
        needs_fresh_scan: tune.needs_fresh_scan,
        now_unix_ms: NOW,
        rescan_cooldown_ms: DEFAULT_RESCAN_COOLDOWN_MS,
        last_rescan_at: tune.last_rescan_at,
    }
}

fn begin(action: &FlowAction, id: &str) -> bool {
    matches!(action, FlowAction::BeginFix { task_id, .. } if task_id == id)
}

#[test]
fn prerequisite_chain_executes_in_dependency_order() {
    let state = flow(|_| {});
    let mk = |blocked: &[&str]| {
        vec![
            task("setup:linux:rootless:packages", State::ReadyToFix, &[], 0),
            task("setup:linux:rootless:subids", State::ReadyToFix, blocked, 0),
            task(
                "setup:computer:qemu",
                State::ReadyToFix,
                &["setup:linux:rootless:subids"],
                0,
            ),
        ]
    };
    let first = beat(&input(&state, mk(&["setup:linux:rootless:packages"])));
    assert!(begin(&first, "setup:linux:rootless:packages"));

    let second = beat(&input(
        &state,
        mk(&[])
            .into_iter()
            .map(|mut t| {
                if t.id == "setup:linux:rootless:packages" {
                    t.state = State::Verified;
                }
                t
            })
            .collect(),
    ));
    assert!(begin(&second, "setup:linux:rootless:subids"));
}

#[test]
fn foundations_run_before_independent_optional_work() {
    let state = flow(|_| {});
    let tasks = vec![
        task("setup:computer:qemu", State::ReadyToFix, &[], 0),
        task("setup:linux:rootless:packages", State::ReadyToFix, &[], 0),
    ];
    assert!(begin(
        &beat(&input(&state, tasks.clone())),
        "setup:linux:rootless:packages"
    ));
    // Shuffled input must yield the identical plan (determinism).
    assert!(begin(
        &beat(&input(&state, tasks.into_iter().rev().collect())),
        "setup:linux:rootless:packages"
    ));
}

#[test]
fn a_presented_consent_review_parks_the_loop_in_waiting_for_consent() {
    let state = flow(|_| {});
    let tasks = vec![task("setup:computer:qemu", State::NeedsApproval, &[], 0)];
    assert_eq!(
        beat(&input(&state, tasks.clone())),
        FlowAction::SyncPhase {
            phase: FlowPhaseV1::WaitingForConsent {
                task_id: "setup:computer:qemu".into()
            },
            detail_task: None
        }
    );
    // Still under review next beat.
    assert!(matches!(
        beat(&input(&state, tasks)),
        FlowAction::SyncPhase {
            phase: FlowPhaseV1::WaitingForConsent { .. },
            ..
        }
    ));
}

#[test]
fn an_approved_operation_in_flight_holds_until_it_verifies() {
    let state = flow(|_| {});
    let tasks = vec![task("setup:computer:qemu", State::InProgress, &[], 0)];
    assert_eq!(beat(&input(&state, tasks)), FlowAction::Hold);
}

#[test]
fn a_verified_operation_demands_a_fresh_scan_before_downstream_work() {
    let state = flow(|_| {});
    let verified = task("setup:linux:rootless:packages", State::Verified, &[], 0);
    let waiting = task(
        "setup:linux:rootless:subids",
        State::ReadyToFix,
        &["setup:linux:rootless:packages"],
        0,
    );
    let input = input_ext(
        &state,
        vec![verified.clone(), waiting.clone()],
        BeatTune {
            scan_generation: GEN,
            needs_fresh_scan: true,
            ..Default::default()
        },
    );
    assert_eq!(beat(&input), FlowAction::TriggerRescan);
    // Cooldown honored.
    let cooled = input_ext(
        &state,
        vec![verified, waiting],
        BeatTune {
            scan_generation: GEN,
            needs_fresh_scan: true,
            last_rescan_at: Some(NOW - 10),
            ..Default::default()
        },
    );
    assert_eq!(beat(&cooled), FlowAction::Hold);
}

#[test]
fn a_new_detection_gets_promoted_by_a_scan_instead_of_deadlocking() {
    let state = flow(|_| {});
    let tasks = vec![task("setup:computer:qemu", State::Detected, &[], 0)];
    assert_eq!(beat(&input(&state, tasks)), FlowAction::TriggerRescan);
}

#[test]
fn unsupported_repairs_escalate_with_their_human_reason() {
    let state = flow(|_| {});
    let cases = [
        (
            "setup:docker:engine",
            State::ReadyToFix,
            UserActionReasonV1::ManualInstructions,
        ),
        (
            "setup:cli:codex:auth",
            State::ReadyToFix,
            UserActionReasonV1::VendorAuthentication,
        ),
        (
            "setup:docker:permission",
            State::ReadyToFix,
            UserActionReasonV1::OperatingSystemPrompt,
        ),
    ];
    for (id, st, reason) in cases {
        assert_eq!(
            beat(&input(&state, vec![task(id, st, &[], 0)])),
            FlowAction::SyncPhase {
                phase: FlowPhaseV1::WaitingForUser {
                    task_id: id.into(),
                    reason
                },
                detail_task: None
            },
            "{id}"
        );
    }
}

#[test]
fn exhausted_retries_stop_the_auto_loop_and_ask_for_judgment() {
    let state = flow(|_| {});
    let failed = task(
        "setup:computer:qemu",
        State::Failed,
        &[],
        policy::RETRY_LIMIT,
    );
    assert_eq!(
        beat(&input(&state, vec![failed])),
        FlowAction::SyncPhase {
            phase: FlowPhaseV1::WaitingForUser {
                task_id: "setup:computer:qemu".into(),
                reason: UserActionReasonV1::RepeatedFailure
            },
            detail_task: None
        }
    );
    // Under the limit, the same task gets its automatic retry.
    let retryable = task(
        "setup:computer:qemu",
        State::Failed,
        &[],
        policy::RETRY_LIMIT - 1,
    );
    assert!(begin(
        &beat(&input(&state, vec![retryable])),
        "setup:computer:qemu"
    ));
}

#[test]
fn skipped_operations_are_bypassed_until_an_explicit_restart() {
    let state = flow(|state| {
        state.skipped.push("setup:computer:qemu".into());
    });
    let tasks = vec![
        task("setup:computer:qemu", State::ReadyToFix, &[], 0),
        task("setup:linux:rootless:packages", State::ReadyToFix, &[], 0),
    ];
    assert!(begin(
        &beat(&input(&state, tasks)),
        "setup:linux:rootless:packages"
    ));
}

#[test]
fn all_verified_completes_only_against_a_current_scan() {
    let state = flow(|_| {});
    let verified = task("setup:computer:qemu", State::Verified, &[], 0);
    assert_eq!(
        beat(&input(&state, vec![verified.clone()])),
        FlowAction::SyncPhase {
            phase: FlowPhaseV1::Finishing,
            detail_task: None
        }
    );
    let finishing = flow(|state| state.phase = FlowPhaseV1::Finishing);
    assert_eq!(
        beat(&input(&finishing, vec![verified.clone()])),
        FlowAction::Finish(DoneOutcomeV1::FullyPrepared)
    );
    let mixed = vec![
        verified.clone(),
        task("setup:computer:viewer", State::Unsupported, &[], 0),
    ];
    assert_eq!(
        beat(&input(&finishing, mixed)),
        FlowAction::Finish(DoneOutcomeV1::PreparedWithUnavailable {
            unavailable_count: 1
        })
    );
    // A newer scan arrived: demand it before declaring victory.
    let tuned = input_ext(
        &state,
        vec![verified.clone()],
        BeatTune {
            scan_generation: GEN + 1,
            ..Default::default()
        },
    );
    assert_eq!(beat(&tuned), FlowAction::TriggerRescan);
}

#[test]
fn a_regressed_verification_halts_the_flow_instead_of_auto_repairing() {
    let state = flow(|state| {
        state.seen_verified.push("setup:computer:qemu".into());
    });
    let regressed = task("setup:computer:qemu", State::Failed, &[], 0);
    assert_eq!(
        beat(&input(&state, vec![regressed])),
        FlowAction::Halt {
            reason: HaltReasonV1::ReadinessChanged,
            detail_task: Some("setup:computer:qemu".into())
        }
    );
}

#[test]
fn terminal_phases_hold_and_recovered_phases_resume_cleanly() {
    let halted = flow(|state| {
        state.phase = FlowPhaseV1::Halted {
            reason: HaltReasonV1::InterruptedByRestart,
            detail_task: None,
        };
    });
    assert_eq!(beat(&input(&halted, vec![])), FlowAction::Hold);
    let paused = flow(|state| state.phase = FlowPhaseV1::Paused);
    assert_eq!(beat(&input(&paused, vec![])), FlowAction::Hold);
    // No flow installed yet.
    let bare_state = flow(|_| {});
    let mut bare = input(&bare_state, vec![]);
    bare.flow = None;
    assert_eq!(beat(&bare), FlowAction::Hold);
}

#[test]
fn waiting_for_user_clears_once_the_operator_performs_the_step() {
    let state = flow(|state| {
        state.phase = FlowPhaseV1::WaitingForUser {
            task_id: "setup:docker:engine".into(),
            reason: UserActionReasonV1::ManualInstructions,
        };
    });
    let held = task("setup:docker:engine", State::ReadyToFix, &[], 0);
    assert_eq!(beat(&input(&state, vec![held])), FlowAction::Hold);
    let done = task("setup:docker:engine", State::Verified, &[], 0);
    assert_eq!(
        beat(&input(&state, vec![done])),
        FlowAction::SyncPhase {
            phase: FlowPhaseV1::Running,
            detail_task: None
        }
    );
}

#[test]
fn a_consented_but_reverted_review_returns_to_running_without_double_beginning() {
    let state = flow(|state| {
        state.phase = FlowPhaseV1::WaitingForConsent {
            task_id: "setup:computer:qemu".into(),
        };
    });
    let reverted = task("setup:computer:qemu", State::ReadyToFix, &[], 0);
    let back = beat(&input(&state, vec![reverted.clone()]));
    assert_eq!(
        back,
        FlowAction::SyncPhase {
            phase: FlowPhaseV1::Running,
            detail_task: None
        }
    );
    // Back in Running with the review canceled: begin once, cleanly.
    let fresh = flow(|_| {});
    let rerun = input(&fresh, vec![reverted.clone()]);
    assert!(begin(&beat(&rerun), "setup:computer:qemu"));
}

#[test]
fn sequence_errors_surface_as_internal_halts() {
    let cyclic = vec![
        task("setup:cyc:one", State::ReadyToFix, &["setup:cyc:two"], 0),
        task("setup:cyc:two", State::ReadyToFix, &["setup:cyc:one"], 0),
    ];
    assert!(matches!(
        order::sequence(&cyclic),
        Err(order::SequenceError::Cycle)
    ));
    let state = flow(|_| {});
    assert_eq!(
        beat(&input(&state, cyclic)),
        FlowAction::Halt {
            reason: HaltReasonV1::InternalError,
            detail_task: None
        }
    );
}
#[test]
fn a_busy_family_holds_back_every_sibling_in_that_family() {
    // Family-serial executors: while ANY remediation operation is
    // in flight, no second remediation operation may be proposed, even
    // for a different task.
    let state = flow(|f| {
        f.seen_verified.clear();
    });
    let slots: &'static [SlotStatus] = Box::leak(Box::new([SlotStatus {
        family: Some(SlotFamily::Remediation),
        task_id: Some("setup:linux:rootless:packages".to_owned()),
        phase: SlotPhase::Running,
    }]));
    let tune = BeatTune {
        slots,
        ..Default::default()
    };
    let input = input_ext(
        &state,
        vec![task("setup:computer:qemu", State::ReadyToFix, &[], 0)],
        tune,
    );
    assert!(
        matches!(beat(&input), FlowAction::Hold),
        "same-family sibling must wait while the family is busy"
    );
}

#[test]
fn a_busy_family_does_not_block_independent_families() {
    let state = flow(|f| {
        f.seen_verified.clear();
    });
    let slots: &'static [SlotStatus] = Box::leak(Box::new([SlotStatus {
        family: Some(SlotFamily::Remediation),
        task_id: Some("setup:linux:rootless:packages".to_owned()),
        phase: SlotPhase::Running,
    }]));
    let tune = BeatTune {
        slots,
        ..Default::default()
    };
    let input = input_ext(
        &state,
        vec![
            task("setup:computer:qemu", State::ReadyToFix, &[], 0),
            task("setup:cli:claude:install", State::ReadyToFix, &[], 0),
        ],
        tune,
    );
    assert!(
        begin(&beat(&input), "setup:cli:claude:install"),
        "an independent family may proceed while remediation is busy"
    );
}
