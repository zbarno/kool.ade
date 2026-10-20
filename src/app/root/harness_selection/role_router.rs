use crate::execution::contract::HarnessRole;
use crate::persistence::harness_settings::{
    DECISION_EXPLANATION, DOCUMENTATION, FEATURE_PLANNING, IMPLEMENTATION, INVESTIGATION, MANAGER,
    PLAN_REVIEW, PLANNING, QA, RECONCILIATION, REPAIR, RUNTIME_QA, SPECIFICATION, TASK_GENERATION,
    WorkRoute,
};
use std::collections::BTreeMap;

pub(crate) struct RouteSources<'a> {
    pub category: Option<&'a str>,
    pub task_override: Option<&'a WorkRoute>,
    pub project_routes: &'a BTreeMap<String, WorkRoute>,
    pub global_routes: &'a BTreeMap<String, WorkRoute>,
    pub default_harness: Option<&'a str>,
    pub legacy_harness: Option<&'a str>,
}

pub(crate) struct RouteSelection<'a> {
    pub harness: Option<&'a str>,
    pub route: Option<&'a WorkRoute>,
}

/// Resolve route precedence once for every role entry point. A task override
/// is accepted only for its own category; Manager safety is validated by the
/// caller before reaching this selector.
pub(crate) fn resolve<'a>(sources: RouteSources<'a>) -> RouteSelection<'a> {
    if let Some(route) = sources.task_override {
        return RouteSelection {
            harness: Some(route.harness.as_str()),
            route: Some(route),
        };
    }

    let categories = sources
        .category
        .map(|category| vec![category])
        .unwrap_or_else(|| vec![PLANNING, IMPLEMENTATION]);
    for category in categories {
        if let Some(route) = sources.project_routes.get(category) {
            return RouteSelection {
                harness: Some(route.harness.as_str()),
                route: Some(route),
            };
        }
        if let Some(route) = sources.global_routes.get(category) {
            return RouteSelection {
                harness: Some(route.harness.as_str()),
                route: Some(route),
            };
        }
    }
    RouteSelection {
        harness: sources.default_harness.or(sources.legacy_harness),
        route: None,
    }
}

/// Maps persisted work categories to their authority-bearing harness role.
pub(crate) fn role_for_work_category(category: &str) -> Option<HarnessRole> {
    match category {
        MANAGER => Some(HarnessRole::Manager),
        PLANNING | SPECIFICATION | FEATURE_PLANNING => Some(HarnessRole::PlanningAnalyst),
        TASK_GENERATION => Some(HarnessRole::TaskDecomposer),
        INVESTIGATION => Some(HarnessRole::PlanningAnalyst),
        PLAN_REVIEW => Some(HarnessRole::PlanReviewer),
        IMPLEMENTATION => Some(HarnessRole::Implementer),
        QA => Some(HarnessRole::CodeReviewer),
        RUNTIME_QA => Some(HarnessRole::RuntimeQaReviewer),
        DOCUMENTATION => Some(HarnessRole::PRPreparer),
        RECONCILIATION => Some(HarnessRole::SpecReconciler),
        DECISION_EXPLANATION => Some(HarnessRole::DecisionExplainer),
        REPAIR => Some(HarnessRole::RepairWorker),
        _ => None,
    }
}
