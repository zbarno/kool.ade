use super::*;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PlanReviewArbitration {
    pub(crate) finding_id: String,
    pub(crate) evidence_id: String,
    pub(crate) evidence_sha256: String,
    pub(crate) source_quote: String,
    pub(crate) rationale: String,
    pub(crate) receipt: GateReceipt,
    pub(crate) verdict: crate::harness::prompt_catalog::ReviewVerdictV1,
}

pub(super) fn eligible_evidence_id(evidence_id: &str) -> bool {
    [
        "plan:",
        "decision:",
        "feature-decisions:",
        "readiness:",
        "product-spec:",
    ]
    .iter()
    .any(|prefix| {
        evidence_id
            .strip_prefix(prefix)
            .is_some_and(|identity| !identity.trim().is_empty())
    })
}

#[allow(clippy::too_many_arguments)] // Mirrors the persisted arbitration receipt fields.
pub(crate) fn arbitration_change(
    store: &PlanningStore,
    snapshot: &PlanSnapshotV1,
    finding_id: &str,
    evidence_id: &str,
    evidence_sha256: &str,
    source_quote: &str,
    rationale: &str,
    receipt: &GateReceipt,
    verdict: &crate::harness::prompt_catalog::ReviewVerdictV1,
) -> Result<Option<(String, Vec<u8>, RecordRevisionCheck)>, StoreError> {
    snapshot.validate().map_err(StoreError::MalformedState)?;
    let path = review_path(snapshot);
    let mut record = read_review(store, &path, snapshot)?.ok_or_else(|| {
        StoreError::MalformedState("arbitration requires an existing plan-review record".into())
    })?;
    let original = record
        .arbitrations
        .last()
        .map(|arbitration| &arbitration.verdict)
        .or(record.verdict.as_ref())
        .ok_or_else(|| {
            StoreError::MalformedState("arbitration requires a persisted review verdict".into())
        })?;
    if record
        .arbitrations
        .iter()
        .filter(|arbitration| arbitration.finding_id == finding_id)
        .count()
        >= 2
    {
        return Err(StoreError::MalformedState(
            "bounded arbitration attempts for this finding are exhausted".into(),
        ));
    }
    if verdict.outcome == Some(crate::harness::prompt_catalog::ReviewOutcomeV1::ReviewUnable) {
        return Err(StoreError::MalformedState(
            "ReviewUnable cannot be persisted as an arbitration result".into(),
        ));
    }
    if !original
        .findings
        .iter()
        .any(|finding| finding.id == finding_id)
    {
        return Err(StoreError::MalformedState(
            "arbitration finding is not present in the persisted review".into(),
        ));
    }
    if !eligible_evidence_id(evidence_id) {
        return Err(StoreError::MalformedState(
            "arbitration requires eligible plan, decision, readiness, or product-spec evidence"
                .into(),
        ));
    }
    if !receipt
        .evidence_refs
        .iter()
        .any(|reference| reference == evidence_id)
    {
        return Err(StoreError::MalformedState(
            "arbitration receipt does not cite its contested evidence".into(),
        ));
    }
    if evidence_sha256.len() != 64
        || !evidence_sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
        || source_quote.trim().len() < 8
        || rationale.trim().len() < 32
    {
        return Err(StoreError::MalformedState(
            "arbitration evidence provenance is incomplete".into(),
        ));
    }
    validate_plan_review(snapshot, receipt)?;
    validate_verdict(snapshot, receipt, verdict).map_err(StoreError::MalformedState)?;
    let expected_revision = record.revision;
    record.revision += 1;
    record.updated_at_ms = super::super::now_ms();
    record.arbitrations.push(PlanReviewArbitration {
        finding_id: finding_id.to_owned(),
        evidence_id: evidence_id.to_owned(),
        evidence_sha256: evidence_sha256.to_owned(),
        source_quote: source_quote.to_owned(),
        rationale: rationale.to_owned(),
        receipt: receipt.clone(),
        verdict: verdict.clone(),
    });
    let bytes = serde_json::to_vec_pretty(&record)
        .map_err(|error| StoreError::MalformedState(error.to_string()))?;
    Ok(Some((
        path.clone(),
        bytes,
        RecordRevisionCheck {
            path,
            expected_revision,
        },
    )))
}

#[cfg(test)]
mod tests {
    use super::eligible_evidence_id;

    #[test]
    fn evidence_id_requires_a_nonempty_approved_source() {
        for id in [
            "plan:snapshot-1",
            "decision:question-1",
            "feature-decisions:feature-1",
            "readiness:feature-1",
            "product-spec:quality",
        ] {
            assert!(eligible_evidence_id(id), "expected eligible ID: {id}");
        }
        for id in ["plan:", "decision:  ", "workspace-summary", "opportunity:1"] {
            assert!(!eligible_evidence_id(id), "expected ineligible ID: {id}");
        }
    }
}
