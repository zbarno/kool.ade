use super::{arbitration_attempt_count, route_contest_to_repair};

pub(crate) fn recover_persisted_arbitration(
    project: &crate::app::session::Project,
    work: &mut crate::core::planning_work::Work,
) -> bool {
    let Some(feature_id) = work.feature_id.clone() else {
        return false;
    };
    let Some(expected_hash) = work
        .detail
        .lines()
        .find_map(|line| line.strip_prefix("Snapshot hash: "))
    else {
        return false;
    };
    let Some(markdown) = project
        .state
        .active_features
        .iter()
        .find(|(id, _)| id == &feature_id)
        .map(|(_, markdown)| markdown)
    else {
        return false;
    };
    let Ok(Some(identity)) = crate::domain::ArtifactIdentity::from_markdown(markdown) else {
        return false;
    };
    let contract = crate::core::workflow::feature_contract(markdown);
    let Ok(snapshot) = crate::core::lifecycle::plan_snapshot::from_feature_with_author(
        &project.state.planning_store,
        &feature_id,
        &identity,
        markdown,
        &contract,
    ) else {
        return false;
    };
    if snapshot.content_hash != expected_hash
        || !crate::core::lifecycle::plan_snapshot::is_persisted(
            &project.state.planning_store,
            &snapshot,
        )
        .unwrap_or(false)
    {
        return false;
    }
    let Ok(Some((selected_finding, receipt))) =
        crate::core::lifecycle::plan_snapshot::latest_plan_arbitration(
            &project.state.planning_store,
            &snapshot,
        )
    else {
        return false;
    };
    let Ok(Some(verdict)) = crate::core::lifecycle::plan_snapshot::plan_review_verdict(
        &project.state.planning_store,
        &snapshot,
    ) else {
        return false;
    };
    if receipt.subject_sha256 != snapshot.content_hash {
        return false;
    }
    let finding_id = work.key.rsplit(':').next().unwrap_or_default();
    let finding_remains = verdict
        .findings
        .iter()
        .any(|finding| finding.id == finding_id);
    work.status = if finding_remains {
        crate::core::planning_work::WorkStatus::NeedsAttention
    } else {
        crate::core::planning_work::WorkStatus::Done
    };
    if selected_finding == finding_id {
        work.detail.push_str(&format!(
            "\n\nIndependent arbitration receipt: {}\nArbitration snapshot hash: {}\nContested finding remains: {}",
            receipt.receipt_uid, snapshot.content_hash, finding_remains,
        ));
        if finding_remains
            && arbitration_attempt_count(&work.detail)
                >= crate::app::manager::MAX_PLAN_ARBITRATION_ATTEMPTS
        {
            route_contest_to_repair(work, &feature_id);
        }
    } else {
        work.detail.push_str(&format!(
            "\n\nEffective arbitration receipt: {}\nArbitration snapshot hash: {}\nContested finding remains: {}",
            receipt.receipt_uid, snapshot.content_hash, finding_remains,
        ));
    }
    true
}
