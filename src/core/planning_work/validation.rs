use super::{CodeReviewDispositionStatusV1, Work, WorkKind, WorkStatus, load, record_changes};
use crate::artifacts::planning_store::PlanningRoot;

mod cycles;
mod history;

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
        if let Some(recovery) = &item.operational_recovery {
            super::operational_recovery::validate(item, recovery)?;
        }
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
            anyhow::ensure!(uid != &item.uid, "Planning work cannot parent itself");
        }
        if let Some(uid) = &item.task_intent_uid {
            anyhow::ensure!(
                uuid::Uuid::parse_str(uid).is_ok(),
                "Invalid TaskIntent root UID"
            );
            if item.kind.starts_task_intent() && item.parent_uid.is_none() {
                anyhow::ensure!(uid == &item.uid, "TaskIntent root must own itself");
            }
        }
        if let Some(uid) = &item.feature_uid {
            anyhow::ensure!(uuid::Uuid::parse_str(uid).is_ok(), "Invalid feature UID");
        }
        if let Some(git_intent) = &item.git_intent {
            git_intent.validate()?;
            anyhow::ensure!(
                item.source_branch.as_deref() == Some(git_intent.source_ref.as_str())
                    && item.destination_branch.as_deref()
                        == Some(git_intent.destination_ref.as_str()),
                "Planning work branch fields disagree with its approved Git intent"
            );
            if let Some(parent_uid) = git_intent.inherited_from_task_uid.as_deref() {
                anyhow::ensure!(
                    item.parent_uid.as_deref() == Some(parent_uid),
                    "Inherited Git intent must name the current task parent"
                );
            }
        }
        for (repository_id, git_intent) in &item.git_intents {
            git_intent.validate()?;
            anyhow::ensure!(
                repository_id == &git_intent.repository_id,
                "Planning work Git intent key does not match its repository ID"
            );
            if let Some(parent_uid) = git_intent.inherited_from_task_uid.as_deref() {
                anyhow::ensure!(
                    item.parent_uid.as_deref() == Some(parent_uid),
                    "Inherited Git intent must name the current task parent"
                );
            }
            if let Some(parent_uid) = git_intent.inherited_from_task_uid.as_deref() {
                let parent = work
                    .iter()
                    .find(|candidate| candidate.uid == parent_uid)
                    .ok_or_else(|| {
                        anyhow::anyhow!("Inherited Git intent parent task {parent_uid} is missing")
                    })?;
                let parent_intent = parent.git_intents.get(repository_id).ok_or_else(|| {
                    anyhow::anyhow!(
                        "Inherited Git intent has no matching repository approval on parent task {parent_uid}"
                    )
                })?;
                anyhow::ensure!(
                    git_intent.source_ref == parent_intent.source_ref
                        && git_intent.source_sha == parent_intent.source_sha
                        && git_intent.destination_ref == parent_intent.destination_ref
                        && git_intent.publication_mode == parent_intent.publication_mode,
                    "Inherited Git intent differs from its parent approval; approve the override explicitly"
                );
            }
        }
        if let Some(primary) = item.git_intents.get("root") {
            anyhow::ensure!(
                item.git_intent.as_ref() == Some(primary)
                    && item.source_branch.as_deref() == Some(primary.source_ref.as_str())
                    && item.destination_branch.as_deref() == Some(primary.destination_ref.as_str()),
                "Primary repository fields disagree with their approved Git intent"
            );
        } else {
            anyhow::ensure!(
                item.git_intent.is_none()
                    && item.source_branch.is_none()
                    && item.destination_branch.is_none(),
                "Unmapped primary repository Git intent fields are not allowed"
            );
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
    for item in work {
        if item.parent_uid.is_none() && item.kind.starts_task_intent() {
            anyhow::ensure!(
                item.task_intent_uid.as_deref() == Some(item.uid.as_str()),
                "TaskIntent root must store its current immutable UID"
            );
        }
        let Some(intent_uid) = item.task_intent_uid.as_deref() else {
            continue;
        };
        if intent_uid == item.uid {
            anyhow::ensure!(
                item.parent_uid.is_none() && item.kind.starts_task_intent(),
                "TaskIntent UID must identify a supported root request"
            );
            cycles::validate(item, work)?;
            continue;
        }
        let root = work
            .iter()
            .find(|candidate| candidate.uid == intent_uid)
            .ok_or_else(|| anyhow::anyhow!("TaskIntent root does not exist in planning work"))?;
        anyhow::ensure!(
            root.parent_uid.is_none()
                && root.kind.starts_task_intent()
                && root.task_intent_root_uid() == Some(intent_uid),
            "Planning work references an invalid TaskIntent root"
        );
        if let Some(cycle_uid) = item.task_intent_cycle_uid.as_deref() {
            anyhow::ensure!(
                root.task_intent_cycles
                    .iter()
                    .any(|cycle| cycle.uid == cycle_uid),
                "Planning work references an invalid TaskIntent cycle"
            );
        }
        if let Some(parent_uid) = item.parent_uid.as_deref()
            && let Some(parent) = work.iter().find(|candidate| candidate.uid == parent_uid)
            && let Some(parent_intent_uid) = parent.task_intent_root_uid()
        {
            anyhow::ensure!(
                intent_uid == parent_intent_uid,
                "Planning work ownership must match its parent work"
            );
        }
    }
    for item in work {
        if item.parent_uid.is_none() && item.kind.starts_task_intent() {
            anyhow::ensure!(
                item.original_goal
                    .as_deref()
                    .is_some_and(|goal| !goal.trim().is_empty()),
                "TaskIntent root must preserve its original goal"
            );
        }
        let mut seen = std::collections::BTreeSet::new();
        let mut parent_uid = item.parent_uid.as_deref();
        while let Some(uid) = parent_uid {
            anyhow::ensure!(
                seen.insert(uid),
                "Planning work parent links contain a cycle"
            );
            parent_uid = work
                .iter()
                .find(|candidate| candidate.uid == uid)
                .and_then(|candidate| candidate.parent_uid.as_deref());
        }
        if !item.task_intent_history.is_empty() {
            anyhow::ensure!(
                item.task_intent_root_uid() == Some(item.uid.as_str()),
                "Only a TaskIntent root may store its shared history"
            );
            history::validate(item, work)?;
        }
        if let Some(archived_at_ms) = item.task_intent_archived_at_ms {
            anyhow::ensure!(
                item.task_intent_root_uid() == Some(item.uid.as_str()) && archived_at_ms > 0,
                "Only a TaskIntent root may have a valid archive timestamp"
            );
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
    let store = repo.planning_store();
    let mut desired = work.to_vec();
    super::normalize_task_intents(&mut desired);
    validate(&desired)?;
    let (changes, checks) = record_changes(&store, &desired)?;
    let (_, revision) = store.transaction_with_revision_and_record_revisions(
        &changes,
        Some(expected_revision),
        &checks,
    )?;
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
                | WorkKind::QaFindingDisposition
        ) && super::belongs_to_feature(work, feature_id, feature_uid)
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
