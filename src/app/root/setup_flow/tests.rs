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

/// Production boundary: the machine-local setup pump reloads a verified
/// prerequisite from the setup-task store and resumes the setup journey without
/// a manual Resume command. Driver tests cannot prove app persistence and pump
/// wiring. Uses local setup-task files and a connected app fixture only.
#[test]
fn verified_required_setup_resumes_the_persisted_flow_automatically() {
    use crate::persistence::setup_tasks::{
        SetupTaskEventV1, SetupTaskStateV1, SetupTaskStoreV1, SetupTaskV1, UserActionReasonV1,
        VerifiedResultV1,
    };

    let home = tempfile::tempdir().unwrap();
    with_connected_app(home.path(), |app| {
        let id = "setup:computer:kvm";
        let mut flow = SetupFlowStateV1::start_new("flow-required-resume".into(), NOW).unwrap();
        flow.last_scan_generation = 2;
        flow.phase = FlowPhaseV1::WaitingForUser {
            task_id: id.into(),
            reason: UserActionReasonV1::OperatingSystemPrompt,
        };
        let task = SetupTaskV1 {
            schema_version: 1,
            id: id.into(),
            title: "Enable virtualization".into(),
            explanation: "A required runtime needs hardware virtualization.".into(),
            next_step: "Enable virtualization in system settings, then check again.".into(),
            dependencies: Vec::new(),
            blocked_by: Vec::new(),
            state: SetupTaskStateV1::Verified,
            created_at_unix_ms: NOW,
            observed_at_unix_ms: NOW,
            scan_generation: 2,
            verified_at_unix_ms: Some(NOW),
            verified_result: Some(VerifiedResultV1 {
                evidence_domain: "computer:kvm".into(),
                scan_generation: 2,
                verified_at_unix_ms: NOW,
            }),
            attempts: Vec::new(),
            events: vec![SetupTaskEventV1 {
                observed_at_unix_ms: NOW,
                scan_generation: 2,
                state: SetupTaskStateV1::Verified,
            }],
            superseded_by: Vec::new(),
        };
        let store = SetupTaskStoreV1 {
            latest_scan_generation: 2,
            tasks: [(id.into(), task)].into_iter().collect(),
            setup_flow: Some(flow.clone()),
            ..SetupTaskStoreV1::default()
        };
        store.validate().unwrap();
        let path = crate::persistence::setup_tasks::machine_setup_state_dir()
            .unwrap()
            .join("tasks.json");
        crate::persistence::setup_tasks::save_to_for_test(&path, &store).unwrap();
        app.setup_flow_boot_synced = true;
        app.machine_setup_scan_current = true;
        app.machine_setup_tasks = crate::persistence::setup_tasks::load()
            .unwrap()
            .tasks
            .into_values()
            .collect();

        app.last_setup_flow_beat = None;
        app.pump_setup_flow();
        let resumed = load_flow().unwrap().expect("verified task resumes flow");
        assert_eq!(resumed.flow_id, flow.flow_id);
        assert!(matches!(phase_of(&resumed), FlowPhaseV1::Running));
        assert!(resumed.seen_verified.iter().any(|seen| seen == id));

        app.last_setup_flow_beat = None;
        app.pump_setup_flow();
        assert!(matches!(
            phase_of(load_flow().unwrap().as_ref().unwrap()),
            FlowPhaseV1::Finishing
        ));
        app.last_setup_flow_beat = None;
        app.pump_setup_flow();
        assert!(matches!(
            phase_of(load_flow().unwrap().as_ref().unwrap()),
            FlowPhaseV1::Complete(crate::persistence::setup_tasks::DoneOutcomeV1::FullyPrepared)
        ));
    });
}
