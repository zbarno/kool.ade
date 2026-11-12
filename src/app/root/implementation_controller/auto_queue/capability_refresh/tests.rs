use std::sync::mpsc::{self, Sender};

#[path = "tests/failure.rs"]
mod failure;

struct ExactResumeHarness(Sender<String>);

impl crate::harness::AiHarness for ExactResumeHarness {
    fn label(&self) -> String {
        "synthetic exact-resume worker".into()
    }

    fn check_available(&self) -> Result<String, crate::AppError> {
        Ok(self.label())
    }

    fn execute(
        &self,
        request: &crate::harness::PlanningRequest,
    ) -> Result<crate::harness::HarnessOutcome, crate::AppError> {
        self.0
            .send(request.task_id.clone().expect("implementation task UID"))
            .expect("test awaits the resumed task");
        Err(crate::AppError::Other(
            "synthetic stop after exact ticket admission".into(),
        ))
    }
}

fn ready_snapshot() -> crate::execution::contract::HarnessCapabilitySnapshotV1 {
    use crate::execution::contract::{
        HarnessCapabilitySnapshotV1, HarnessRole, ModeReadinessV1, ReadinessState, RoleReadinessV1,
    };
    let provider = "pi";
    HarnessCapabilitySnapshotV1 {
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
    }
}

#[test]
// Production boundary: durable Auto-queue encoding/rebinding, capability-refresh
// admission, and ImplementationController dispatch. This checks that fresh proof
// for the unchanged route admits the same ticket once after restart and the
// worker receives its original task UID. A reducer-only test cannot prove queue
// reload or controller dispatch. Synthetic provider evidence and worker only;
// local Git/private files, no Docker or network; moderate setup.
fn fresh_capability_proof_resumes_the_exact_persisted_ticket_after_restart() {
    use crate::{
        app::root::{Screen, board_tests},
        core::implementation::Failure,
        execution::contract::HarnessRole,
        persistence::harness_settings::{
            DetectedHarness, HarnessSettings, IMPLEMENTATION, WorkRoute,
        },
    };

    let _shield = crate::core::gitops::test_support::shield("capability-refresh-restart");
    let app = board_tests::fixture();
    let (mut docs, repo_root, planning_store, code_root) = match &app.screen {
        Screen::Connected(project) => (
            project.task_documents.clone(),
            project.state.planning_store.root.clone(),
            project.state.planning_store.clone(),
            project.state.repo_root.clone(),
        ),
        _ => panic!("fixture should be connected"),
    };
    let ticket = docs[0].path.clone();
    let source_sha = String::from_utf8(
        std::process::Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(&code_root)
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap()
    .trim()
    .to_owned();
    let targets = crate::core::workflow::BranchTargets {
        source: "main".into(),
        destination: "main".into(),
    };
    let intent = crate::core::git_intent::GitIntentV1::approve(
        "root".into(),
        targets.source.clone(),
        source_sha.clone(),
        targets.destination.clone(),
        crate::core::git_intent::PublicationModeV1::HoldForReview,
        None,
    )
    .unwrap();
    let metadata = docs[0]
        .metadata
        .as_ref()
        .unwrap()
        .clone()
        .with_branch_targets(Some(&targets))
        .unwrap()
        .with_source_sha(Some(&source_sha))
        .unwrap()
        .with_git_intent(Some(&intent))
        .unwrap();
    docs[0].metadata = Some(metadata.clone());
    let mut task_markdown = String::from_utf8(planning_store.read(&ticket).unwrap()).unwrap();
    let identity = serde_json::to_string(docs[0].identity.as_ref().unwrap()).unwrap();
    task_markdown.push_str(&format!("\n<!-- koolade-artifact-id:v1 {identity} -->\n"));
    task_markdown = crate::artifacts::task_docs::embed(&task_markdown, &metadata).unwrap();
    planning_store
        .atomic_write(&ticket, task_markdown.as_bytes())
        .unwrap();
    let observed_at = 1;
    let binding = crate::harness::CapabilityRefreshBindingV1 {
        provider_id: "pi".into(),
        role: HarnessRole::Implementer,
        work_category: IMPLEMENTATION.into(),
        selected_model: "pinned-model".into(),
        requested_model: Some("pinned-model".into()),
        observed_at_unix_ms: observed_at,
    };
    let now = crate::persistence::harness_settings::current_unix_ms();
    let settings = HarnessSettings {
        work_routes: std::collections::BTreeMap::from([(
            IMPLEMENTATION.into(),
            WorkRoute {
                harness: "pi".into(),
                model: Some("pinned-model".into()),
            },
        )]),
        discovered: std::collections::BTreeMap::from([(
            "pi".into(),
            DetectedHarness {
                verified_at_unix_ms: Some(now),
                status: "pi ready".into(),
                version: Some("1.0.0".into()),
                executable: Some("/synthetic/bin/pi".into()),
                diagnostic: None,
                ready: true,
                models: vec!["pinned-model".into()],
                default_model: Some("pinned-model".into()),
                model_catalog_fresh: true,
                configuration_required: false,
                implementation_available: true,
                checked_at_unix_ms: now,
                capability_snapshot: Some(ready_snapshot()),
            },
        )]),
        ..HarnessSettings::default()
    };
    let settings_path = std::env::temp_dir().join(format!(
        "capability-refresh-restart-{}.json",
        uuid::Uuid::new_v4()
    ));

    crate::persistence::harness_settings::with_settings_path(settings_path.clone(), || {
        crate::persistence::harness_settings::save(&settings).unwrap();
        let mut app = app;
        let Screen::Connected(project) = &mut app.screen else {
            unreachable!()
        };
        project.queue.auto_build = true;
        project.queue.blocked.insert(
            ticket.clone(),
            Failure::awaiting_capability_refresh("refreshing selected proof", binding),
        );
        project
            .queue
            .blocked
            .get_mut(&ticket)
            .unwrap()
            .retry_after_unix_ms = None;
        project
            .queue
            .bind_task_documents(&project.task_documents)
            .unwrap();
        project.queue.save(&repo_root).unwrap();

        let mut restarted = board_tests::restart_fixture(app);
        let Screen::Connected(project) = &mut restarted.screen else {
            unreachable!()
        };
        project.task_documents = docs.clone();
        project.queue = crate::core::implementation_queue::Queue::load(&repo_root).unwrap();
        project
            .queue
            .bind_task_documents(&project.task_documents)
            .unwrap();

        let task_uid = docs[0].identity.as_ref().unwrap().uid.clone();
        let (attempt_tx, attempt_rx) = mpsc::channel();
        let _runtime = crate::harness::runtime_capabilities::RuntimeCapabilities::override_for_test(
            crate::harness::runtime_capabilities::RuntimeCapabilities {
                planning_access:
                    crate::harness::runtime_capabilities::PlanningAccess::SandboxedRepositoryReads,
                implementation: true,
            },
        );
        restarted.task_harness = Some(Box::new(ExactResumeHarness(attempt_tx)));
        restarted.advance_auto_queue();
        let Screen::Connected(project) = &restarted.screen else {
            unreachable!()
        };
        assert!(
            project.active_implementations.contains_key(&ticket),
            "Auto queue failed to dispatch the exact refreshed ticket: running={}, error={:?}, blocked={:?}, states={:?}",
            project.queue.running,
            project.queue.last_error,
            project.queue.blocked,
            project
                .implementation_states
                .iter()
                .map(|(ticket, state)| (ticket, state.status))
                .collect::<Vec<_>>()
        );
        let resumed_ticket = match attempt_rx.recv_timeout(std::time::Duration::from_secs(30)) {
            Ok(ticket_uid) => ticket_uid,
            Err(error) => {
                crate::app::root::tick::poll_implementations_for_test(&mut restarted);
                let Screen::Connected(project) = &restarted.screen else {
                    unreachable!()
                };
                panic!(
                    "resumed worker did not receive the task UID ({error}); queue error: {}; states: {:?}",
                    project.queue.last_error,
                    project
                        .implementation_states
                        .iter()
                        .map(|(ticket, state)| (ticket, state.status, &state.detail))
                        .collect::<Vec<_>>()
                );
            }
        };
        assert_eq!(resumed_ticket, task_uid);
        assert!(project.queue.running);
        assert!(!project.queue.blocked.contains_key(&ticket));
        assert!(project.active_implementations.contains_key(&ticket));
        assert_eq!(project.queue.recovery_attempts[&ticket], 1);
        assert_eq!(
            project
                .queue
                .in_flight
                .iter()
                .filter(|path| *path == &ticket)
                .count(),
            1,
            "only the selected ticket is admitted for implementation"
        );
        let mut restored = crate::core::implementation_queue::Queue::load(&repo_root).unwrap();
        restored.bind_task_documents(&docs).unwrap();
        assert_eq!(
            restored.recovery_attempts[&ticket], 1,
            "the exact ticket must remain durably admitted for its resumed attempt"
        );
    });
    let _ = std::fs::remove_file(settings_path);
}
