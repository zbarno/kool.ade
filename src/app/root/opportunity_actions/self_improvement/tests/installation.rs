use super::*;
use crate::core::self_improvement::{SelfImprovementEventV1 as Event, SelfImprovementStore};

/// Production boundary: drive Pursue, the app's installation-confirmation
/// command, GitHub profile lookup, and durable lifecycle reload. Store tests
/// cannot prove the UI composes those boundaries correctly. Dependencies are
/// headless egui, a guarded local `gh` executable, and temporary records; no
/// network or Docker. Setup cost is one fixture app and short local writes.
#[cfg(unix)]
#[test]
fn authenticated_installation_confirmation_survives_store_reload() {
    use std::os::unix::fs::PermissionsExt;

    const MERGED_SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    let _identity = crate::build_identity::override_for_test("0.1.0-test", MERGED_SHA);
    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    let temp = tempfile::tempdir().unwrap();
    let bin = temp.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let gh = bin.join("gh");
    std::fs::write(&gh, "#!/bin/sh\nprintf '987654321\\n'\n").unwrap();
    std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o700)).unwrap();
    let original_path = std::env::var_os("PATH").unwrap_or_default();
    let path = std::iter::once(bin.clone())
        .chain(std::env::split_paths(&original_path))
        .collect::<Vec<_>>();
    environment.set("PATH", std::env::join_paths(path).unwrap());

    let mut app = crate::app::root::board_tests::fixture();
    let opportunity_uid = {
        let crate::app::root::Screen::Connected(project) = &mut app.screen else {
            panic!("fixture should be connected")
        };
        app_product_proposal(project)
    };
    let ctx = egui::Context::default();
    click_text(&mut app, &ctx, "Improvement proposals (1)");
    click_text(&mut app, &ctx, "Improve planning reliability");
    click_text(&mut app, &ctx, "Pursue");

    let crate::app::root::Screen::Connected(project) = &mut app.screen else {
        panic!("fixture should remain connected")
    };
    let planning = &project.state.planning_store;
    let opportunities = crate::core::opportunity::OpportunityStore::new(planning);
    let decision = opportunities
        .decision_for_opportunity(&opportunity_uid)
        .unwrap()
        .unwrap();
    let run_uid = SelfImprovementStore::run_uid_for_decision(&decision.uid).unwrap();
    let runs = SelfImprovementStore::new(planning);
    let mut run = runs.load(&run_uid).unwrap();
    let task_uid = uuid::Uuid::new_v4().hyphenated().to_string();
    let implementation_hash = "b".repeat(64);
    let record = |run: &mut crate::core::self_improvement::SelfImprovementRunV1, event: Event| {
        *run = runs
            .apply_event(
                &run.run_uid,
                run.revision,
                &uuid::Uuid::new_v4().hyphenated().to_string(),
                event,
            )
            .unwrap();
    };
    record(
        &mut run,
        Event::PlanApproved {
            approval_sha256: "c".repeat(64),
            capability_id: Some("self-improvement-lifecycle".into()),
            review_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
            source_ref: "main".into(),
            destination_ref: "main".into(),
            task_uids: vec![task_uid.clone()],
            owner_task_uid: task_uid.clone(),
            parent_task_uids: vec![task_uid],
        },
    );
    record(
        &mut run,
        Event::CloneAllocated {
            clone_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
        },
    );
    record(
        &mut run,
        Event::ImplementationFinished {
            head_sha: "d".repeat(40),
            subject_sha256: implementation_hash.clone(),
        },
    );
    record(
        &mut run,
        Event::CodeReviewPassed {
            receipt_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
            subject_sha256: implementation_hash.clone(),
        },
    );
    record(
        &mut run,
        Event::QaPassed {
            receipt_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
            subject_sha256: implementation_hash.clone(),
        },
    );
    record(
        &mut run,
        Event::SpecReconciled {
            receipt_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
            subject_sha256: implementation_hash,
        },
    );
    record(
        &mut run,
        Event::PrPrepared {
            work_item_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
        },
    );
    record(
        &mut run,
        Event::PrOpened {
            url: "https://github.com/zbarno/kool.ade/pull/999999".into(),
        },
    );
    record(
        &mut run,
        Event::Merged {
            sha: MERGED_SHA.into(),
        },
    );
    let expected_revision = run.revision;
    let run_uid = run.run_uid.clone();
    let store_identity = (planning.project_id, planning.root.clone(), planning.mode);
    app.dispatch_ui_command(
        crate::ui::ApplicationCommand::ConfirmSelfImprovementInstallation {
            run_uid: run_uid.clone(),
            expected_revision,
        },
    );

    assert!(
        app.toasts
            .last_message()
            .is_some_and(|message| { message.contains("Installation confirmation recorded") })
    );
    let reopened = crate::artifacts::planning_store::PlanningStore::new(
        store_identity.0,
        store_identity.1,
        store_identity.2,
    );
    let reopened_runs = SelfImprovementStore::new(&reopened);
    let installed = reopened_runs.load(&run_uid).unwrap();
    assert_eq!(
        installed.phase,
        crate::core::self_improvement::SelfImprovementPhaseV1::InstalledNotActivated
    );
    assert_eq!(installed.schema_version, 4);
    let installation = reopened_runs
        .transitions(&run_uid)
        .unwrap()
        .into_iter()
        .find(|transition| {
            transition.to_phase
                == crate::core::self_improvement::SelfImprovementPhaseV1::InstalledNotActivated
        })
        .unwrap();
    assert!(matches!(
        installation.event,
        Some(Event::Installed {
            operator_id,
            github_user_id: Some(github_user_id),
            build_commit_sha,
            ..
        }) if operator_id == "Koolade Acceptance"
            && github_user_id == "987654321"
            && build_commit_sha == MERGED_SHA
    ));
}
