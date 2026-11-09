//! App-integration coverage for the "Prepare this computer" flow commands.
//!
//! Boundary under test: operator commands must persist the flow position
//! through the real transaction path (file lock, trim-on-write) on a
//! connected app, so a restart or crash resumes from durable state rather
//! than in-memory luck. Uses the board fixture (ephemeral git + state dirs,
//! no Docker/network) with a redirected `KOOLADE_HOME` so persistence
//! round-trips stay hermetic.

use super::KooladeApp;
use crate::persistence::setup_tasks::{
    FlowPhaseV1, HaltReasonV1, SetupFlowStateV1, insert_flow, load_flow, update_flow,
};

const NOW: u64 = 1_700_000_001_000;

fn with_connected_app<R>(home: &std::path::Path, use_app: impl FnOnce(&mut KooladeApp) -> R) -> R {
    let guard = crate::test_support::KooladeHomeGuard::set(home);
    let mut app = crate::app::root::board_tests::fixture();
    let result = use_app(&mut app);
    drop(guard);
    result
}

fn phase_of(state: &SetupFlowStateV1) -> &FlowPhaseV1 {
    &state.phase
}

/// Restart recovery: a flow persisted mid-run becomes auditable-halted, and
/// the pump is idempotent afterwards.
#[test]
fn restart_recovery_halts_an_in_flight_flow() {
    let home = tempfile::tempdir().unwrap();
    {
        let _guard = crate::test_support::KooladeHomeGuard::set(home.path());
        insert_flow(SetupFlowStateV1::start_new("flow-boot-a1".to_owned(), NOW).unwrap()).unwrap();
        assert!(matches!(
            phase_of(load_flow().unwrap().as_ref().unwrap()),
            FlowPhaseV1::Running
        ));
    }
    let mut app = crate::app::root::board_tests::fixture();
    {
        let _guard = crate::test_support::KooladeHomeGuard::set(home.path());
        app.pump_setup_flow();
        let settled = load_flow().unwrap().expect("flow survives restart sync");
        assert!(matches!(
            phase_of(&settled),
            FlowPhaseV1::Halted { reason, .. }
                if *reason == HaltReasonV1::InterruptedByRestart
        ));
        assert!(matches!(
            app.setup_flow_cache.as_ref().map(phase_of),
            Some(FlowPhaseV1::Halted { .. })
        ));
        app.pump_setup_flow();
        let again = load_flow().unwrap().unwrap();
        assert!(matches!(
            phase_of(&again),
            FlowPhaseV1::Halted {
                reason: HaltReasonV1::InterruptedByRestart,
                ..
            }
        ));
    }
    drop(app);
    let _ = home;
}

/// Control commands: start persists a running flow, pause and stop move it,
/// resume keeps the flow, and starting from a stopped run is a clean slate.
#[test]
fn flow_control_commands_persist_through_real_transactions() {
    let home = tempfile::tempdir().unwrap();
    with_connected_app(home.path(), |app| {
        assert!(load_flow().unwrap().is_none());

        app.start_setup_flow_cmd();
        let started = load_flow().unwrap().expect("start persists");
        let first_id = started.flow_id.clone();
        assert!(matches!(phase_of(&started), FlowPhaseV1::Running));

        app.pause_setup_flow_cmd();
        assert!(matches!(
            phase_of(load_flow().unwrap().as_ref().unwrap()),
            FlowPhaseV1::Paused
        ));

        app.start_setup_flow_cmd();
        let resumed = load_flow().unwrap().unwrap();
        assert!(matches!(phase_of(&resumed), FlowPhaseV1::Running));
        assert_eq!(resumed.flow_id, first_id, "resume keeps the same flow");

        app.stop_setup_flow_cmd();
        assert!(matches!(
            phase_of(load_flow().unwrap().as_ref().unwrap()),
            FlowPhaseV1::Halted { reason, .. }
                if *reason == HaltReasonV1::StoppedByOperator
        ));

        app.start_setup_flow_cmd();
        let fresh = load_flow().unwrap().unwrap();
        assert_ne!(
            fresh.flow_id, first_id,
            "restarting from a stop begins anew"
        );
        assert!(matches!(phase_of(&fresh), FlowPhaseV1::Running));
        assert!(fresh.skipped.is_empty());
    });
}

/// Declines are durable operator intent: the declined operation joins the
/// skip list (surviving reloads) and a matching consent hold releases back
/// to running. Unrelated declines still record suppression and never touch
/// someone else's outstanding hold.
#[test]
fn decline_records_a_durable_skip_and_releases_matching_holds() {
    let home = tempfile::tempdir().unwrap();
    with_connected_app(home.path(), |app| {
        let mut consent = SetupFlowStateV1::start_new("flow-consent1".to_owned(), NOW).unwrap();
        consent.advance(NOW, |f| {
            f.phase = FlowPhaseV1::WaitingForConsent {
                task_id: "setup:computer:qemu".to_owned(),
            };
            true
        });
        insert_flow(consent).unwrap();

        app.decline_setup_operation_cmd("setup:computer:qemu");

        let released = load_flow().unwrap().expect("decline persists");
        assert!(matches!(phase_of(&released), FlowPhaseV1::Running));
        assert_eq!(released.skipped, vec!["setup:computer:qemu".to_owned()]);

        // A hold on a DIFFERENT task is left standing; the decline still
        // records suppression for the id it was issued for.
        update_flow(|f| {
            f.phase = FlowPhaseV1::WaitingForConsent {
                task_id: "setup:cli:claude:install".to_owned(),
            };
            true
        })
        .unwrap();
        app.decline_setup_operation_cmd("setup:computer:qemu");
        let untouched = load_flow().unwrap().unwrap();
        assert!(matches!(
            phase_of(&untouched),
            FlowPhaseV1::WaitingForConsent { task_id }
                if task_id == "setup:cli:claude:install"
        ));
        assert_eq!(untouched.skipped, vec!["setup:computer:qemu".to_owned()]);

        // Duplicate declines do not pile up.
        app.decline_setup_operation_cmd("setup:computer:qemu");
        assert_eq!(
            load_flow().unwrap().unwrap().skipped,
            vec!["setup:computer:qemu".to_owned()]
        );
    });
}
