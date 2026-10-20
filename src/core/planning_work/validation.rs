use super::{CodeReviewDispositionStatusV1, Work, WorkKind, WorkStatus, load, record_changes};
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
        match (&item.kind, &item.code_review_disposition) {
            (WorkKind::CodeReviewDisposition, Some(disposition)) => {
                anyhow::ensure!(
                    disposition.finding_id.trim().len() <= 128
                        && !disposition.finding_id.trim().is_empty()
                        && !disposition.finding_id.chars().any(char::is_control)
                        && is_sha256(&disposition.subject_hash)
                        && disposition
                            .finding_fingerprint
                            .as_deref()
                            .is_none_or(is_sha256)
                        && disposition.occurrence_count > 0
                        && disposition.arbitration_attempts
                            <= super::MAX_CODE_REVIEW_ARBITRATION_ATTEMPTS
                        && disposition.decision_reason.trim().len() >= 16
                        && disposition.decision_reason.len() <= 2048
                        && disposition.owner_work_id == item.uid
                        && disposition.evidence_ref.starts_with("code-review-finding:")
                        && disposition.evidence_ref.len() == 44
                        && disposition.evidence_ref[20..]
                            .bytes()
                            .all(|byte| byte.is_ascii_hexdigit()),
                    "Code-review disposition is incomplete or is not bound to its finding work record"
                );
                anyhow::ensure!(
                    item.parent_uid.is_some(),
                    "Code-review disposition must name its parent implementation task"
                );
                if disposition.status == CodeReviewDispositionStatusV1::Open {
                    anyhow::ensure!(
                        matches!(item.status, WorkStatus::NeedsAttention | WorkStatus::Done),
                        "Open code-review decisions must remain on the attention lane"
                    );
                }
                if disposition.status == CodeReviewDispositionStatusV1::RiskApproved {
                    anyhow::ensure!(
                        item.status == WorkStatus::Done
                            && disposition.scope
                                == Some(
                                    crate::harness::prompt_catalog::FindingScopeV1::NonBlocking
                                ),
                        "Risk approval requires a completed nonblocking finding decision"
                    );
                }
            }
            (WorkKind::CodeReviewDisposition, None) => {
                anyhow::bail!("Code-review work is missing its structured finding disposition");
            }
            (_, Some(_)) => anyhow::bail!(
                "Structured code-review disposition is only valid on code-review work"
            ),
            (_, None) => {}
        }
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

fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
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
    let store = repo.planning_store();
    let (changes, checks, legacy_migration) = record_changes(&store, work)?;
    let (_, revision) = if legacy_migration {
        store.transaction_with_revision_and_record_revisions(
            &changes,
            Some(expected_revision),
            &checks,
        )?
    } else {
        store.transaction_with_record_revisions(&changes, &checks)?
    };
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
        !matches!(
            work.kind,
            WorkKind::TaskGeneration
                | WorkKind::PlanReviewDisposition
                | WorkKind::CodeReviewDisposition
        ) && (feature_uid.is_some() && work.feature_uid.as_deref() == feature_uid
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
