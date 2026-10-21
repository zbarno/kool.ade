use super::*;
use crate::{
    app::root::board_tests::click_text,
    artifacts::planning_store::StoreMode,
    core::opportunity::{
        EvidenceConfidentialityV1, OpportunityDecisionKindV1, OpportunityEvidenceRefV1,
        OpportunityKindV1, OpportunityOriginV1, OpportunitySaveResult, OpportunityScopeV1,
        OpportunityStatusV1, OpportunityTargetV1, OpportunityV1,
    },
    harness::prompt_catalog::{OpportunityChoice, OpportunityDecisionV1},
};

#[path = "tests/installation.rs"]
mod installation;
#[path = "tests/retry_recovery.rs"]
mod retry_recovery;

#[cfg(unix)]
#[test]
fn product_pursuit_records_authenticated_github_user_as_actor() {
    use std::os::unix::fs::PermissionsExt;

    // Production boundary: an explicit product Pursue action must persist the
    // authenticated GitHub account, not a user-editable display name. The
    // store test alone cannot prove the app's identity probe contract. A fake
    // local gh executable only; no network or Docker, negligible setup.
    let directory = tempfile::tempdir().unwrap();
    let gh = directory.path().join("gh");
    std::fs::write(
        &gh,
        "#!/bin/sh\n[ \"$1\" = api ] && [ \"$2\" = user ] || exit 2\nprintf '987654321\\n'\n",
    )
    .unwrap();
    std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o700)).unwrap();

    assert_eq!(
        super::decision_user_id(true, "Editable display name", &gh).unwrap(),
        "github:987654321"
    );
    assert_eq!(super::decision_user_id(false, "Ada", &gh).unwrap(), "Ada");
}

#[cfg(unix)]
#[test]
fn product_pursuit_without_github_auth_is_not_recorded_as_a_decision() {
    use std::os::unix::fs::PermissionsExt;

    let directory = tempfile::tempdir().unwrap();
    let gh = directory.path().join("gh");
    std::fs::write(&gh, "#!/bin/sh\nexit 1\n").unwrap();
    std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o700)).unwrap();

    assert!(super::decision_user_id(true, "Ada", &gh).is_err());
}

#[cfg(unix)]
#[test]
fn app_does_not_record_product_pursuit_when_github_auth_fails() {
    use std::os::unix::fs::PermissionsExt;

    // Production boundary: a failed GitHub identity probe during the actual
    // board Pursue action must leave the proposal undecided and must not start
    // its self-improvement run. The helper unit test cannot prove the UI
    // command stops before persisting the receipt. Uses a local failing `gh`,
    // headless egui, and temporary stores; no network or Docker.
    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    let directory = tempfile::tempdir().unwrap();
    let bin = directory.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let gh = bin.join("gh");
    std::fs::write(&gh, "#!/bin/sh\nexit 1\n").unwrap();
    std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o700)).unwrap();
    let original_path = std::env::var_os("PATH").unwrap_or_default();
    let path = std::iter::once(bin)
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
    let context = egui::Context::default();
    click_text(&mut app, &context, "Improvement proposals (1)");
    click_text(&mut app, &context, "Improve planning reliability");
    click_text(&mut app, &context, "Pursue");

    let crate::app::root::Screen::Connected(project) = &app.screen else {
        panic!("fixture should remain connected")
    };
    let opportunities = OpportunityStore::new(&project.state.planning_store);
    assert!(
        opportunities
            .decision_for_opportunity(&opportunity_uid)
            .unwrap()
            .is_none()
    );
    let (proposal, _) = opportunities.load(&opportunity_uid).unwrap();
    assert_eq!(proposal.status, OpportunityStatusV1::AwaitingDecision);
    assert!(proposal.related_feature_uids.is_empty());
    assert!(app.toasts.last_message().is_some_and(|message| {
        message.contains("Authenticate with GitHub before pursuing a Kool.ad/e self-improvement")
    }));
}

fn app_proposal(
    project: &mut crate::app::session::Project,
    target_kind: OpportunityTargetKindV1,
) -> String {
    let planning = &project.state.planning_store;
    crate::artifacts::migration::bootstrap_product_with_store(
        &project.state.repo_root,
        planning,
        "Synthetic Kool.ad/e fixture",
    )
    .unwrap();
    let workspace_uid = planning.project_id.hyphenated().to_string();
    let mut manifest = crate::core::project_repos::ProjectManifest::load(planning).unwrap();
    let repository = manifest
        .repositories
        .iter_mut()
        .find(|repository| repository.id == "root")
        .unwrap();
    repository.remote = "https://github.com/zbarno/kool.ade".into();
    project.state.repositories = manifest.clone();
    planning
        .transaction(
            &[(
                crate::artifacts::planning_store::paths::PROJECT_MANIFEST.to_owned(),
                serde_json::to_vec(&manifest).unwrap(),
            )],
            None,
        )
        .unwrap();
    project.state.identity = crate::domain::user::ResolvedIdentity {
        user: crate::domain::user::CurrentUser::new("Koolade Acceptance", Vec::new()),
        source: crate::domain::user::IdentitySource::ConfigBlock,
    };
    project.state.baseline_planning_revision = planning.revision().unwrap();

    let output = std::process::Command::new("git")
        .args([
            "-C",
            project.state.repo_root.to_str().unwrap(),
            "rev-parse",
            "HEAD",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let source_sha = String::from_utf8(output.stdout).unwrap().trim().to_owned();
    let proposal = OpportunityV1 {
        schema_version: 1,
        opportunity_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
        workspace_uid: workspace_uid.clone(),
        origin: OpportunityOriginV1::Roadmap,
        kind: OpportunityKindV1::Quality,
        target: OpportunityTargetV1 {
            kind: target_kind,
            repository_uid: crate::core::project_repos::stable_uid(
                &workspace_uid,
                "repository",
                "root",
            ),
            canonical_remote: "https://github.com/zbarno/kool.ade".into(),
            source_sha,
        },
        related_feature_uids: Vec::new(),
        related_task_uids: Vec::new(),
        discovered_at_ms: 1,
        observation_revision: 1,
        evidence_refs: vec![OpportunityEvidenceRefV1 {
            evidence_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
            sha256: "b".repeat(64),
            confidentiality: EvidenceConfidentialityV1::Public,
        }],
        problem_or_opportunity: "Improve planning reliability".into(),
        user_value: "Plans stay consistent".into(),
        expected_outcome: "A reviewed feature is available".into(),
        possible_approaches: vec!["Use current planning records".into()],
        recommended_approach: "Use current planning records".into(),
        alternatives: vec!["Defer".into()],
        estimated_scope: OpportunityScopeV1::Small,
        impact: "Clearer planning".into(),
        cost_or_risk: "Ordinary product risk".into(),
        affected_systems: vec!["planning".into()],
        current_capability_state: "Not implemented".into(),
        generic_workaround: None,
        proposed_approach: "Create a reviewed feature".into(),
        dedup_fingerprint: "c".repeat(64),
        status: OpportunityStatusV1::Proposed,
        revision: 1,
    };
    let store = OpportunityStore::new(planning);
    let OpportunitySaveResult::Created { proposal, .. } = store.save_advisory(proposal).unwrap()
    else {
        panic!("fixture opportunity should be created")
    };
    proposal.opportunity_uid
}

fn app_product_proposal(project: &mut crate::app::session::Project) -> String {
    app_proposal(project, OpportunityTargetKindV1::KooladeProduct)
}

/// Production boundary: dispatch an installation confirmation from a guest
/// workspace and verify the app denies it before touching run state. A unit
/// test cannot prove the UI command reaches the identity gate. Dependencies:
/// headless app fixture and temporary stores; no external services. Setup cost:
/// one fixture app and one command dispatch.
#[test]
fn guest_cannot_confirm_self_improvement_installation() {
    let mut app = crate::app::root::board_tests::fixture();
    let crate::app::root::Screen::Connected(project) = &mut app.screen else {
        panic!("fixture should be connected")
    };
    project.state.identity.source = crate::domain::user::IdentitySource::Guest;

    app.dispatch_ui_command(
        crate::ui::ApplicationCommand::ConfirmSelfImprovementInstallation {
            run_uid: uuid::Uuid::new_v4().to_string(),
            expected_revision: 1,
        },
    );

    assert!(app.toasts.last_message().is_some_and(|message| {
        message.contains("Set a current user identity before confirming installation")
    }));
}

/// Production boundary: a configured display name reaches the UI action but
/// cannot install a merged update when the authenticated GitHub profile probe
/// fails. A store test cannot prove the action requires this live identity
/// lookup before recording installation. Dependencies are a headless app and
/// a temporary failing `gh` executable; no network or Docker. Setup cost is
/// one fixture and a short subprocess.
#[cfg(unix)]
#[test]
fn named_user_cannot_confirm_installation_without_github_authentication() {
    use std::os::unix::fs::PermissionsExt;

    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    let temp = tempfile::tempdir().unwrap();
    let bin = temp.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let gh = bin.join("gh");
    std::fs::write(&gh, "#!/bin/sh\nexit 1\n").unwrap();
    std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o700)).unwrap();

    let mut app = crate::app::root::board_tests::fixture();
    let crate::app::root::Screen::Connected(project) = &mut app.screen else {
        panic!("fixture should be connected")
    };
    project.state.identity = crate::domain::user::ResolvedIdentity {
        user: crate::domain::user::CurrentUser::new("Configured Name", Vec::new()),
        source: crate::domain::IdentitySource::ConfigBlock,
    };
    environment.set("PATH", &bin);

    app.dispatch_ui_command(
        crate::ui::ApplicationCommand::ConfirmSelfImprovementInstallation {
            run_uid: uuid::Uuid::new_v4().to_string(),
            expected_revision: 1,
        },
    );

    assert!(
        app.toasts
            .last_message()
            .is_some_and(|message| { message.contains("GitHub authentication is required") })
    );
}

/// Production boundary: exercise the board action, persisted user receipt,
/// feature creation, and self-improvement run setup together. A unit test
/// cannot prove the UI action wires those services in the right order. It
/// uses headless egui and a local Git fixture; no Docker or network is needed.
/// Setup cost is a few local store writes and one temporary Git repository.
#[test]
fn pursue_button_creates_a_planning_run_without_dispatching_implementation() {
    let mut app = crate::app::root::board_tests::fixture();
    let uid = {
        let crate::app::root::Screen::Connected(project) = &mut app.screen else {
            panic!("fixture should be connected")
        };
        app_proposal(project, OpportunityTargetKindV1::ActiveUserProject)
    };
    let ctx = egui::Context::default();
    click_text(&mut app, &ctx, "Improvement proposals (1)");
    click_text(&mut app, &ctx, "Improve planning reliability");
    click_text(&mut app, &ctx, "Pursue");
    assert!(
        app.toasts
            .last_message()
            .is_some_and(|message| { message.contains("Draft feature") }),
        "Pursue should confirm that a run was created, got: {:?}",
        app.toasts.last_message()
    );

    let crate::app::root::Screen::Connected(project) = &app.screen else {
        panic!("fixture should remain connected")
    };
    let opportunities = OpportunityStore::new(&project.state.planning_store);
    let decision = opportunities
        .decision_for_opportunity(&uid)
        .unwrap()
        .expect("Pursue action persists its operator receipt");
    assert_eq!(decision.decision, OpportunityDecisionKindV1::Pursue);
    let (proposal, _) = opportunities.load(&uid).unwrap();
    assert_eq!(
        proposal.status,
        OpportunityStatusV1::Planning,
        "Pursue action did not link the draft feature: {:?}",
        app.toasts.last_message()
    );
    assert!(decision.user_id.contains("Koolade Acceptance"));
    assert_eq!(
        proposal.target.kind,
        OpportunityTargetKindV1::ActiveUserProject
    );
    assert!(project.active_implementations.is_empty());
    assert!(project.active_turn.is_none());
}

#[cfg(unix)]
#[test]
fn product_pursuit_action_persists_authenticated_actor_and_creates_only_a_plan() {
    use std::os::unix::fs::PermissionsExt;

    // Production boundary: exercise the app opportunity controller through
    // decision persistence and self-improvement run creation. Store-only tests
    // cannot prove the authenticated CLI identity reaches the saved actor.
    // Uses a headless app, temporary Git/store fixture, and fake gh executable;
    // no external network or Docker, moderate setup.
    let mut app = crate::app::root::board_tests::fixture();
    let uid = {
        let crate::app::root::Screen::Connected(project) = &mut app.screen else {
            panic!("fixture should be connected")
        };
        app_proposal(project, OpportunityTargetKindV1::KooladeProduct)
    };
    let gh_dir = tempfile::tempdir().unwrap();
    let gh = gh_dir.path().join("gh");
    std::fs::write(
        &gh,
        "#!/bin/sh\n[ \"$1\" = api ] && [ \"$2\" = user ] || exit 2\nprintf '987654321\\n'\n",
    )
    .unwrap();
    std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o700)).unwrap();
    let crate::app::root::Screen::Connected(project) = &app.screen else {
        panic!("fixture should remain connected")
    };
    let revision = OpportunityStore::new(&project.state.planning_store)
        .load(&uid)
        .unwrap()
        .1;

    app.decide_opportunity_with_gh(&uid, revision, OpportunityChoice::Pursue, None, &gh);

    let crate::app::root::Screen::Connected(project) = &app.screen else {
        panic!("fixture should remain connected")
    };
    let opportunities = OpportunityStore::new(&project.state.planning_store);
    let decision = opportunities
        .decision_for_opportunity(&uid)
        .unwrap()
        .expect("authenticated Pursue decision should persist");
    assert_eq!(decision.user_id, "github:987654321");
    let run_uid = SelfImprovementStore::run_uid_for_decision(&decision.uid).unwrap();
    let run = SelfImprovementStore::new(&project.state.planning_store)
        .load(&run_uid)
        .unwrap();
    assert_eq!(run.phase, SelfImprovementPhaseV1::Planning);
    assert!(run.clone_uid.is_none());
    assert!(project.active_implementations.is_empty());
    assert!(project.active_turn.is_none());
}

#[cfg(unix)]
#[test]
fn product_pursuit_action_does_not_persist_when_github_authentication_fails() {
    use std::os::unix::fs::PermissionsExt;

    let mut app = crate::app::root::board_tests::fixture();
    let uid = {
        let crate::app::root::Screen::Connected(project) = &mut app.screen else {
            panic!("fixture should be connected")
        };
        app_proposal(project, OpportunityTargetKindV1::KooladeProduct)
    };
    let gh_dir = tempfile::tempdir().unwrap();
    let gh = gh_dir.path().join("gh");
    std::fs::write(&gh, "#!/bin/sh\nexit 1\n").unwrap();
    std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o700)).unwrap();
    let crate::app::root::Screen::Connected(project) = &app.screen else {
        panic!("fixture should remain connected")
    };
    let revision = OpportunityStore::new(&project.state.planning_store)
        .load(&uid)
        .unwrap()
        .1;

    app.decide_opportunity_with_gh(&uid, revision, OpportunityChoice::Pursue, None, &gh);

    let crate::app::root::Screen::Connected(project) = &app.screen else {
        panic!("fixture should remain connected")
    };
    let opportunities = OpportunityStore::new(&project.state.planning_store);
    assert!(
        opportunities
            .decision_for_opportunity(&uid)
            .unwrap()
            .is_none()
    );
    assert_eq!(
        opportunities.load(&uid).unwrap().0.status,
        OpportunityStatusV1::AwaitingDecision
    );
}

#[test]
fn pursuing_koolade_product_persists_a_planning_run_from_the_user_receipt() {
    let temp = tempfile::tempdir().unwrap();
    let planning = PlanningStore::new(
        uuid::Uuid::new_v4(),
        temp.path().join("planning"),
        StoreMode::ManagedLocal,
    );
    crate::artifacts::migration::bootstrap_product_with_store(
        temp.path(),
        &planning,
        "Synthetic Kool.ad/e fixture",
    )
    .unwrap();
    let opportunities = OpportunityStore::new(&planning);
    let proposal = crate::core::opportunity::OpportunityV1 {
        schema_version: 1,
        opportunity_uid: uuid::Uuid::new_v4().to_string(),
        workspace_uid: uuid::Uuid::new_v4().to_string(),
        origin: OpportunityOriginV1::Roadmap,
        kind: OpportunityKindV1::Quality,
        target: OpportunityTargetV1 {
            kind: OpportunityTargetKindV1::KooladeProduct,
            repository_uid: uuid::Uuid::new_v4().to_string(),
            canonical_remote: "ssh://git@github.com/zbarno/kool.ade".into(),
            source_sha: "a".repeat(40),
        },
        related_feature_uids: Vec::new(),
        related_task_uids: Vec::new(),
        discovered_at_ms: 1,
        observation_revision: 1,
        evidence_refs: vec![OpportunityEvidenceRefV1 {
            evidence_uid: uuid::Uuid::new_v4().to_string(),
            sha256: "b".repeat(64),
            confidentiality: EvidenceConfidentialityV1::Public,
        }],
        problem_or_opportunity: "Improve planning reliability".into(),
        user_value: "Plans stay consistent".into(),
        expected_outcome: "A reviewed feature is available".into(),
        possible_approaches: vec!["Use current planning records".into()],
        recommended_approach: "Use current planning records".into(),
        alternatives: vec!["Defer".into()],
        estimated_scope: OpportunityScopeV1::Small,
        impact: "Clearer planning".into(),
        cost_or_risk: "Ordinary product risk".into(),
        affected_systems: vec!["planning".into()],
        current_capability_state: "Not implemented".into(),
        generic_workaround: None,
        proposed_approach: "Create a reviewed feature".into(),
        dedup_fingerprint: "c".repeat(64),
        status: OpportunityStatusV1::Proposed,
        revision: 1,
    };
    let OpportunitySaveResult::Created {
        proposal,
        record_revision,
    } = opportunities.save_advisory(proposal).unwrap()
    else {
        panic!("test proposal should be created")
    };
    let decision = OpportunityDecisionV1 {
        uid: uuid::Uuid::new_v4().to_string(),
        opportunity_uid: proposal.opportunity_uid.clone(),
        revision_hash: proposal.revision_hash().unwrap(),
        user_id: "local-controller".into(),
        decision: OpportunityChoice::Pursue,
        occurred_at: "2026-10-13T12:00:00Z".into(),
        snooze_until: None,
        explanation: Some("Synthetic UI action".into()),
    };
    opportunities
        .record_user_decision(record_revision, &decision, "local-controller")
        .unwrap();

    let (feature_id, feature_uid, markdown, _) =
        super::super::feature::create_draft_feature(&planning, &proposal).unwrap();
    assert!(markdown.contains(&proposal.target.repository_uid));
    assert!(markdown.contains(&proposal.target.source_sha));
    assert!(!markdown.contains(&proposal.target.canonical_remote));
    assert!(
        crate::artifacts::product_docs::active_features(&planning)
            .iter()
            .any(|(id, content)| id == &feature_id && content == &markdown)
    );
    let run = create_self_improvement_run(&planning, &opportunities, &proposal, &feature_uid)
        .unwrap()
        .unwrap();

    assert_eq!(run.phase, SelfImprovementPhaseV1::Planning);
    assert_eq!(run.feature_uid, feature_uid);
    assert_eq!(run.approved_decision_uid, decision.uid);
    assert_eq!(run.canonical_remote, "https://github.com/zbarno/kool.ade");
    let (linked, _) = opportunities.load(&proposal.opportunity_uid).unwrap();
    assert_eq!(linked.status, OpportunityStatusV1::Planning);
    assert!(linked.related_feature_uids.contains(&run.feature_uid));
    assert_eq!(
        SelfImprovementStore::new(&planning)
            .load(&run.run_uid)
            .unwrap(),
        run
    );
}
