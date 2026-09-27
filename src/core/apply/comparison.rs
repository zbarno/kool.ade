use super::ApplyReceipt;
use crate::core::state::PlannerState;

pub(super) fn apply(
    state: &mut PlannerState,
    comparison: &crate::domain::PlanComparison,
) -> anyhow::Result<ApplyReceipt> {
    let (id, body) = state
        .active_feature
        .as_ref()
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("Plan comparison has no feature target"))?;
    let path =
        crate::artifacts::product_docs::document_path(&state.repo_root, &format!("feature:{id}"))?;
    anyhow::ensure!(
        crate::artifacts::read_utf8_lossy(&path)? == body,
        "Feature changed before the plan comparison was saved"
    );
    let updated = crate::domain::ChangeMetadata::save_plan_comparison(&body, comparison.clone())?;
    crate::artifacts::atomic_write(&path, &updated)?;
    if let Some((_, feature_body)) = state
        .active_features
        .iter_mut()
        .find(|(feature_id, _)| feature_id == &id)
    {
        *feature_body = updated.clone();
    }
    state.active_feature = Some((id.clone(), updated));
    Ok(ApplyReceipt {
        spec_written: false,
        items_written: false,
        commit_message: format!("planner: compare implementation plans for {id}"),
        repo_relative_paths: vec![
            path.strip_prefix(&state.repo_root)?
                .to_string_lossy()
                .replace('\\', "/"),
        ],
        synthesized_open_items: Vec::new(),
    })
}
