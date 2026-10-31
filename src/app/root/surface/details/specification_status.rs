use crate::app::root::Screen;
use crate::artifacts::planning_store::PlanningRoot;

#[cfg(test)]
#[path = "specification_status/tests.rs"]
mod tests;

macro_rules! specification_status_surface_methods {
    () => {
        fn specification_trace_navigation(
            &self,
        ) -> Option<crate::core::lifecycle::SpecTraceNavigationV1> {
            $crate::app::root::surface::details::specification_status::trace_navigation(
                &self.screen,
            )
        }
        fn requirements_for_specification_reference(
            &self,
            feature_uid: &str,
            reference: &str,
        ) -> Vec<crate::core::lifecycle::SpecRequirementTraceV1> {
            $crate::app::root::surface::details::specification_status::requirements_for_reference(
                &self.screen,
                feature_uid,
                reference,
            )
        }
        fn product_specification_status(
            &self,
        ) -> Result<Option<crate::core::lifecycle::ProductSpecAggregateV1>, String> {
            $crate::app::root::surface::details::specification_status::product_status(&self.screen)
        }
        fn feature_specification_status(
            &self,
            markdown: &str,
        ) -> Result<Option<crate::ui::FeatureSpecificationStatus>, String> {
            $crate::app::root::surface::details::specification_status::for_markdown(
                &self.screen,
                markdown,
            )
        }
    };
}
pub(in crate::app::root::surface) use specification_status_surface_methods;

pub(in crate::app::root::surface) fn trace_navigation(
    screen: &Screen,
) -> Option<crate::core::lifecycle::SpecTraceNavigationV1> {
    let Screen::Connected(project) = screen else {
        return None;
    };
    project.specification_trace_navigation.clone()
}

pub(in crate::app::root::surface) fn requirements_for_reference(
    screen: &Screen,
    feature_uid: &str,
    reference: &str,
) -> Vec<crate::core::lifecycle::SpecRequirementTraceV1> {
    let Screen::Connected(project) = screen else {
        return Vec::new();
    };
    crate::core::lifecycle::LifecycleStore::new(&project.state.planning_store)
        .requirements_for_specification_reference(feature_uid, reference)
        .unwrap_or_default()
}

pub(in crate::app::root::surface) fn product_status(
    screen: &Screen,
) -> Result<Option<crate::core::lifecycle::ProductSpecAggregateV1>, String> {
    let Screen::Connected(project) = screen else {
        return Ok(None);
    };
    crate::core::lifecycle::LifecycleStore::new(&project.state.planning_store)
        .product_specification_aggregate()
        .map(Some)
        .map_err(|error| error.to_string())
}

pub(in crate::app::root::surface) fn for_markdown(
    screen: &Screen,
    markdown: &str,
) -> Result<Option<crate::ui::FeatureSpecificationStatus>, String> {
    let Screen::Connected(project) = screen else {
        return Ok(None);
    };
    let Some(identity) = crate::domain::ArtifactIdentity::from_markdown(markdown)
        .map_err(|error| error.to_string())?
    else {
        return Ok(None);
    };
    let current_spec_hash =
        crate::core::lifecycle::product_spec::current_product_hash(&project.state.planning_store)
            .ok();
    let lifecycle = crate::core::lifecycle::LifecycleStore::new(&project.state.planning_store);
    let feature = match lifecycle.load_feature(&identity.uid) {
        Ok(feature) => feature,
        Err(crate::artifacts::planning_store::StoreError::Io { source, .. })
            if source.kind() == std::io::ErrorKind::NotFound =>
        {
            return Ok(Some(crate::ui::FeatureSpecificationStatus {
                current_spec_hash,
                drift_status: "baseline not recorded",
                ..Default::default()
            }));
        }
        Err(error) => return Err(error.to_string()),
    };
    let revision = lifecycle
        .load_spec_revision(&identity.uid)
        .map_err(|error| error.to_string())?;
    let pending_triggers = lifecycle
        .list_spec_sync_queue(&identity.uid, false)
        .map_err(|error| error.to_string())?
        .len();
    let drift_status = match revision.as_ref() {
        None => "baseline not recorded",
        Some(revision) if current_spec_hash.as_deref() != Some(revision.content_hash.as_str()) => {
            "drift detected"
        }
        Some(revision)
            if !matches!(
                revision.reconciliation_status,
                crate::core::lifecycle::SpecReconciliationStatusV1::Synchronized
                    | crate::core::lifecycle::SpecReconciliationStatusV1::OutstandingNonblockingFindings
            ) => "needs decision",
        Some(_) => "current",
    };
    let current_implementation = lifecycle
        .load_current_implementation(&identity.uid)
        .map_err(|error| error.to_string())?;
    let deployment = lifecycle
        .load_deployment_evidence(&identity.uid)
        .map_err(|error| error.to_string())?;
    let deployment_status = deployment
        .as_ref()
        .zip(current_implementation.as_ref())
        .filter(|(deployment, current)| deployment.matches_current(current))
        .map(|(deployment, _)| {
            deployment.state.clone().unwrap_or_else(|| {
                if deployment.deployment_id.is_some() {
                    "deployment found; status unavailable".into()
                } else {
                    "no matching production deployment".into()
                }
            })
        });
    let deployment_observed_at_ms = deployment
        .as_ref()
        .zip(current_implementation.as_ref())
        .filter(|(deployment, current)| deployment.matches_current(current))
        .map(|(deployment, _)| deployment.observed_at_ms);
    let deployed_spec_hash = deployment
        .as_ref()
        .zip(current_implementation.as_ref())
        .and_then(|(deployment, current)| deployment.deployed_spec_hash(current))
        .map(str::to_owned);
    let mut status = crate::ui::FeatureSpecificationStatus {
        feature_uid: Some(identity.uid.clone()),
        current_spec_hash,
        drift_status,
        approved_target_hash: feature.approved_plan_sha256,
        revision: revision.as_ref().map(|revision| revision.seq),
        reconciliation_status: revision
            .as_ref()
            .map(|revision| revision.reconciliation_status),
        outcome_reason: revision.and_then(|revision| revision.outcome_reason),
        pending_triggers,
        current_implementation,
        current_implementation_stale: false,
        deployment_status,
        deployment_observed_at_ms,
        deployed_spec_hash,
        ..Default::default()
    };
    if let Some(current) = status.current_implementation.as_ref() {
        let registered =
            crate::core::project_repos::ProjectManifest::load(&project.state.planning_store)
                .and_then(|manifest| {
                    manifest.target_if_available(
                        project.state.planning_store.project_id,
                        project.state.planning_store.planning_layout().root(),
                        &current.repository_id,
                    )
                })
                .map_err(|error| error.to_string())?;
        status.current_implementation_stale = match registered {
            Some(repository) => !checkout_matches_current_evidence(&repository, current),
            None => true,
        };
    }
    if let Some(revision) = lifecycle
        .spec_revision_history(&identity.uid)
        .map_err(|error| error.to_string())?
        .into_iter()
        .last()
    {
        status.feature_deltas = revision.1;
    }
    for task in lifecycle
        .list_tasks()
        .map_err(|error| error.to_string())?
        .into_iter()
        .filter(|task| task.feature_uid == identity.uid)
    {
        if task.code_review.as_ref().is_some_and(|review| {
            review.decision == crate::core::lifecycle::GateDecision::Approved
                && task.reviewed_code_head_sha.is_some()
        }) && let Some(head) = task.reviewed_code_head_sha
        {
            status.reviewed_heads.push((task.task_uid.clone(), head));
        }
        if let Some(work_uid) = task.pr_work_item_uid.as_deref() {
            let work = lifecycle
                .load_pr_preparation(work_uid)
                .map_err(|error| error.to_string())?;
            if let Some(commit) = work.merged_commit_sha {
                status.merged_commits.push((task.task_uid, commit));
            }
        }
    }
    status.reviewed_heads.sort();
    status.merged_commits.sort();
    Ok(Some(status))
}

fn checkout_matches_current_evidence(
    repository: &std::path::Path,
    evidence: &crate::core::lifecycle::CurrentImplementationEvidenceV1,
) -> bool {
    let Ok(mut head) = crate::core::planning_sync::validated_git_command(repository) else {
        return false;
    };
    let Ok(output) = head.args(["rev-parse", "HEAD"]).output() else {
        return false;
    };
    if !output.status.success()
        || String::from_utf8_lossy(&output.stdout).trim() != evidence.checked_out_head_sha
    {
        return false;
    }
    let Ok(mut status) = crate::core::planning_sync::validated_git_command(repository) else {
        return false;
    };
    let Ok(output) = status
        .args(["status", "--porcelain", "--untracked-files=all"])
        .output()
    else {
        return false;
    };
    output.status.success() && output.stdout.is_empty()
}
