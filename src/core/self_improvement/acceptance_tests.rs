//! User-facing opportunity and self-improvement path for #174/#175.
//! The proposal is inert until persisted Pursue; active capability readiness is
//! outside this fixture. It covers store gates and isolated local Git clone only.
#[path = "acceptance_tests/docker_execution.rs"]
mod docker_execution;
#[path = "acceptance_tests/fixture.rs"]
mod fixture;
#[path = "acceptance_tests/scenarios.rs"]
mod scenarios;
#[path = "acceptance_support.rs"]
mod support;

const RESTART_CHILD: &str = "KOOLADE_SELF_IMPROVEMENT_RESTART_CHILD";
const PR_RESTART_CHILD: &str = "KOOLADE_SELF_IMPROVEMENT_PR_RESTART_CHILD";

fn verify_restart_child() -> bool {
    let Some(root) = std::env::var_os(RESTART_CHILD) else {
        return false;
    };
    let project_id =
        uuid::Uuid::parse_str(&std::env::var("KOOLADE_SELF_IMPROVEMENT_PROJECT_ID").unwrap())
            .unwrap();
    let store = crate::artifacts::planning_store::PlanningStore::new(
        project_id,
        root,
        crate::artifacts::planning_store::StoreMode::ManagedLocal,
    );
    let runs = crate::core::self_improvement::SelfImprovementStore::new(&store);
    let run_uid = std::env::var("KOOLADE_SELF_IMPROVEMENT_RUN_UID").unwrap();
    let run = runs.load(&run_uid).unwrap();
    assert_eq!(
        run.phase,
        crate::core::self_improvement::SelfImprovementPhaseV1::Ready
    );
    assert!(run.plan_approval_hash.is_some());
    let updated = runs
        .apply_event(
            &run_uid,
            run.revision,
            &std::env::var("KOOLADE_SELF_IMPROVEMENT_EVENT_UID").unwrap(),
            crate::core::self_improvement::SelfImprovementEventV1::CloneAllocated {
                clone_uid: std::env::var("KOOLADE_SELF_IMPROVEMENT_CLONE_UID").unwrap(),
            },
        )
        .unwrap();
    assert_eq!(
        updated.phase,
        crate::core::self_improvement::SelfImprovementPhaseV1::Implementing
    );
    // Terminate without running test-harness teardown or Rust destructors. The
    // parent verifies that the committed event survives this process boundary.
    std::process::exit(86)
}

fn verify_pr_restart_child() -> bool {
    let Some(root) = std::env::var_os(PR_RESTART_CHILD) else {
        return false;
    };
    let project_id =
        uuid::Uuid::parse_str(&std::env::var("KOOLADE_SELF_IMPROVEMENT_PROJECT_ID").unwrap())
            .unwrap();
    let store = crate::artifacts::planning_store::PlanningStore::new(
        project_id,
        root,
        crate::artifacts::planning_store::StoreMode::ManagedLocal,
    );
    let run_uid = std::env::var("KOOLADE_SELF_IMPROVEMENT_RUN_UID").unwrap();
    let run = crate::core::self_improvement::SelfImprovementStore::new(&store)
        .load(&run_uid)
        .unwrap();
    assert_eq!(
        run.phase,
        crate::core::self_improvement::SelfImprovementPhaseV1::PrOpened
    );
    assert_eq!(
        run.pr_url.as_deref(),
        Some(
            std::env::var("KOOLADE_SELF_IMPROVEMENT_PR_URL")
                .unwrap()
                .as_str()
        )
    );
    assert!(run.merged_sha.is_none());
    // Exit immediately so the parent proves PR publication state survived an
    // OS-process boundary while the controller is waiting for provider input.
    std::process::exit(87)
}

#[derive(Clone, Copy)]
enum OpportunityScenario {
    UnsupportedLanguage,
    UnrelatedUx,
}
