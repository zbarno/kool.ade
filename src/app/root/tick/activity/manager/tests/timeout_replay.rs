use crate::harness::{AiHarness, HarnessOutcome, PlanningRequest};
use std::time::{Duration, Instant};

struct TimedOutHarness(crate::harness::HarnessRouteProvenance);

impl AiHarness for TimedOutHarness {
    fn label(&self) -> String {
        "synthetic-timeout-provider".into()
    }

    fn check_available(&self) -> Result<String, crate::error::AppError> {
        Ok("synthetic provider ready".into())
    }

    fn route_provenance(
        &self,
        _request: &PlanningRequest,
    ) -> Option<crate::harness::HarnessRouteProvenance> {
        Some(self.0.clone())
    }

    fn execute(
        &self,
        _request: &PlanningRequest,
    ) -> Result<HarnessOutcome, crate::error::AppError> {
        Err(crate::error::AppError::HarnessTimedOut { secs: 1 })
    }
}

/// Production boundary: asynchronous supervised Manager invocation, activity
/// outcome persistence, and the saved-event retry path. A classifier-only test
/// cannot prove the timeout bit reaches the durable event or that the original
/// checkpoint is requeued. Dependencies: local Git/private-store fixture and a
/// synthetic harness; no provider, Docker, or network service. Setup cost is low.
#[test]
fn manager_timeout_is_persisted_and_requeues_the_same_checkpoint_once() {
    use crate::{
        app::{
            manager::Manager,
            root::{Screen, board_tests},
        },
        domain::{ManagerEventCheckpointV1, ManagerRunEventV1, ManagerRunOutcomeV1},
        execution::contract::HarnessRole,
        harness::{CapabilityRefreshBindingV1, HarnessRouteProvenance},
    };

    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    let home = tempfile::tempdir().unwrap();
    environment.set("KOOLADE_HOME", home.path());
    let mut app = board_tests::fixture();
    let Screen::Connected(project) = &mut app.screen else {
        unreachable!()
    };
    let source_events = vec!["Review the saved TaskIntent state".to_owned()];
    let binding = CapabilityRefreshBindingV1 {
        provider_id: "pi".into(),
        role: HarnessRole::Manager,
        work_category: "manager".into(),
        selected_model: "manager-model".into(),
        requested_model: Some("manager-model".into()),
        observed_at_unix_ms: 1,
    };
    let checkpoint =
        ManagerEventCheckpointV1::from_messages(&source_events, Some(binding.clone()), None, None)
            .unwrap();
    let run_id = uuid::Uuid::new_v4().hyphenated().to_string();
    project
        .persist_manager_run_event(
            ManagerRunEventV1 {
                run_id: run_id.clone(),
                sequence: 1,
                outcome: ManagerRunOutcomeV1::Started,
                checkpoint: Some(checkpoint.clone()),
                checkpoint_cursor: checkpoint.cursor,
                checkpoint_sha256: checkpoint.sha256.clone(),
                retry_after_unix_ms: None,
            },
            "Manager check started.",
        )
        .unwrap();
    project.activity.last_update = Some(Instant::now());
    let harness = TimedOutHarness(HarnessRouteProvenance {
        provider_id: "pi".into(),
        role: HarnessRole::Manager,
        work_category: Some("manager".into()),
        selected_model: Some("manager-model".into()),
        requested_model: Some("manager-model".into()),
        capability_probe_id: Some("synthetic-probe".into()),
        capability_generation: Some(1),
        authentication_mode: None,
        profile_evidence_id: None,
        cli_version: Some("1.0.0".into()),
        capability_expires_at_unix_ms: None,
    });
    project.activity.manager = Some(Manager::start_supervised(
        project,
        &source_events,
        None,
        Box::new(harness),
        run_id.clone(),
        checkpoint.clone(),
    ));

    let deadline = Instant::now() + Duration::from_secs(15);
    while project.activity.manager.is_some() {
        super::advance(project, &mut None);
        assert!(
            Instant::now() < deadline,
            "timed-out Manager result was not collected"
        );
        std::thread::sleep(Duration::from_millis(5));
    }

    let events = crate::persistence::manager_activity::load(&project.chat_slug)
        .into_iter()
        .filter_map(|message| message.manager_run_event)
        .filter(|event| event.run_id == run_id)
        .collect::<Vec<_>>();
    assert_eq!(
        events.iter().map(|event| event.outcome).collect::<Vec<_>>(),
        vec![
            ManagerRunOutcomeV1::Started,
            ManagerRunOutcomeV1::TimedOut,
            ManagerRunOutcomeV1::Retried,
        ]
    );
    assert!(events[0].checkpoint.as_ref() == Some(&checkpoint));
    assert_eq!(events[1].checkpoint_sha256, checkpoint.sha256);
    assert_eq!(events[2].checkpoint_cursor, checkpoint.cursor);
    assert!(project.activity.pending.contains(&source_events[0]));
}
