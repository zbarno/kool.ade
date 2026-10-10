use super::*;

pub(super) fn prompt(
    state: &PlannerState,
    candidate: &Candidate,
    evidence: &str,
) -> anyhow::Result<String> {
    let feature_path = crate::artifacts::product_docs::document_path(
        &state.planning_store,
        &format!("feature:{}", candidate.feature_id),
    )?;
    let current_feature =
        String::from_utf8(state.planning_store.read_planning_path(&feature_path)?)?;
    let mut modules = String::new();
    for id in candidate.contract.product_modules.keys() {
        let path = crate::artifacts::product_docs::document_path(
            &state.planning_store,
            &format!("product:{id}"),
        )?;
        modules.push_str(&format!(
            "\n=== product:{id} ===\n{}\n",
            String::from_utf8(state.planning_store.read_planning_path(&path)?)?
        ));
    }
    Ok(format!(
        "Reconcile the approved feature with ACTUAL MERGED implementation. Inspect the merged commits in their target repositories using read-only git show/diff; the current checkout may not contain those commits. The product specification must describe only resulting current truth. If implementation matches the approved feature, return full replacements for affected product modules and the feature specification, set that document update's typed status to `implemented`, and include the merged commit references. Preserve the approved feature's normative sections verbatim. If there is a material disagreement, do NOT change product modules: set the feature update's typed status to `reconciliation` and create one Review or Human open item with feature_id, recommendation, evidence, and a question describing the discrepancy. The `status` field is machine-authoritative; the visible Status line is rendered from it. Do not silently rewrite intent. Return schema_version 2 JSON with assistant_message, document_updates, open_items_added, open_items_updated=[], open_items_resolved=[], next_question_id=null, updated_specification=null. Only product IDs in the affected modules and feature:{} are writable. All changes are validated as one transaction.\n\n=== APPROVED FEATURE ===\n{}\n\n=== CURRENT FEATURE ===\n{}\n\n=== AFFECTED PRODUCT MODULES ===\n{}\n\n=== MERGED IMPLEMENTATION EVIDENCE ===\n{}\n",
        candidate.feature_id,
        candidate.contract.feature_specification,
        current_feature,
        modules,
        evidence
    ))
}

pub(super) fn validate_response(
    state: &PlannerState,
    candidate: &Candidate,
    response: &crate::harness::responses::ReconciliationResponse,
) -> anyhow::Result<validation::NormalizedTurn> {
    anyhow::ensure!(
        response
            .open_items_updated
            .as_deref()
            .unwrap_or_default()
            .is_empty()
            && response
                .open_items_resolved
                .as_deref()
                .unwrap_or_default()
                .is_empty()
            && response.next_question_id.is_none(),
        "Reconciliation may only update scoped documents and add discrepancy items"
    );
    let updates = response.document_updates.as_deref().unwrap_or_default();
    let feature_key = format!("feature:{}", candidate.feature_id);
    let feature_update = updates
        .iter()
        .find(|update| update.document_id == feature_key)
        .ok_or_else(|| anyhow::anyhow!("Reconciliation must update the feature status"))?;
    for update in updates {
        if update.document_id == feature_key {
            continue;
        }
        let Some(id) = update.document_id.strip_prefix("product:") else {
            anyhow::bail!("Reconciliation wrote an unrelated document");
        };
        anyhow::ensure!(
            candidate.contract.product_modules.contains_key(id),
            "Reconciliation wrote an unaffected product module"
        );
    }
    anyhow::ensure!(
        workflow::feature_contract(&feature_update.content)
            == workflow::feature_contract(&candidate.contract.feature_specification),
        "Reconciliation changed the approved feature contract"
    );
    let implemented = feature_update.status == Some(crate::domain::ChangeStatus::Implemented);
    let discrepancy = feature_update.status == Some(crate::domain::ChangeStatus::Reconciliation);
    anyhow::ensure!(
        implemented || discrepancy,
        "Feature must become Implemented or Reconciliation"
    );
    if implemented {
        anyhow::ensure!(
            updates
                .iter()
                .any(|update| update.document_id.starts_with("product:")),
            "Implemented feature must reconcile affected product modules"
        );
        anyhow::ensure!(
            response
                .open_items_added
                .as_deref()
                .unwrap_or_default()
                .is_empty(),
            "Implemented feature cannot add an unresolved discrepancy"
        );
        for task in &candidate.tasks {
            anyhow::ensure!(
                feature_update
                    .content
                    .contains(task.merged_commit.as_deref().unwrap()),
                "Feature implementation references omit a merged commit"
            );
        }
    } else {
        anyhow::ensure!(
            updates.len() == 1,
            "Discrepancy must not rewrite product truth"
        );
        let items = response.open_items_added.as_deref().unwrap_or_default();
        anyhow::ensure!(
            items.len() == 1
                && items[0].feature_id.as_deref() == Some(&candidate.feature_id)
                && matches!(
                    items[0].authority.as_deref(),
                    Some("Review" | "Human" | "review" | "human")
                ),
            "Discrepancy requires one review or human board item for this feature"
        );
    }
    validation::validate(
        &TurnEnvelope::from(response.clone()),
        state,
        &state.effective_user(),
    )
    .map_err(|problems| anyhow::anyhow!(problems.join("; ")))
}
