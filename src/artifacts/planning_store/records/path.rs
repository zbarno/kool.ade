use std::path::{Component, Path};

use crate::artifacts::planning_store::StoreError;

pub(crate) fn is_opportunity_quota_record(path: &str) -> bool {
    let components = Path::new(path).components().collect::<Vec<_>>();
    if components.len() != 3
        || !matches!(components.first(), Some(Component::Normal(name)) if name.to_str() == Some("state"))
        || !matches!(components.get(1), Some(Component::Normal(name)) if name.to_str() == Some("opportunity_quota"))
    {
        return false;
    }
    components
        .get(2)
        .and_then(|component| match component {
            Component::Normal(name) => name.to_str(),
            _ => None,
        })
        .and_then(|name| name.strip_suffix(".json"))
        .is_some_and(|date| {
            chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")
                .is_ok_and(|parsed| parsed.format("%Y-%m-%d").to_string() == date)
        })
}

pub(crate) fn validate_record_path(path: &str) -> Result<(), StoreError> {
    if path == "state/opportunity_discovery_settings.json" {
        return Ok(());
    }
    let components = Path::new(path).components().collect::<Vec<_>>();
    let flat_collection = components.get(1).is_some_and(|component| {
        matches!(
            component,
            Component::Normal(name)
                if matches!(
                    name.to_str(),
                    Some(
                        "work"
                            | "workflow"
                            | "items"
                            | "questions"
                            | "readiness"
                            | "tasks"
                            | "lifecycle_features"
                            | "lifecycle_tasks"
                            | "lifecycle_events"
                            | "lifecycle_plans"
                            | "lifecycle_plan_reviews"
                            | "lifecycle_plan_authors"
                            | "lifecycle_plan_confirmations"
                            | "lifecycle_pr_work_items"
                            | "lifecycle_feature_delta_authorizations"
                            | "spec_revisions"
                            | "spec_revision_history"
                            | "current_implementations"
                            | "current_implementation_history"
                            | "spec_sync_queue"
                            | "opportunities"
                            | "opportunity_decisions"
                            | "opportunity_dedup"
                            | "opportunity_quota"
                            | "self_improvements"
                            | "runtime-receipts"
                            | "runtime-target-approvals"
                    )
                )
        )
    });
    let nested_collection = components.len() >= 3
        && matches!(components.first(), Some(Component::Normal(name)) if name.to_str() == Some("state"))
        && matches!(components.get(1), Some(Component::Normal(name)) if name.to_str() == Some("self_improvement_events"))
        && components
            .get(2)
            .and_then(|component| match component {
                Component::Normal(name) => name.to_str(),
                _ => None,
            })
            .is_some_and(|uid| uuid::Uuid::parse_str(uid).is_ok());
    let valid_collection = flat_collection || nested_collection;
    let file_component = if components.len() == 4 {
        components.get(3)
    } else {
        components.get(2)
    };
    let valid_file = is_opportunity_quota_record(path)
        || file_component.is_some_and(|component| {
            let Component::Normal(name) = component else {
                return false;
            };
            let Some(name) = name.to_str() else {
                return false;
            };
            name.strip_suffix(".json").is_some_and(|uid| {
                if components.get(1).is_some_and(|component| {
                    matches!(component, Component::Normal(name) if name.to_str() == Some("opportunity_dedup"))
                }) {
                    uid.len() == 64 && uid.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
                } else {
                    uuid::Uuid::parse_str(uid).is_ok()
                }
            })
        });
    let valid_nested_event = components.len() == 4
        && matches!(components.first(), Some(Component::Normal(name)) if name.to_str() == Some("state"))
        && matches!(components.get(1), Some(Component::Normal(name)) if name.to_str() == Some("self_improvement_events"))
        && components
            .get(2)
            .and_then(|component| match component {
                Component::Normal(name) => name.to_str(),
                _ => None,
            })
            .is_some_and(|uid| uuid::Uuid::parse_str(uid).is_ok())
        && components
            .get(3)
            .and_then(|component| match component {
                Component::Normal(name) => name.to_str(),
                _ => None,
            })
            .and_then(|name| name.strip_suffix(".json"))
            .is_some_and(|uid| uuid::Uuid::parse_str(uid).is_ok());
    if (components.len() != 3 && !valid_nested_event)
        || !matches!(components.first(), Some(Component::Normal(name)) if name.to_str() == Some("state"))
        || !valid_collection
        || !valid_file
    {
        return Err(StoreError::InvalidPath(path.to_owned()));
    }
    Ok(())
}
