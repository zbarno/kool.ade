use crate::{
    app::root::harness_selection::role_for_work_category, execution::contract::HarnessRole,
};

#[test]
fn every_routable_role_category_has_explicit_provider_neutral_authority() {
    use crate::persistence::harness_settings as routes;
    for (category, role) in [
        (routes::MANAGER, HarnessRole::Manager),
        (routes::PLANNING, HarnessRole::PlanningAnalyst),
        (routes::SPECIFICATION, HarnessRole::PlanningAnalyst),
        (routes::FEATURE_PLANNING, HarnessRole::PlanningAnalyst),
        (routes::TASK_GENERATION, HarnessRole::TaskDecomposer),
        (routes::INVESTIGATION, HarnessRole::PlanningAnalyst),
        (routes::PLAN_REVIEW, HarnessRole::PlanReviewer),
        (routes::IMPLEMENTATION, HarnessRole::Implementer),
        (routes::QA, HarnessRole::CodeReviewer),
        (routes::RUNTIME_QA, HarnessRole::RuntimeQaReviewer),
        (routes::DOCUMENTATION, HarnessRole::PRPreparer),
        (routes::RECONCILIATION, HarnessRole::SpecReconciler),
        (routes::DECISION_EXPLANATION, HarnessRole::DecisionExplainer),
        (routes::REPAIR, HarnessRole::RepairWorker),
    ] {
        assert_eq!(role_for_work_category(category), Some(role), "{category}");
    }
}

#[test]
fn task_overrides_cover_routable_categories_but_never_manager() {
    use crate::persistence::harness_settings as routes;
    assert!(routes::is_task_routable_category(routes::INVESTIGATION));
    assert!(routes::is_task_routable_category(routes::RECONCILIATION));
    assert!(routes::is_task_routable_category(
        routes::DECISION_EXPLANATION
    ));
    assert!(!routes::is_task_routable_category(routes::MANAGER));
    assert!(routes::is_task_routable_category(routes::RUNTIME_QA));
    assert!(!routes::is_task_routable_category("unknown_role"));
}

#[test]
fn work_kinds_use_the_matching_global_role_route() {
    use crate::{core::planning_work::WorkKind, persistence::harness_settings as routes};
    for (kind, category) in [
        (WorkKind::Feature, routes::FEATURE_PLANNING),
        (WorkKind::Bug, routes::INVESTIGATION),
        (WorkKind::NewProject, routes::SPECIFICATION),
        (WorkKind::DocumentationRefresh, routes::DOCUMENTATION),
        (WorkKind::Question, routes::INVESTIGATION),
        (WorkKind::TaskGeneration, routes::TASK_GENERATION),
        (WorkKind::PlanReviewDisposition, routes::MANAGER),
    ] {
        assert_eq!(kind.harness_route_category(), category);
    }
}

#[test]
fn route_precedence_is_task_then_project_then_global_then_default() {
    use crate::persistence::harness_settings::{IMPLEMENTATION, WorkRoute};
    use std::collections::BTreeMap;

    let route = |harness: &str| WorkRoute {
        harness: harness.into(),
        model: None,
    };
    let task = route("copilot");
    let project_routes = BTreeMap::from([(IMPLEMENTATION.into(), route("claude"))]);
    let global_routes = BTreeMap::from([(IMPLEMENTATION.into(), route("codex"))]);
    let empty_routes = BTreeMap::new();

    let selected =
        super::super::super::role_router::resolve(super::super::super::role_router::RouteSources {
            category: Some(IMPLEMENTATION),
            task_override: Some(&task),
            project_routes: &project_routes,
            global_routes: &global_routes,
            default_harness: Some("pi"),
            legacy_harness: Some("antigravity"),
        });
    assert_eq!(selected.harness, Some("copilot"));

    let selected =
        super::super::super::role_router::resolve(super::super::super::role_router::RouteSources {
            category: Some(IMPLEMENTATION),
            task_override: None,
            project_routes: &project_routes,
            global_routes: &global_routes,
            default_harness: Some("pi"),
            legacy_harness: Some("antigravity"),
        });
    assert_eq!(selected.harness, Some("claude"));

    let selected =
        super::super::super::role_router::resolve(super::super::super::role_router::RouteSources {
            category: Some(IMPLEMENTATION),
            task_override: None,
            project_routes: &empty_routes,
            global_routes: &global_routes,
            default_harness: Some("pi"),
            legacy_harness: Some("antigravity"),
        });
    assert_eq!(selected.harness, Some("codex"));

    let selected =
        super::super::super::role_router::resolve(super::super::super::role_router::RouteSources {
            category: Some(IMPLEMENTATION),
            task_override: None,
            project_routes: &empty_routes,
            global_routes: &empty_routes,
            default_harness: Some("pi"),
            legacy_harness: Some("antigravity"),
        });
    assert_eq!(selected.harness, Some("pi"));

    let selected =
        super::super::super::role_router::resolve(super::super::super::role_router::RouteSources {
            category: Some(IMPLEMENTATION),
            task_override: None,
            project_routes: &empty_routes,
            global_routes: &empty_routes,
            default_harness: None,
            legacy_harness: Some("antigravity"),
        });
    assert_eq!(selected.harness, Some("antigravity"));
}
