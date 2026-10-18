use super::*;
use sha2::Digest;

/// Persist an independent review only when it still refers to the exact
/// currently confirmed plan snapshot. The receipt is create-only per snapshot.
pub fn record_plan_review_if_current<R: crate::artifacts::planning_store::PlanningRoot + ?Sized>(
    repo: &R,
    id: &str,
    receipt: &crate::core::lifecycle::GateReceipt,
    verdict: &crate::harness::prompt_catalog::ReviewVerdictV1,
    expected_revision: &str,
) -> anyhow::Result<String> {
    let _guard = crate::core::writer_gate::acquire();
    let store = repo.planning_store();
    let path = crate::artifacts::product_docs::document_path(repo, &format!("feature:{id}"))?;
    let text = String::from_utf8(repo.read_planning_path(&path)?)?;
    let contract = feature_contract(&text);
    anyhow::ensure!(
        crate::persistence::feature_approvals::contains(&store.git_root(), id, &contract),
        "plan review requires current user confirmation"
    );
    let identity = crate::domain::ArtifactIdentity::from_markdown(&text)?
        .ok_or_else(|| anyhow::anyhow!("feature has no stable artifact identity"))?;
    let snapshot = crate::core::lifecycle::plan_snapshot::from_feature_with_author(
        &store, id, &identity, &text, &contract,
    )
    .map_err(anyhow::Error::msg)?;
    anyhow::ensure!(
        crate::core::lifecycle::plan_snapshot::has_user_confirmation(&store, &snapshot)?,
        "plan review requires a persisted user confirmation receipt for this exact snapshot"
    );
    anyhow::ensure!(
        crate::core::lifecycle::plan_snapshot::is_persisted(&store, &snapshot)?,
        "reviewed plan snapshot is not persisted"
    );
    let Some((record_path, bytes, check)) =
        crate::core::lifecycle::plan_snapshot::review_change_with_verdict(
            &store, &snapshot, receipt, verdict,
        )?
    else {
        return Ok(store.revision()?);
    };
    let (paths, revision) = store.transaction_with_revision_and_record_revisions(
        &[(record_path, bytes)],
        Some(expected_revision),
        &[check],
    )?;
    let result = crate::core::gitops::commit(
        &store.git_root(),
        &format!("planner: record independent plan review {id}"),
        &paths
            .iter()
            .map(|path| store.git_path(path))
            .collect::<Vec<_>>(),
    );
    result.map_err(|error| anyhow::anyhow!("review saved but checkpoint failed: {error}"))?;
    Ok(revision)
}

/// Persist arbitration as a distinct durable review result, fenced to the same
/// confirmed snapshot and planning-store revision as its cited evidence.
#[allow(clippy::too_many_arguments)]
pub fn record_plan_arbitration_if_current<
    R: crate::artifacts::planning_store::PlanningRoot + ?Sized,
>(
    repo: &R,
    id: &str,
    finding_id: &str,
    evidence_id: &str,
    evidence_sha256: &str,
    evidence_content: &str,
    source_quote: &str,
    rationale: &str,
    receipt: &crate::core::lifecycle::GateReceipt,
    verdict: &crate::harness::prompt_catalog::ReviewVerdictV1,
    expected_revision: &str,
) -> anyhow::Result<String> {
    let _guard = crate::core::writer_gate::acquire();
    let store = repo.planning_store();
    anyhow::ensure!(
        store.revision()? == expected_revision,
        "arbitration evidence changed during review"
    );
    anyhow::ensure!(
        format!("{:x}", sha2::Sha256::digest(evidence_content.as_bytes())) == evidence_sha256
            && evidence_content.contains(source_quote),
        "arbitration source quote does not match its captured evidence hash"
    );
    anyhow::ensure!(
        receipt
            .evidence_refs
            .iter()
            .any(|reference| reference == evidence_id),
        "arbitration receipt must cite the exact evidence used for its decision"
    );
    let path = crate::artifacts::product_docs::document_path(repo, &format!("feature:{id}"))?;
    let text = String::from_utf8(repo.read_planning_path(&path)?)?;
    let contract = feature_contract(&text);
    anyhow::ensure!(
        crate::persistence::feature_approvals::contains(&store.git_root(), id, &contract),
        "arbitration requires current user confirmation"
    );
    let identity = crate::domain::ArtifactIdentity::from_markdown(&text)?
        .ok_or_else(|| anyhow::anyhow!("feature has no stable artifact identity"))?;
    let snapshot = crate::core::lifecycle::plan_snapshot::from_feature_with_author(
        &store, id, &identity, &text, &contract,
    )
    .map_err(anyhow::Error::msg)?;
    anyhow::ensure!(
        crate::core::lifecycle::plan_snapshot::has_user_confirmation(&store, &snapshot)?,
        "arbitration requires a persisted user confirmation receipt for this exact snapshot"
    );
    let Some((record_path, bytes, check)) =
        crate::core::lifecycle::plan_snapshot::arbitration_change(
            &store,
            &snapshot,
            finding_id,
            evidence_id,
            evidence_sha256,
            source_quote,
            rationale,
            receipt,
            verdict,
        )?
    else {
        return Ok(store.revision()?);
    };
    let (paths, revision) = store.transaction_with_revision_and_record_revisions(
        &[(record_path, bytes)],
        Some(expected_revision),
        &[check],
    )?;
    let committed = crate::core::gitops::commit(
        &store.git_root(),
        &format!("planner: record plan arbitration {id}"),
        &paths
            .iter()
            .map(|path| store.git_path(path))
            .collect::<Vec<_>>(),
    );
    committed
        .map_err(|error| anyhow::anyhow!("arbitration saved but checkpoint failed: {error}"))?;
    Ok(revision)
}
