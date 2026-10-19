use super::*;

pub(super) fn read_review(
    store: &PlanningStore,
    path: &str,
    snapshot: &PlanSnapshotV1,
) -> Result<Option<PlanReviewRecord>, StoreError> {
    match store.read_record::<PlanReviewRecord>(path) {
        Ok((record, revision)) => {
            if record.schema_version != 1
                || record.uid != snapshot.uid
                || record.revision != revision
                || record.created_at_ms == 0
                || record.updated_at_ms < record.created_at_ms
            {
                return Err(StoreError::MalformedState(format!(
                    "invalid plan review record: {path}"
                )));
            }
            validate_plan_review(snapshot, &record.receipt)?;
            if let Some(verdict) = &record.verdict {
                validate_verdict(snapshot, &record.receipt, verdict)
                    .map_err(StoreError::MalformedState)?;
            }
            for attempt in &record.attempts {
                validate_plan_review(snapshot, &attempt.receipt)?;
                if let Some(verdict) = &attempt.verdict {
                    validate_verdict(snapshot, &attempt.receipt, verdict)
                        .map_err(StoreError::MalformedState)?;
                }
            }
            let mut effective_verdict = record.verdict.as_ref();
            let mut arbitrated_findings = std::collections::HashMap::<&str, usize>::new();
            for arbitration in &record.arbitrations {
                validate_plan_review(snapshot, &arbitration.receipt)?;
                validate_verdict(snapshot, &arbitration.receipt, &arbitration.verdict)
                    .map_err(StoreError::MalformedState)?;
                let attempts = arbitrated_findings
                    .entry(arbitration.finding_id.as_str())
                    .or_default();
                *attempts += 1;
                if *attempts > 2
                    || is_review_unable(&arbitration.verdict)
                    || !effective_verdict.is_some_and(|verdict| {
                        verdict
                            .findings
                            .iter()
                            .any(|finding| finding.id == arbitration.finding_id)
                    })
                {
                    return Err(StoreError::MalformedState(
                        "arbitration does not address a unique active finding".into(),
                    ));
                }
                if arbitration.finding_id.is_empty()
                    || !eligible_evidence_id(&arbitration.evidence_id)
                    || !arbitration
                        .receipt
                        .evidence_refs
                        .iter()
                        .any(|reference| reference == &arbitration.evidence_id)
                    || arbitration.evidence_sha256.len() != 64
                    || !arbitration
                        .evidence_sha256
                        .bytes()
                        .all(|byte| byte.is_ascii_hexdigit())
                    || arbitration.source_quote.trim().len() < 8
                    || arbitration.rationale.trim().len() < 32
                {
                    return Err(StoreError::MalformedState(
                        "invalid plan arbitration provenance".into(),
                    ));
                }
                effective_verdict = Some(&arbitration.verdict);
            }
            Ok(Some(record))
        }
        Err(StoreError::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

pub(super) fn validate_plan_review(
    snapshot: &PlanSnapshotV1,
    receipt: &GateReceipt,
) -> Result<(), StoreError> {
    receipt.validate().map_err(StoreError::MalformedState)?;
    if receipt.actor != GateActor::IndependentReviewer
        || receipt.subject_uid != snapshot.uid
        || receipt.subject_sha256 != snapshot.content_hash
        || receipt.author_run_id.as_deref() != snapshot.author_run_id.as_deref()
        || receipt.author_session_id.as_deref() != snapshot.author_session_id.as_deref()
        || receipt.prompt_role() != Some(crate::harness::prompt_catalog::PromptRole::PlanReviewer)
    {
        return Err(StoreError::MalformedState(
            "plan review must be an independent PlanReviewer receipt with matching author provenance for this exact snapshot"
                .into(),
        ));
    }
    Ok(())
}

pub(super) fn review_path(snapshot: &PlanSnapshotV1) -> String {
    format!("state/lifecycle_plan_reviews/{}.json", snapshot.uid)
}

pub(super) const MAX_REVIEW_ATTEMPTS: usize = 3;

pub(super) fn is_review_unable(verdict: &crate::harness::prompt_catalog::ReviewVerdictV1) -> bool {
    verdict.outcome == Some(crate::harness::prompt_catalog::ReviewOutcomeV1::ReviewUnable)
}
