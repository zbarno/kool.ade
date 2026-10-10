use super::{SCHEMA_VERSION, Work, WorkFile, WorkKind, load};
use crate::artifacts::planning_store::PlanningRoot;

pub(super) fn validate(work: &[Work]) -> anyhow::Result<()> {
    let mut uids = std::collections::BTreeSet::new();
    for item in work {
        anyhow::ensure!(
            uuid::Uuid::parse_str(&item.uid).is_ok(),
            "Planning work has invalid UID: {}",
            item.uid
        );
        anyhow::ensure!(
            uids.insert(item.uid.as_str()),
            "Duplicate planning work UID"
        );
        if let Some(uid) = &item.parent_uid {
            anyhow::ensure!(
                uuid::Uuid::parse_str(uid).is_ok(),
                "Invalid parent work UID"
            );
        }
        if let Some(uid) = &item.feature_uid {
            anyhow::ensure!(uuid::Uuid::parse_str(uid).is_ok(), "Invalid feature UID");
        }
        if let Some(uid) = &item.routing_inherited_from {
            anyhow::ensure!(
                uuid::Uuid::parse_str(uid).is_ok(),
                "Invalid task routing parent UID"
            );
            anyhow::ensure!(
                item.parent_uid.as_deref() == Some(uid.as_str()),
                "Inherited task routing must name the task parent"
            );
        }
        for (category, route) in &item.routing_overrides {
            anyhow::ensure!(
                matches!(
                    category.as_str(),
                    crate::persistence::harness_settings::IMPLEMENTATION
                        | crate::persistence::harness_settings::QA
                        | crate::persistence::harness_settings::DOCUMENTATION
                ),
                "Task routing cannot override category '{category}'"
            );
            anyhow::ensure!(
                !route.harness.trim().is_empty()
                    && route.harness.len() <= 128
                    && route
                        .harness
                        .chars()
                        .all(|ch| ch.is_ascii_alphanumeric() || "-_".contains(ch)),
                "Task routing has an invalid harness ID"
            );
            if let Some(model) = &route.model {
                anyhow::ensure!(
                    !model.trim().is_empty()
                        && model.len() <= 512
                        && !model.chars().any(char::is_control),
                    "Task routing has an invalid model ID"
                );
            }
        }
    }
    Ok(())
}

pub fn save<R: PlanningRoot + ?Sized>(repo: &R, work: &[Work]) -> anyhow::Result<()> {
    let revision = repo.planning_store().revision()?;
    save_expected(repo, work, &revision)?;
    Ok(())
}

pub fn save_expected<R: PlanningRoot + ?Sized>(
    repo: &R,
    work: &[Work],
    expected_revision: &str,
) -> anyhow::Result<String> {
    validate(work)?;
    let file = WorkFile {
        schema_version: SCHEMA_VERSION,
        items: work.to_vec(),
    };
    let store = repo.planning_store();
    let changes = vec![(
        crate::artifacts::planning_store::paths::WORK.to_owned(),
        serde_json::to_vec_pretty(&file)?,
    )];
    let (_, revision) = store.transaction_with_revision(&changes, Some(expected_revision))?;
    Ok(revision)
}

/// Snapshot task-owned routing into implementation stories for a feature.
/// The saved task record is authoritative even if app defaults changed later.
pub fn routing_for_feature<R: PlanningRoot + ?Sized>(
    repo: &R,
    feature_id: &str,
    feature_uid: Option<&str>,
) -> anyhow::Result<crate::core::workflow::TaskRoutingSnapshot> {
    let work = load(repo)?.into_iter().rev().find(|work| {
        work.kind != WorkKind::TaskGeneration
            && (feature_uid.is_some() && work.feature_uid.as_deref() == feature_uid
                || work.feature_id.as_deref() == Some(feature_id))
    });
    Ok(work.map_or_else(
        crate::core::workflow::TaskRoutingSnapshot::default,
        |work| crate::core::workflow::TaskRoutingSnapshot {
            overrides: work.routing_overrides,
            source_work_uid: Some(work.uid),
            inherited_from: work.routing_inherited_from,
        },
    ))
}
