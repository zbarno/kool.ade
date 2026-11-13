struct CheckpointResumeHarness(
    crate::harness::CapabilityRefreshBindingV1,
    std::sync::mpsc::Sender<String>,
);

impl crate::harness::AiHarness for CheckpointResumeHarness {
    fn label(&self) -> String {
        "synthetic exact-route Manager resume".into()
    }

    fn check_available(&self) -> Result<String, crate::AppError> {
        Ok("synthetic selected route ready".into())
    }

    fn capability_refresh_binding(&self) -> Option<crate::harness::CapabilityRefreshBindingV1> {
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
            capability_probe_id: Some("synthetic-refreshed-pi-proof".into()),
            capability_generation: Some(1),
            authentication_mode: None,
            profile_evidence_id: None,
            cli_version: Some("1.2.3".into()),
            capability_expires_at_unix_ms: Some(u64::MAX),
        })
    }

    fn execute(
        &self,
        request: &crate::harness::PlanningRequest,
    ) -> Result<crate::harness::HarnessOutcome, crate::AppError> {
        self.1
            .send(request.prompt_body.clone())
            .expect("test observes the resumed Manager request");
        Ok(crate::harness::HarnessOutcome {
            final_text: r#"{"summary":"Recovered the exact queued Manager checkpoint.","what_exists":"Synthetic test fixture.","how_known":["workspace-summary"],"uncertainty":"No external provider was used.","next_action":"Continue eligible work.","conflicts":[],"summary_evidence":[],"proposed_actions":[],"opportunities":[]}"#.into(),
            envelope: None,
            stderr_tail: String::new(),
        })
    }
}

/// Production boundary: stale-route admission, automatic background discovery,
/// durable Manager checkpoint replay, and completion persistence. Reducer tests
/// cannot prove that an event stays queued across proof refresh and is then
/// delivered once to a worker bound to its original route. Uses a fake local Pi
/// discovery CLI, a synthetic route-bound worker, and temporary app/settings
/// stores; no account, Docker, or network.
#[test]
fn queued_manager_checkpoint_replays_and_completes_after_automatic_proof_refresh() {
    use crate::{
        app::{root::Screen, root::board_tests},
        domain::ManagerRunOutcomeV1 as Outcome,
        persistence::harness_settings::{DetectedHarness, HarnessSettings, MANAGER, WorkRoute},
    };
    use std::{os::unix::fs::PermissionsExt, time::Instant};

    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let agent = home.join(".pi/agent");
    std::fs::create_dir_all(&agent).unwrap();
    std::fs::write(
        agent.join("settings.json"),
        r#"{"defaultProvider":"synthetic","defaultModel":"manager-model"}"#,
    )
    .unwrap();
    std::fs::write(
        agent.join("models.json"),
        r#"{"providers":{"synthetic":{"models":[{"id":"manager-model"}]}}}"#,
    )
    .unwrap();
    let pi = root.path().join("pi");
    std::fs::write(
        &pi,
        r##"#!/bin/sh
case "$1" in
--version) echo 'pi 1.2.3'; exit 0;;
--help) printf '%s\n' --print --mode --no-session --no-approve --append-system-prompt --thinking --provider --model --no-extensions --no-skills --no-prompt-templates --no-themes --no-context-files --no-tools --no-builtin-tools --tools --extension json xhigh; exit 0;;
auth) echo '{"status":"ready","provider":"synthetic","authType":"api_key"}'; exit 0;;
*) echo 'Pi execution is outside this discovery-only fixture' >&2; exit 3;;
esac
exit 3
"##,
    )
    .unwrap();
    std::fs::set_permissions(&pi, std::fs::Permissions::from_mode(0o755)).unwrap();

    let settings_path = root.path().join("harness-settings.json");
    let settings = HarnessSettings {
        default_harness: Some("pi".into()),
        work_routes: std::collections::BTreeMap::from([(
            MANAGER.into(),
            WorkRoute {
                harness: "pi".into(),
                model: Some("manager-model".into()),
            },
        )]),
        discovered: std::collections::BTreeMap::from([(
            "pi".into(),
            DetectedHarness {
                status: "pi ready".into(),
                version: Some("1.2.2".into()),
                executable: Some(pi.display().to_string()),
                diagnostic: None,
                ready: true,
                models: vec!["manager-model".into()],
                default_model: Some("manager-model".into()),
                model_catalog_fresh: false,
                configuration_required: false,
                implementation_available: false,
                capability_snapshot: None,
                checked_at_unix_ms: 1,
                verified_at_unix_ms: None,
            },
        )]),
        ..HarnessSettings::default()
    };
    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    environment.set("KOOLADE_HOME", root.path());
    environment.set("HOME", &home);
    environment.set("PI_CODING_AGENT_DIR", &agent);
    environment.set(crate::harness::pi_harness::PI_BINARY_ENV, &pi);
    environment.set("PATH", "/usr/bin:/bin");

    crate::persistence::harness_settings::with_settings_path(settings_path, || {
        crate::persistence::harness_settings::save(&settings).unwrap();
        let mut app = board_tests::fixture();
        let Screen::Connected(project) = &mut app.screen else {
            unreachable!()
        };
        project.activity.pending = vec!["resume saved TaskIntent checkpoint".into()];
        let stale_route = crate::app::root::configured_harness_for_project(
            &mut None,
            Some(MANAGER),
            &project.state.harness_routes.routes,
        );
        let binding = stale_route
            .capability_refresh_binding()
            .expect("stale selected route retains its replay binding");
        assert_eq!(binding.provider_id, "pi");
        assert_eq!(binding.selected_model, "manager-model");
        assert!(stale_route.route_blocker().is_some());

        super::advance(project, &mut None);
        assert!(project.activity.pending.is_empty());
        let queued = project
            .activity
            .manager_history
            .iter()
            .filter_map(|message| message.manager_run_event.as_ref())
            .find(|event| event.outcome == Outcome::Queued)
            .expect("the exact event batch is durably queued while proof is stale");
        let checkpoint = queued.checkpoint.clone().expect("queued checkpoint");
        assert_eq!(
            checkpoint.events[0].text,
            "resume saved TaskIntent checkpoint"
        );
        assert_eq!(checkpoint.route.as_ref(), Some(&binding));

        let deadline = Instant::now() + std::time::Duration::from_secs(45);
        let (replay_tx, replay_rx) = std::sync::mpsc::channel();
        let mut replay_harness: Option<Box<dyn crate::harness::AiHarness>> = None;
        loop {
            crate::app::dialogs::with_cli_background_sweeps_permitted_for_test(|| {
                crate::app::dialogs::poll_cli_auto_refresh_for_test();
            });
            if replay_harness.is_none()
                && crate::persistence::harness_settings::load()
                    .0
                    .discovered
                    .get("pi")
                    .is_some_and(|detected| {
                        detected.has_current_authenticated_capability(
                            "pi",
                            crate::persistence::harness_settings::current_unix_ms(),
                        )
                    })
            {
                replay_harness = Some(Box::new(CheckpointResumeHarness(
                    binding.clone(),
                    replay_tx.clone(),
                )));
            }
            super::advance(project, &mut replay_harness);
            let outcomes = project
                .activity
                .manager_history
                .iter()
                .filter_map(|message| message.manager_run_event.as_ref())
                .filter(|event| event.checkpoint_sha256 == checkpoint.sha256)
                .map(|event| event.outcome)
                .collect::<Vec<_>>();
            if outcomes.last() == Some(&Outcome::Completed) {
                assert!(outcomes.contains(&Outcome::Retried));
                assert!(outcomes.contains(&Outcome::Started));
                break;
            }
            assert!(
                Instant::now() < deadline,
                "exact Manager checkpoint did not complete after auto refresh; outcomes={outcomes:?}, manager_error={:?}",
                project.activity.last_manager_error
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        let persisted = crate::persistence::manager_activity::load(&project.chat_slug);
        let events = persisted
            .iter()
            .filter_map(|message| message.manager_run_event.as_ref())
            .filter(|event| event.checkpoint_sha256 == checkpoint.sha256)
            .collect::<Vec<_>>();
        assert_eq!(events.last().unwrap().outcome, Outcome::Completed);
        assert!(events.iter().any(|event| {
            event.outcome == Outcome::Started && event.checkpoint.as_ref() == Some(&checkpoint)
        }));
        assert!(persisted.iter().any(|message| {
            message
                .text
                .contains("Recovered the exact queued Manager checkpoint.")
        }));
        let replayed_requests = replay_rx.try_iter().collect::<Vec<_>>();
        assert_eq!(
            replayed_requests.len(),
            1,
            "checkpoint dispatch occurs once"
        );
        assert!(replayed_requests[0].contains("resume saved TaskIntent checkpoint"));
    });
}
