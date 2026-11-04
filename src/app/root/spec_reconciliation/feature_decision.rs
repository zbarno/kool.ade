use crate::{
    app::session::Project,
    artifacts::product_docs,
    core::lifecycle::{
        FeatureDeltaAuthorizationV1, LifecycleStore, SpecDeltaOperatorChoiceV1,
        SpecDeltaOperatorDecisionV1, SpecReconciliationStatusV1, SpecRevisionDraftV1,
    },
};
use sha2::Digest as _;

#[cfg(test)]
#[path = "feature_decision/tests.rs"]
mod tests;

pub(crate) fn decide_feature_delta(
    project: &mut Project,
    feature_uid: &str,
    requirement_id: &str,
    expected_sequence: u64,
    adopt: bool,
) -> Result<(), String> {
    let store = &project.state.planning_store;
    let expected_store_revision = store.revision().map_err(|error| error.to_string())?;
    let lifecycle = LifecycleStore::new(store);
    let feature = lifecycle
        .load_feature(feature_uid)
        .map_err(|error| error.to_string())?;
    let history = lifecycle
        .spec_revision_history(feature_uid)
        .map_err(|error| error.to_string())?;
    let (current, mut deltas) = history
        .last()
        .cloned()
        .ok_or("No feature reconciliation proposal is recorded")?;
    if current.seq != expected_sequence {
        return Err("The feature proposal changed; reload it before deciding.".into());
    }
    let current_hash = crate::core::lifecycle::product_spec::current_product_hash(store)
        .map_err(|error| error.to_string())?;
    if current_hash != current.content_hash {
        return Err(
            "The product specification changed since reconciliation; refresh before deciding."
                .into(),
        );
    }
    let delta = deltas
        .iter_mut()
        .find(|delta| delta.requirement_id == requirement_id && delta.affected_task_ids.is_empty())
        .ok_or("No taskless proposal exists for this requirement")?;
    if delta.operator_decision.is_some() {
        return Err("This feature proposal already has an operator decision.".into());
    }
    let receipt_uid = uuid::Uuid::new_v4().hyphenated().to_string();
    let mut related_changes = Vec::new();
    let mut updated_spec = None;
    let resulting_hash = if adopt {
        let old = delta
            .old_target
            .as_deref()
            .ok_or("The proposal has no prior target text")?;
        let new = delta
            .proposed_target
            .as_deref()
            .ok_or("The proposal has no new target text")?;
        if !old.contains(requirement_id) || !new.contains(requirement_id) {
            return Err("The proposal does not preserve its requirement identity.".into());
        }
        let documents = product_docs::load_documents(store)
            .map_err(|error| error.to_string())?
            .ok_or("The product specification documents are unavailable")?;
        let matches = documents
            .iter()
            .filter(|doc| doc.content.match_indices(old).count() > 0)
            .count();
        let exact_matches = documents
            .iter()
            .map(|doc| doc.content.match_indices(old).count())
            .sum::<usize>();
        if matches != 1 || exact_matches != 1 {
            return Err(
                "The prior target must match exactly one product specification location.".into(),
            );
        }
        let mut updates = Vec::new();
        for document in documents {
            let next = document.content.replacen(old, new, 1);
            if next != document.content {
                product_docs::validate_module(&next).map_err(|error| error.to_string())?;
                updates.push((format!("product:{}", document.module.id), next));
            }
        }
        if updates.len() != 1 {
            return Err("The approved target must update exactly one product document.".into());
        }
        related_changes = crate::core::apply::private_document_draft_changes(store, &updates)
            .map_err(|error| error.to_string())?;
        let mut effective_updates = crate::core::apply::load_current(store)
            .map_err(|error| error.to_string())?
            .map(|draft| {
                draft
                    .changes
                    .into_iter()
                    .filter(|change| change.document_id.starts_with("product:"))
                    .map(|change| (change.document_id, change.content))
                    .collect::<std::collections::BTreeMap<_, _>>()
            })
            .unwrap_or_default();
        for (id, content) in &updates {
            effective_updates.insert(id.clone(), content.clone());
        }
        let effective_updates = effective_updates.into_iter().collect::<Vec<_>>();
        let updated = product_docs::render_product_with_updates(store, &effective_updates)
            .map_err(|error| error.to_string())?
            .ok_or("The updated product specification could not be rendered")?;
        delta.needs_user_reapproval = false;
        let encoded =
            serde_json::to_vec(&Some(updated.clone())).map_err(|error| error.to_string())?;
        let hash = format!("{:x}", sha2::Sha256::digest(encoded));
        updated_spec = Some(updated);
        Some(hash)
    } else {
        None
    };
    let recorded_at_ms = now_ms();
    delta.operator_decision = Some(SpecDeltaOperatorDecisionV1 {
        receipt_uid: receipt_uid.clone(),
        decision: if adopt {
            SpecDeltaOperatorChoiceV1::Adopted
        } else {
            SpecDeltaOperatorChoiceV1::Declined
        },
        actor: crate::core::lifecycle::GateActor::User,
        recorded_at_ms,
        resulting_spec_sha256: resulting_hash.clone(),
    });
    // The product hash includes the private proposal overlay. It stays bound
    // to the same published-spec pin until the proposal is reviewed and merged.
    let content_hash = resulting_hash
        .clone()
        .unwrap_or_else(|| current_hash.clone());
    let decision = if adopt {
        SpecDeltaOperatorChoiceV1::Adopted
    } else {
        SpecDeltaOperatorChoiceV1::Declined
    };
    let authorization = FeatureDeltaAuthorizationV1 {
        schema_version: 1,
        receipt_uid: receipt_uid.clone(),
        feature_uid: feature_uid.to_owned(),
        source_spec_sequence: expected_sequence,
        requirement_id: delta.requirement_id.clone(),
        event_uid: delta.event_id.clone(),
        old_target_sha256: delta
            .old_target
            .as_deref()
            .map(|text| format!("{:x}", sha2::Sha256::digest(text.as_bytes()))),
        proposed_target_sha256: delta
            .proposed_target
            .as_deref()
            .map(|text| format!("{:x}", sha2::Sha256::digest(text.as_bytes()))),
        resulting_product_sha256: resulting_hash.clone(),
        decision,
        actor: crate::core::lifecycle::GateActor::User,
        recorded_at_ms,
    };
    let unresolved = deltas.iter().any(|delta| delta.operator_decision.is_none());
    let status = if unresolved {
        SpecReconciliationStatusV1::NeedsHumanDecision
    } else {
        SpecReconciliationStatusV1::Synchronized
    };
    let mut decisions = current.decision_refs.clone();
    decisions.push(receipt_uid);
    let draft = SpecRevisionDraftV1 {
        no_change_reason: None,
        content_hash: content_hash.clone(),
        source_revision: feature.revision,
        produced_by_run_id: None,
        origin_events: current.origin_events.clone(),
        decision_refs: decisions,
        delta_approval_refs: Vec::new(),
        approved_target_hash: resulting_hash.or(current.approved_target_hash.clone()),
        implementation_head_sha: current.implementation_head_sha.clone(),
        reconciliation_status: status,
        outcome_reason: unresolved
            .then(|| "Other feature reconciliation findings still need a decision.".to_owned()),
        deltas,
    };
    lifecycle
        .append_spec_revision_with_feature_authorization(
            feature_uid,
            expected_sequence,
            draft,
            Some(&expected_store_revision),
            authorization,
            related_changes,
        )
        .map_err(|error| error.to_string())?;
    if updated_spec.is_some() {
        project.state.local_spec_draft =
            crate::core::apply::load_current(store).map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}
