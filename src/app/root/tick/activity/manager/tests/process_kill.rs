use crate::{
    app::root::{Screen, board_tests},
    domain::{
        ChatMessage, ChatRole, ManagerEventCheckpointV1, ManagerRunEventV1,
        ManagerRunOutcomeV1 as Outcome,
    },
    execution::contract::{
        HarnessCapabilitySnapshotV1, HarnessRole, ModeReadinessV1, ReadinessState, RoleReadinessV1,
    },
    harness::CapabilityRefreshBindingV1,
    persistence::harness_settings::{DetectedHarness, HarnessSettings, MANAGER, WorkRoute},
};
use std::process::{Command, Stdio};

const CHILD_ENV: &str = "KOOLADE_TEST_MANAGER_CRASH_AFTER_START";
const SLUG_ENV: &str = "KOOLADE_TEST_MANAGER_CRASH_SLUG";
const MARKER_ENV: &str = "KOOLADE_TEST_MANAGER_CRASH_MARKER";

fn route_settings() -> HarnessSettings {
    let now = crate::persistence::harness_settings::current_unix_ms();
    let provider = "pi";
    let snapshot = HarnessCapabilitySnapshotV1 {
        schema_version: 1,
        provider_id: provider.into(),
        authentication_mode: crate::execution::contract::current_authentication_mode(provider),
        manager_profile_id: crate::execution::contract::profile_evidence_id_for_current_snapshot(
            provider,
            Some("1.0.0"),
            crate::harness::ExecutionProfile::Manager,
        ),
        worker_profile_id: crate::execution::contract::profile_evidence_id_for_current_snapshot(
            provider,
            Some("1.0.0"),
            crate::harness::ExecutionProfile::Worker,
        ),
        probe_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        probe_cancellation_generation: 1,
        probe_sequence: 1,
        cli_binary: ReadinessState::Ready,
        cli_version: Some("1.0.0".into()),
        authentication: ReadinessState::Ready,
        model_catalog: ReadinessState::Ready,
        scriptable_invocation: ReadinessState::Ready,
        docker_mcp_bridge: ReadinessState::Ready,
        execution_target: ReadinessState::Ready,
        failure: None,
        modes: crate::harness::ExecutionMode::ALL
            .into_iter()
            .map(|mode| ModeReadinessV1 {
                mode,
                available: ReadinessState::Ready,
                failure: None,
            })
            .collect(),
        roles: HarnessRole::ALL
            .into_iter()
            .map(|role| RoleReadinessV1 {
                role,
                available: ReadinessState::Ready,
                failure: None,
            })
            .collect(),
    };
    HarnessSettings {
        default_harness: Some(provider.into()),
        work_routes: std::collections::BTreeMap::from([(
            MANAGER.into(),
            WorkRoute {
                harness: provider.into(),
                model: Some("synthetic-manager".into()),
            },
        )]),
        discovered: std::collections::BTreeMap::from([(
            provider.into(),
            DetectedHarness {
                status: "synthetic provider ready".into(),
                version: Some("1.0.0".into()),
                executable: Some("/synthetic/bin/pi".into()),
                diagnostic: None,
                ready: true,
                models: vec!["synthetic-manager".into()],
                default_model: Some("synthetic-manager".into()),
                model_catalog_fresh: true,
                configuration_required: false,
                implementation_available: true,
                checked_at_unix_ms: now,
                verified_at_unix_ms: None,
                capability_snapshot: Some(snapshot),
            },
        )]),
        ..HarnessSettings::default()
    }
}

struct ReplayedManagerHarness(CapabilityRefreshBindingV1);

impl crate::harness::AiHarness for ReplayedManagerHarness {
    fn label(&self) -> String {
        "synthetic restarted Manager".into()
    }

    fn capability_refresh_binding(&self) -> Option<CapabilityRefreshBindingV1> {
        Some(self.0.clone())
    }

    fn route_provenance(
        &self,
        _: &crate::harness::PlanningRequest,
    ) -> Option<crate::harness::HarnessRouteProvenance> {
        Some(crate::harness::HarnessRouteProvenance {
            provider_id: self.0.provider_id.clone(),
            role: self.0.role,
            work_category: Some(self.0.work_category.clone()),
            selected_model: Some(self.0.selected_model.clone()),
            requested_model: self.0.requested_model.clone(),
            capability_probe_id: Some("synthetic-crash-recovery-probe".into()),
            capability_generation: Some(1),
            authentication_mode: None,
            profile_evidence_id: None,
            cli_version: Some("1.0.0".into()),
            capability_expires_at_unix_ms: None,
        })
    }

    fn check_available(&self) -> Result<String, crate::AppError> {
        Ok(self.label())
    }

    fn execute(
        &self,
        _: &crate::harness::PlanningRequest,
    ) -> Result<crate::harness::HarnessOutcome, crate::AppError> {
        Err(crate::AppError::HarnessTimedOut { secs: 1 })
    }
}

/// Child entry point: persist a started Manager checkpoint, announce durability,
/// and stay alive until the parent kills this process.
///
/// Production boundary: the private Manager event store and app recovery loop
/// must replay the exact event after its owning process dies. In-memory timeout
/// tests cannot prove file durability across process death. Dependencies: a
/// subprocess and local private-store files only; no provider, Docker, SSH, or
/// network. Setup cost is one temporary home and one child test process.
#[test]
fn manager_crash_after_durable_start_replays_checkpoint_on_restart() {
    if std::env::var_os(CHILD_ENV).is_some() {
        let slug = std::env::var(SLUG_ENV).unwrap();
        let binding = CapabilityRefreshBindingV1 {
            provider_id: "pi".into(),
            role: HarnessRole::Manager,
            work_category: MANAGER.into(),
            selected_model: "synthetic-manager".into(),
            requested_model: Some("synthetic-manager".into()),
            observed_at_unix_ms: crate::persistence::harness_settings::current_unix_ms(),
        };
        let checkpoint = ManagerEventCheckpointV1::from_messages(
            &["process-crash event".into()],
            Some(binding),
            None,
            None,
        )
        .unwrap();
        let mut message = ChatMessage::new(ChatRole::System, "Manager check started", None);
        message.manager_run_event = Some(ManagerRunEventV1 {
            run_id: uuid::Uuid::new_v4().hyphenated().to_string(),
            sequence: 1,
            outcome: Outcome::Started,
            checkpoint: Some(checkpoint.clone()),
            checkpoint_cursor: checkpoint.cursor,
            checkpoint_sha256: checkpoint.sha256,
            retry_after_unix_ms: None,
        });
        crate::persistence::manager_activity::append(&slug, &[message]).unwrap();
        std::fs::write(
            std::env::var(MARKER_ENV).unwrap(),
            b"durable-start-record-written",
        )
        .unwrap();
        loop {
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    }

    let home = tempfile::tempdir().unwrap();
    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    let registration = crate::persistence::register_test_home(home.path());
    environment.set("KOOLADE_HOME", home.path());
    environment.set("KOOLADE_TEST_HOME_TOKEN", registration.token());
    crate::persistence::harness_settings::save(&route_settings()).unwrap();
    let slug = format!("manager-crash-{}", uuid::Uuid::new_v4());
    let marker = home.path().join("manager-started.marker");
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "app::root::tick::activity::manager::tests::process_kill::manager_crash_after_durable_start_replays_checkpoint_on_restart",
            "--nocapture",
        ])
        .env(CHILD_ENV, "1")
        .env(SLUG_ENV, &slug)
        .env(MARKER_ENV, &marker)
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !marker.exists() {
        if let Some(status) = child.try_wait().unwrap() {
            panic!("Manager child exited before durable marker: {status}");
        }
        assert!(
            std::time::Instant::now() < deadline,
            "Manager child did not persist its start event"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    child.kill().unwrap();
    assert!(!child.wait().unwrap().success(), "child must be terminated");

    let before_restart = crate::persistence::manager_activity::load(&slug);
    assert_eq!(before_restart.len(), 1);
    assert_eq!(
        before_restart[0]
            .manager_run_event
            .as_ref()
            .unwrap()
            .outcome,
        Outcome::Started
    );
    let mut app = board_tests::fixture();
    let Screen::Connected(project) = &mut app.screen else {
        unreachable!()
    };
    project.chat_slug = slug.clone();
    project.activity.manager_history = before_restart;
    let binding = CapabilityRefreshBindingV1 {
        provider_id: "pi".into(),
        role: HarnessRole::Manager,
        work_category: MANAGER.into(),
        selected_model: "synthetic-manager".into(),
        requested_model: Some("synthetic-manager".into()),
        observed_at_unix_ms: crate::persistence::harness_settings::current_unix_ms(),
    };
    let mut harness =
        Some(Box::new(ReplayedManagerHarness(binding)) as Box<dyn crate::harness::AiHarness>);
    super::super::advance(project, &mut harness);
    let restarted_manager = project
        .activity
        .manager
        .as_ref()
        .expect("app recovery dispatches the requeued checkpoint to Manager");
    assert_eq!(restarted_manager.source_events(), ["process-crash event"]);
    assert!(restarted_manager.run_event_saved());
    let after_restart = crate::persistence::manager_activity::load(&slug);
    let outcomes = after_restart
        .iter()
        .filter_map(|message| message.manager_run_event.as_ref())
        .map(|event| event.outcome)
        .collect::<Vec<_>>();
    assert_eq!(
        outcomes,
        [Outcome::Started, Outcome::Retried, Outcome::Started]
    );
    assert_eq!(
        after_restart
            .iter()
            .filter(|message| message
                .text
                .contains("replaying the exact saved event batch"))
            .count(),
        1
    );
}
