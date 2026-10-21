use super::super::{MERGED_RESTART_CHILD, PR_RESTART_CHILD, RESTART_CHILD};
use crate::{
    artifacts::planning_store::{PlanningStore, StoreMode},
    core::self_improvement::{
        SelfImprovementEventV1 as Event, SelfImprovementPhaseV1 as Phase, SelfImprovementRunV1,
        SelfImprovementStore,
    },
};
use std::process::Command;

pub(super) fn verify_clone_allocation_restart(
    planning: PlanningStore,
    run: &SelfImprovementRunV1,
    event_uid: &str,
    clone_uid: &str,
) -> (PlanningStore, SelfImprovementRunV1) {
    let project_id = planning.project_id;
    let planning_root = planning.root.clone();
    let run_uid = run.run_uid.clone();
    let test_name = std::thread::current().name().unwrap().to_owned();
    drop(planning);
    // Production boundary: a fresh OS process reloads the approved run, saves
    // clone allocation, and exits. A same-process unit test cannot prove the
    // store reopens across process death or that replay remains idempotent.
    // It uses local Git/store data and a second test-binary process, with no
    // Docker or network; setup cost is moderate because each scenario reopens
    // the lifecycle state and repeats the fixture.
    let status = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", &test_name, "--nocapture"])
        .env(
            "KOOLADE_SELF_IMPROVEMENT_PROJECT_ID",
            project_id.to_string(),
        )
        .env("KOOLADE_SELF_IMPROVEMENT_RUN_UID", &run_uid)
        .env("KOOLADE_SELF_IMPROVEMENT_EVENT_UID", event_uid)
        .env("KOOLADE_SELF_IMPROVEMENT_CLONE_UID", clone_uid)
        .env(RESTART_CHILD, &planning_root)
        .status()
        .unwrap();
    assert_eq!(
        status.code(),
        Some(86),
        "restart child must persist clone allocation and terminate immediately"
    );
    let planning = PlanningStore::new(project_id, planning_root, StoreMode::ManagedLocal);
    let runs = SelfImprovementStore::new(&planning);
    let run = runs.load(&run_uid).unwrap();
    assert_eq!(run.phase, Phase::Implementing);
    let resumed = runs
        .apply_event(
            &run_uid,
            run.revision,
            event_uid,
            Event::CloneAllocated {
                clone_uid: clone_uid.to_owned(),
            },
        )
        .unwrap();
    assert_eq!(
        resumed, run,
        "replaying the child process event must be idempotent"
    );
    assert_eq!(resumed.phase, Phase::Implementing);
    (planning, resumed)
}

pub(super) fn verify_pr_open_restart(
    planning: PlanningStore,
    run: &SelfImprovementRunV1,
) -> PlanningStore {
    let project_id = planning.project_id;
    let planning_root = planning.root.clone();
    let run_uid = run.run_uid.clone();
    let test_name = std::thread::current().name().unwrap().to_owned();
    let pr_url = run.pr_url.as_deref().unwrap().to_owned();
    drop(planning);
    // Production boundary: a fresh OS process reloads durable PR-opened state
    // while the controller waits for the provider. A same-process unit test
    // cannot prove publication state survives process death. It uses local
    // store data and a second test-binary process, with no Docker or network;
    // setup cost is moderate because each scenario repeats the fixture.
    let status = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", &test_name, "--nocapture"])
        .env(
            "KOOLADE_SELF_IMPROVEMENT_PROJECT_ID",
            project_id.to_string(),
        )
        .env("KOOLADE_SELF_IMPROVEMENT_RUN_UID", &run_uid)
        .env("KOOLADE_SELF_IMPROVEMENT_PR_URL", &pr_url)
        .env(PR_RESTART_CHILD, &planning_root)
        .status()
        .unwrap();
    assert_eq!(
        status.code(),
        Some(87),
        "restart child must load the published PR state and terminate immediately"
    );
    let planning = PlanningStore::new(project_id, planning_root, StoreMode::ManagedLocal);
    let runs = SelfImprovementStore::new(&planning);
    let run = runs.load(&run_uid).unwrap();
    assert_eq!(run.phase, Phase::PrOpened);
    assert_eq!(run.pr_url.as_deref(), Some(pr_url.as_str()));
    planning
}

pub(super) fn verify_merged_restart(
    planning: PlanningStore,
    run: &SelfImprovementRunV1,
) -> PlanningStore {
    let project_id = planning.project_id;
    let planning_root = planning.root.clone();
    let run_uid = run.run_uid.clone();
    let merged_sha = run.merged_sha.as_deref().unwrap().to_owned();
    let test_name = std::thread::current().name().unwrap().to_owned();
    drop(planning);
    // Production boundary: reopen the managed self-improvement record in a
    // fresh process after provider merge evidence is committed. This proves
    // restart preserves merged-but-not-installed state; in-process unit tests
    // cannot cover process death. It uses local store data only, with no
    // Docker or network; setup cost is moderate because each scenario repeats
    // the approval and PR fixture.
    let status = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", &test_name, "--nocapture"])
        .env(
            "KOOLADE_SELF_IMPROVEMENT_PROJECT_ID",
            project_id.to_string(),
        )
        .env("KOOLADE_SELF_IMPROVEMENT_RUN_UID", &run_uid)
        .env("KOOLADE_SELF_IMPROVEMENT_MERGED_SHA", &merged_sha)
        .env(MERGED_RESTART_CHILD, &planning_root)
        .status()
        .unwrap();
    assert_eq!(
        status.code(),
        Some(88),
        "restart child must retain merged-but-not-installed state"
    );
    let planning = PlanningStore::new(project_id, planning_root, StoreMode::ManagedLocal);
    let runs = SelfImprovementStore::new(&planning);
    let run = runs.load(&run_uid).unwrap();
    assert_eq!(run.phase, Phase::MergedNotActivated);
    assert_eq!(run.merged_sha.as_deref(), Some(merged_sha.as_str()));
    assert!(run.installed_version.is_none());
    planning
}
