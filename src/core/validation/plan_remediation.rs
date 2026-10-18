use crate::core::lifecycle::{PlanFindingActionV1, PlanRemediationV1};
use crate::core::state::PlannerState;
use crate::harness::TurnEnvelope;

pub(super) fn validate(
    remediation: &PlanRemediationV1,
    envelope: &TurnEnvelope,
    state: &PlannerState,
    purpose: crate::core::workflow::TurnPurpose,
    user_message: &str,
) -> Result<(String, String), String> {
    remediation.validate_shape()?;
    if purpose != crate::core::workflow::TurnPurpose::PlanRemediation {
        return Err("plan remediation is only valid in a Manager planning turn".into());
    }
    let (feature_id, markdown) = state
        .active_features
        .iter()
        .find(|(_, markdown)| {
            crate::domain::ArtifactIdentity::from_markdown(markdown)
                .ok()
                .flatten()
                .is_some_and(|identity| identity.uid == remediation.feature_uid)
        })
        .ok_or_else(|| "plan remediation references an inactive feature".to_owned())?;
    let identity = crate::domain::ArtifactIdentity::from_markdown(markdown)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "plan remediation feature identity is missing".to_owned())?;
    let snapshot = crate::core::lifecycle::plan_snapshot::from_feature_with_author(
        &state.planning_store,
        feature_id,
        &identity,
        markdown,
        &crate::core::workflow::feature_contract(markdown),
    )?;
    if remediation.snapshot_uid != snapshot.uid
        || remediation.snapshot_hash != snapshot.content_hash
    {
        return Err("plan remediation refers to a stale candidate".into());
    }
    if let Some((requested_feature, requested_hash)) = remediation_request(user_message)
        && (requested_feature != feature_id || requested_hash != snapshot.content_hash)
    {
        return Err("plan remediation does not match the requested candidate".into());
    }
    let receipt = crate::core::lifecycle::plan_snapshot::plan_review_receipt(
        &state.planning_store,
        &snapshot,
    )
    .map_err(|error| error.to_string())?
    .ok_or_else(|| "plan remediation has no persisted reviewer receipt".to_owned())?;
    if receipt.receipt_uid != remediation.review_receipt_uid
        || receipt.decision != crate::core::lifecycle::GateDecision::ChangesRequested
    {
        return Err("plan remediation does not match the current changes-requested review".into());
    }
    let verdict = crate::core::lifecycle::plan_snapshot::plan_review_verdict(
        &state.planning_store,
        &snapshot,
    )
    .map_err(|error| error.to_string())?
    .ok_or_else(|| "plan remediation has no persisted reviewer verdict".to_owned())?;
    let expected = verdict
        .findings
        .iter()
        .map(|finding| finding.id.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    let actual = remediation
        .dispositions
        .iter()
        .map(|disposition| disposition.finding_id.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    if expected.is_empty() || actual != expected {
        return Err(
            "plan remediation must disposition every current reviewer finding exactly once".into(),
        );
    }
    for disposition in &remediation.dispositions {
        match &disposition.action {
            PlanFindingActionV1::RepairPlan { .. }
                if !envelope.document_updates.as_ref().is_some_and(|updates| {
                    updates.iter().any(|update| {
                        update.document_id == format!("feature:{feature_id}")
                            && crate::core::specification::normalize_feature_headings(
                                &update.content,
                            ) != markdown.as_str()
                    })
                }) =>
            {
                return Err(format!(
                    "finding {} selected RepairPlan without changing the feature specification",
                    disposition.finding_id
                ));
            }
            PlanFindingActionV1::AskUser { question, .. }
                if !envelope.added().iter().any(|item| {
                    item.question.as_deref() == Some(question.as_str())
                        && item.feature_id.as_deref() == Some(feature_id.as_str())
                        && matches!(item.authority.as_deref(), Some("Human" | "Review"))
                }) =>
            {
                return Err(format!(
                    "finding {} selected AskUser without creating its feature question",
                    disposition.finding_id
                ));
            }
            PlanFindingActionV1::PromoteFollowUp { title, .. }
                if !envelope.follow_up_task.as_ref().is_some_and(|offer| {
                    offer.title.trim() == title.trim()
                        && offer.description.contains(&disposition.finding_id)
                }) =>
            {
                return Err(format!(
                    "finding {} selected PromoteFollowUp without a matching follow-up task offer",
                    disposition.finding_id
                ));
            }
            PlanFindingActionV1::ContestFinding { evidence_refs, .. }
                if !envelope.added().iter().any(|item| {
                    item.feature_id.as_deref() == Some(feature_id.as_str())
                        && item.authority.as_deref() == Some("Review")
                        && item.question.as_deref().is_some_and(|question| {
                            question.contains(&disposition.finding_id)
                                && question.to_ascii_lowercase().contains("arbitrat")
                        })
                }) =>
            {
                return Err(format!(
                    "finding {} selected ContestFinding without requesting a feature-scoped Review arbitration",
                    disposition.finding_id
                ));
            }
            PlanFindingActionV1::ContestFinding { evidence_refs, .. }
                if evidence_refs.iter().any(|reference| {
                    !receipt
                        .evidence_refs
                        .iter()
                        .any(|trusted| trusted == reference)
                }) =>
            {
                return Err(format!(
                    "finding {} contests the review with evidence outside the persisted reviewer evidence",
                    disposition.finding_id
                ));
            }
            _ => {}
        }
    }
    let path = format!("state/lifecycle_plan_remediations/{}.json", snapshot.uid);
    let content = serde_json::to_string_pretty(remediation).map_err(|error| error.to_string())?;
    match state.planning_store.read(&path) {
        Ok(existing) if existing != content.as_bytes() => {
            return Err(
                "a different Manager disposition is already persisted for this review".into(),
            );
        }
        Ok(_) => {}
        Err(crate::artifacts::planning_store::StoreError::Io { source, .. })
            if source.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.to_string()),
    }
    Ok((path, content))
}

pub(super) fn require_for_request(state: &PlannerState, user_message: &str) -> Result<(), String> {
    let Some((feature_id, requested_hash)) = remediation_request(user_message) else {
        return Ok(());
    };
    let (_, markdown) = state
        .active_features
        .iter()
        .find(|(id, _)| id == feature_id)
        .ok_or_else(|| "plan remediation request references an inactive feature".to_owned())?;
    let identity = crate::domain::ArtifactIdentity::from_markdown(markdown)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "plan remediation request feature identity is missing".to_owned())?;
    let snapshot = crate::core::lifecycle::plan_snapshot::from_feature_with_author(
        &state.planning_store,
        feature_id,
        &identity,
        markdown,
        &crate::core::workflow::feature_contract(markdown),
    )?;
    let receipt = crate::core::lifecycle::plan_snapshot::plan_review_receipt(
        &state.planning_store,
        &snapshot,
    )
    .map_err(|error| error.to_string())?;
    if snapshot.content_hash != requested_hash
        || !receipt.is_some_and(|receipt| {
            receipt.decision == crate::core::lifecycle::GateDecision::ChangesRequested
        })
    {
        return Err("plan remediation request is stale or has no changes-requested review".into());
    }
    Err(
        "Manager response must include typed plan_remediation for every persisted review finding"
            .into(),
    )
}

fn remediation_request(message: &str) -> Option<(&str, &str)> {
    let marker = message.strip_prefix("[KOOLADE_PLAN_REMEDIATION ")?;
    let marker = marker.split_once(']')?.0;
    let mut feature = None;
    let mut hash = None;
    for field in marker.split_ascii_whitespace() {
        if let Some(value) = field.strip_prefix("feature=") {
            feature = Some(value);
        } else if let Some(value) = field.strip_prefix("snapshot_hash=") {
            hash = Some(value);
        }
    }
    Some((feature?, hash?))
}
