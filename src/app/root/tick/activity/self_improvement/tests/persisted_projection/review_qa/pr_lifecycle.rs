use super::super::{assert_app_idle, synchronize_app_run};
use super::*;

#[path = "pr_lifecycle/authorization_race.rs"]
mod authorization_race;
pub(super) use authorization_race::complete_spec_racing_pr_authorization;
#[path = "pr_lifecycle/reordered_manager.rs"]
mod reordered_manager;

/// Production boundary: the app scheduler reads persisted PR authorization and
/// status together with implementation evidence before advancing Evolve state.
/// Repeated ordinary activity passes must not duplicate the receipt or projection.
/// PR values are synthetic (the provider adapter is covered separately); no
/// network, Docker, model, or browser. Setup cost is the existing headless app fixture.
pub(super) fn verify_persisted_pr_state_projection(
    app: &mut crate::app::root::KooladeApp,
    lifecycle: &crate::core::lifecycle::LifecycleStore<'_>,
    runs: &crate::core::self_improvement::SelfImprovementStore<'_>,
    run_uid: &str,
    task_uid: &str,
    work_uid: &str,
) {
    use crate::core::{
        implementation::PullRequestDetails,
        lifecycle::{
            GateActor, GateDecision, GateReceipt, PullRequestPreparationStatus as PrStatus,
            PullRequestPublicationState,
        },
    };

    let initial = lifecycle.load_pr_preparation(work_uid).unwrap();
    let task = lifecycle.load_task(task_uid).unwrap();
    let authorization = initial
        .publication_authorization
        .clone()
        .unwrap_or_else(|| GateReceipt {
            receipt_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
            subject_uid: work_uid.into(),
            subject_sha256: task.implementation_sha256.clone().unwrap(),
            actor: GateActor::User,
            decision: GateDecision::Approved,
            recorded_at_unix_ms: 1,
            evidence_refs: vec![format!("user-action:create-pr:{work_uid}")],
            prompt_sha256: None,
            prompt_manifest: None,
            reviewer_run_id: None,
            author_run_id: None,
            reviewer_session_id: None,
            author_session_id: None,
        });
    let publishing = if initial.status == PrStatus::Publishing {
        initial
    } else {
        assert_eq!(initial.status, PrStatus::AwaitingAuthorization);
        lifecycle
            .authorize_pr_publication(
                work_uid,
                initial.revision,
                &uuid::Uuid::new_v4().to_string(),
                &authorization,
            )
            .unwrap()
    };
    assert_eq!(publishing.status, PrStatus::Publishing);
    let approved_run = runs.load(run_uid).unwrap();
    if let crate::app::root::Screen::Connected(project) = &app.screen {
        let state = project
            .implementation_states
            .values()
            .find(|state| state.task_uid.as_deref() == Some(task_uid))
            .expect("implementation state exists before restart");
        let state_dir = crate::core::implementation::state_dir_for_task(
            &project.state.planning_store.root,
            &state.ticket,
            state.task_uid.as_deref(),
        )
        .unwrap();
        crate::core::implementation::save(&state_dir, state).unwrap();
    }
    let old_app = std::mem::take(app);
    *app = crate::app::root::board_tests::restart_fixture(old_app);
    let crate::app::root::Screen::Connected(project) = &mut app.screen else {
        panic!("restarted fixture should remain connected")
    };
    project.refresh_implementations();
    app.advance_activity();
    let first_tick = runs.load(run_uid).unwrap();
    assert_eq!(first_tick.phase, approved_run.phase);
    if let crate::app::root::Screen::Connected(project) = &mut app.screen {
        project.activity.last_self_improvement_sync = None;
    }
    app.advance_activity();
    let after_ticks = runs.load(run_uid).unwrap();
    assert_eq!(after_ticks.phase, first_tick.phase);
    assert_eq!(after_ticks.phase, approved_run.phase);
    assert_eq!(
        after_ticks
            .parent_task_progress
            .iter()
            .find(|progress| progress.task_uid == task_uid)
            .and_then(|progress| progress.pr_status),
        Some(crate::core::lifecycle::PullRequestPreparationStatus::Publishing),
        "the app scheduler projects the persisted publication authorization"
    );
    if let crate::app::root::Screen::Connected(project) = &mut app.screen {
        project.activity.last_self_improvement_sync = None;
    }
    app.advance_activity();
    let stable_projection = runs.load(run_uid).unwrap();
    assert_eq!(
        stable_projection.revision, after_ticks.revision,
        "a repeated scheduler pass must not append another run projection event"
    );
    assert_eq!(
        stable_projection, after_ticks,
        "once persisted authorization is projected, later scheduler ticks must not append another projection event"
    );
    assert_eq!(
        lifecycle
            .load_pr_preparation(work_uid)
            .unwrap()
            .publication_authorization
            .as_ref(),
        Some(&authorization),
        "repeated scheduler ticks preserve exactly the typed user approval"
    );

    let (head_sha, base_sha) = {
        let crate::app::root::Screen::Connected(project) = &app.screen else {
            panic!("fixture should remain connected")
        };
        let implementation = project
            .implementation_states
            .values()
            .find(|state| state.task_uid.as_deref() == Some(task_uid))
            .expect("implementation state for the completed task");
        (
            implementation.verified_head.clone().expect("verified head"),
            implementation.base_commit.clone(),
        )
    };
    let pr_url = "https://github.com/zbarno/kool.ade/pull/999999";
    let opened = lifecycle
        .record_pr_publication_state(
            work_uid,
            publishing.revision,
            &uuid::Uuid::new_v4().to_string(),
            PullRequestPublicationState {
                status: PrStatus::Opened,
                provider_pr_id: Some(999_999),
                provider_pr_url: Some(pr_url.into()),
                head_sha: Some(head_sha.clone()),
                base_sha: Some(base_sha.clone()),
                merged_commit_sha: None,
                lease_epoch: Some(publishing.publication_lease_epoch()),
            },
        )
        .unwrap();
    let crate::app::root::Screen::Connected(project) = &mut app.screen else {
        panic!("fixture should remain connected")
    };
    let implementation = project
        .implementation_states
        .values_mut()
        .find(|state| state.task_uid.as_deref() == Some(task_uid))
        .expect("implementation state for the completed task");
    implementation.pr_url = Some(pr_url.into());
    implementation.pr_details = Some(PullRequestDetails {
        number: 999_999,
        head_sha: head_sha.clone(),
        base_sha: Some(base_sha.clone()),
        base_branch: "main".into(),
        merged_at: None,
    });

    synchronize_app_run(app, lifecycle, runs, run_uid).unwrap();
    let projected = runs.load(run_uid).unwrap();
    assert_eq!(projected.phase, Phase::PrOpened);
    assert_eq!(projected.pr_url.as_deref(), Some(pr_url));

    const MERGED_SHA: &str = "ffffffffffffffffffffffffffffffffffffffff";
    let _build_identity = crate::build_identity::override_for_test("0.1.0-test", MERGED_SHA);
    let merged_sha = MERGED_SHA.to_owned();
    lifecycle
        .record_pr_publication_state(
            work_uid,
            opened.revision,
            &uuid::Uuid::new_v4().to_string(),
            PullRequestPublicationState {
                status: PrStatus::Merged,
                provider_pr_id: Some(999_999),
                provider_pr_url: Some(pr_url.into()),
                head_sha: Some(head_sha.clone()),
                base_sha: Some(base_sha),
                merged_commit_sha: Some(merged_sha.clone()),
                lease_epoch: None,
            },
        )
        .unwrap();
    let crate::app::root::Screen::Connected(project) = &mut app.screen else {
        panic!("fixture should remain connected")
    };
    let implementation = project
        .implementation_states
        .values_mut()
        .find(|state| state.task_uid.as_deref() == Some(task_uid))
        .expect("implementation state for the completed task");
    implementation.pr_details.as_mut().unwrap().merged_at = Some("2026-10-21T20:00:00Z".into());
    implementation.merged_commit = Some(merged_sha);
    implementation.pr_state = Some(crate::core::implementation::PullRequestState::Merged);

    for _ in 0..100 {
        synchronize_app_run(app, lifecycle, runs, run_uid).unwrap();
        if runs.load(run_uid).unwrap().phase == Phase::MergedNotActivated {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let projected = runs.load(run_uid).unwrap();
    assert_eq!(projected.phase, Phase::MergedNotActivated);
    assert!(projected.installed_version.is_none());

    app.dispatch_ui_command(
        crate::ui::ApplicationCommand::ConfirmSelfImprovementInstallation {
            run_uid: run_uid.into(),
            expected_revision: projected.revision,
        },
    );
    let installed = runs.load(run_uid).unwrap();
    assert_eq!(installed.phase, Phase::InstalledNotActivated);
    assert_eq!(
        installed.installed_build_commit_sha.as_deref(),
        Some(MERGED_SHA)
    );

    synchronize_app_run(app, lifecycle, runs, run_uid).unwrap();
    let activated = runs.load(run_uid).unwrap();
    assert_eq!(activated.phase, Phase::Activated);
    assert_eq!(activated.activated_version.as_deref(), Some("0.1.0-test"));
    assert_app_idle(app);
    reordered_manager::verify_merged_task_intent_survives_reordered_manager(
        app, lifecycle, task_uid, work_uid,
    );
}
