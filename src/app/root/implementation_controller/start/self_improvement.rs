use crate::{
    artifacts::{planning_store::PlanningStore, task_docs::TaskDocument},
    core::{lifecycle::LifecycleStore, project_repos, self_improvement::SelfImprovementStore},
};

pub(super) fn authorize_task_start(
    planning: &PlanningStore,
    documents: &[TaskDocument],
    ticket: &str,
    code_repository_id: &str,
) -> Result<bool, String> {
    let has_feature_marker = documents
        .iter()
        .find(|document| document.path == ticket)
        .and_then(|document| {
            document
                .text
                .lines()
                .find_map(|line| line.strip_prefix("Feature ID: "))
        })
        .is_some();
    let lifecycle = LifecycleStore::new(planning);
    match lifecycle.task_for_planning_item(ticket) {
        Ok(Some(task)) => {
            let workspace_uid = planning.project_id.hyphenated().to_string();
            let target_repository_uid =
                project_repos::stable_uid(&workspace_uid, "repository", code_repository_id);
            let manifest = project_repos::ProjectManifest::load(planning)
                .map_err(|error| format!("Cannot verify task repository identity: {error}"))?;
            let repository_remote = manifest
                .repositories
                .iter()
                .find(|repository| repository.id == code_repository_id)
                .map(|repository| repository.remote.as_str())
                .ok_or_else(|| {
                    "Cannot verify task repository identity in the manifest".to_owned()
                })?;
            let runs = SelfImprovementStore::new(planning);
            let is_self_improvement = runs
                .run_for_feature(&task.feature_uid)
                .map_err(|error| format!("Cannot verify self-improvement run: {error}"))?
                .is_some();
            runs.authorize_task_start(
                &task.feature_uid,
                Some(&task.task_uid),
                &target_repository_uid,
                repository_remote,
            )
            .map_err(|error| {
                format!("Self-improvement target check blocked implementation: {error}")
            })?;
            Ok(is_self_improvement)
        }
        Ok(None) if has_feature_marker => {
            Err("Cannot verify lifecycle task mapping before implementation".into())
        }
        Ok(None) => Ok(false),
        Err(error) => Err(format!(
            "Cannot verify lifecycle identity before implementation: {error}"
        )),
    }
}

pub(super) fn publication_policy(
    is_self_improvement: bool,
    explicit_publish: bool,
    is_subtask: bool,
    allow_offline: bool,
    requested_changes: bool,
    auto_publish: bool,
    require_independent_checks: bool,
) -> (crate::core::implementation::PublicationMode, bool) {
    use crate::core::implementation::PublicationMode;

    let mode = if is_subtask || allow_offline || requested_changes {
        PublicationMode::HoldForReview
    } else if explicit_publish {
        PublicationMode::CreatePullRequest
    } else if auto_publish {
        PublicationMode::AutoPublish
    } else {
        PublicationMode::HoldForReview
    };
    let mode = if is_self_improvement && !explicit_publish {
        PublicationMode::HoldForReview
    } else {
        mode
    };
    let require_independent_checks =
        !allow_offline && (mode == PublicationMode::AutoPublish || require_independent_checks);
    (mode, require_independent_checks)
}

pub(super) fn start_message(
    allow_offline: bool,
    explicit_publish: bool,
    publication_mode: crate::core::implementation::PublicationMode,
    auto_build: bool,
) -> &'static str {
    use crate::core::implementation::PublicationMode;

    if allow_offline {
        "keep all work local without a shared claim; publishing requires a fresh coordinated attempt"
    } else if explicit_publish {
        "share the already verified changes for review"
    } else if publication_mode == PublicationMode::AutoPublish {
        "publish verified changes automatically"
    } else if auto_build {
        "verify the work and keep it local until you choose to publish"
    } else {
        "verify the work; no remote publication happens automatically"
    }
}

pub(super) fn record_start_notice(
    activity: &mut crate::app::manager::WorkspaceActivity,
    ticket: &str,
    allow_offline: bool,
) {
    activity.pending.push(format!("Assigned task {ticket} to an implementation worker. Verification and integration are managed by the queue."));
    if allow_offline {
        activity.pending.push(format!(
            "{ticket}: operator explicitly chose local-only execution; automatic publication is disabled until a new remote claim is acquired."
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::publication_policy;
    use crate::core::implementation::PublicationMode;

    #[test]
    fn self_improvement_never_inherits_auto_publish_without_explicit_pr_action() {
        assert_eq!(
            publication_policy(true, false, false, false, false, true, true),
            (PublicationMode::HoldForReview, true)
        );
        assert_eq!(
            publication_policy(true, true, false, false, false, true, true),
            (PublicationMode::CreatePullRequest, true)
        );
        assert_eq!(
            publication_policy(false, false, false, false, false, true, false),
            (PublicationMode::AutoPublish, true)
        );
    }
}
