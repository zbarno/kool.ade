use crate::core::planning_work::{Work, WorkStatus};
use sha2::Digest;

pub(super) fn disposition_key(task_uid: &str, finding_fingerprint: &str) -> String {
    let digest = format!(
        "{:x}",
        sha2::Sha256::digest(format!("{task_uid}\0{finding_fingerprint}").as_bytes())
    );
    format!("task:code-review-disposition:{}", &digest[..24])
}

pub(super) fn merge(
    existing: &Work,
    mut proposed: Work,
    evidence_id: &str,
    reason: &str,
) -> Result<Option<Work>, String> {
    let Some(previous) = existing.code_review_disposition.as_ref() else {
        return Err("recurring code-review work is missing its structured disposition".into());
    };
    let Some(mut current) = proposed.code_review_disposition.take() else {
        return Err("new code-review disposition was not constructed".into());
    };
    if previous.subject_hash == current.subject_hash {
        return Ok(None);
    }
    if existing.status == WorkStatus::InProgress {
        return Err("recurring code-review finding already has an active disposition".into());
    }
    let mut recurring = existing.clone();
    recurring.title = proposed.title;
    recurring.request = proposed.request;
    recurring.detail.push_str(&format!(
        "\n\nRecurring finding observed on new review subject {} as {} (previous subject {}, finding {}). Current evidence: {}. Manager rationale: {}",
        current.subject_hash,
        current.finding_id,
        previous.subject_hash,
        previous.finding_id,
        evidence_id,
        reason
    ));
    recurring.status = proposed.status;
    recurring.feature_uid = proposed.feature_uid;
    recurring.parent_uid = proposed.parent_uid;
    current.occurrence_count = previous.occurrence_count.saturating_add(1);
    current.arbitration_attempts = previous.arbitration_attempts;
    current.decision_reason = reason.to_owned();
    current.owner_work_id = recurring.uid.clone();
    recurring.code_review_disposition = Some(current);
    Ok(Some(recurring))
}

#[cfg(test)]
#[path = "recurrence/tests.rs"]
mod tests;
