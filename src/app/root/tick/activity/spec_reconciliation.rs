use crate::{
    app::{root::configured_harness_for, session::Project},
    core::lifecycle::LifecycleStore,
};
use sha2::Digest as _;

mod feature;

pub(in crate::app::root) fn advance_spec_reconciliation(
    project: &mut Project,
    task_harness: &mut Option<Box<dyn crate::harness::AiHarness>>,
) {
    if let Some(controller) = project.activity.spec_reconciliation.as_ref() {
        let Some(result) = controller.take_result() else {
            return;
        };
        let task_uid = controller.task_uid.clone();
        let expected_revision = controller.expected_revision;
        let spec_sha256 = controller.spec_sha256.clone();
        let post_completion_pr_refresh = controller.post_completion_pr_refresh;
        project.activity.spec_reconciliation = None;
        finish(
            project,
            task_uid,
            expected_revision,
            spec_sha256,
            post_completion_pr_refresh,
            result,
        );
        return;
    }

    if feature::advance_feature_spec_reconciliation(project, task_harness) {
        return;
    }

    let candidates =
        match LifecycleStore::new(&project.state.planning_store).spec_reconciliation_candidates() {
            Ok(candidates) => candidates,
            Err(error) => {
                project.activity.pending.push(format!(
                    "Specification reconciliation queue could not be read: {error}"
                ));
                return;
            }
        };
    for mut task in candidates {
        let lifecycle = LifecycleStore::new(&project.state.planning_store);
        match lifecycle.list_spec_sync_queue(&task.feature_uid, false) {
            Ok(entries) if entries.iter().any(|entry| entry.source_task_uid.is_none()) => continue,
            Err(error) => {
                project.activity.pending.push(format!(
                    "Task specification scope could not be read: {error}"
                ));
                continue;
            }
            _ => {}
        }
        let current_spec_sha256 = match crate::core::lifecycle::product_spec::current_product_hash(
            &project.state.planning_store,
        ) {
            Ok(hash) => hash,
            Err(error) => {
                project.activity.pending.push(format!(
                    "Current specification identity could not be computed: {error}"
                ));
                return;
            }
        };
        if task.spec_reconciliation_blocker.is_some() {
            if task.spec_reconciliation_spec_sha256.as_deref() == Some(current_spec_sha256.as_str())
            {
                continue;
            }
            let retry_event = event_uid(
                &task.task_uid,
                task.revision,
                "spec-reconciliation-retry-v1",
                &current_spec_sha256,
            );
            match LifecycleStore::new(&project.state.planning_store).retry_spec_reconciliation(
                &task.task_uid,
                task.revision,
                &retry_event,
                current_spec_sha256.clone(),
            ) {
                Ok(updated) => task = updated,
                Err(error) => {
                    project.activity.pending.push(format!(
                        "Specification reconciliation for task {} could not resume after the current spec changed: {error}",
                        task.task_uid
                    ));
                    continue;
                }
            }
        }
        let harness = configured_harness_for(
            task_harness,
            Some(crate::persistence::harness_settings::PLAN_REVIEW),
        );
        let route = harness.label();
        match crate::app::root::spec_reconciliation::SpecReconciliation::start(
            project, &task, harness,
        ) {
            Ok(controller) => {
                project.activity.spec_reconciliation = Some(controller);
                project.live_progress = crate::harness::LiveProgress {
                    activity: Some(format!(
                        "Reconciling verified implementation against the approved specification with {route}…"
                    )),
                    selected_route: Some(route),
                    ..Default::default()
                };
            }
            Err(reason) => {
                let proposal_sha256 = hash_text(&reason);
                record_blocker(
                    project,
                    &task.task_uid,
                    task.revision,
                    &current_spec_sha256,
                    proposal_sha256,
                    reason,
                    None,
                );
            }
        }
        return;
    }
}

pub(crate) mod finish;
use finish::{finish, record_blocker};

fn hash_text(value: &str) -> String {
    format!("{:x}", sha2::Sha256::digest(value.as_bytes()))
}

fn stable_uid(task_uid: &str, name: &str) -> String {
    let namespace = uuid::Uuid::parse_str(task_uid).unwrap_or(uuid::Uuid::NAMESPACE_OID);
    uuid::Uuid::new_v5(&namespace, name.as_bytes())
        .hyphenated()
        .to_string()
}

fn event_uid(task_uid: &str, revision: u64, label: &str, binding: &str) -> String {
    stable_uid(task_uid, &format!("{label}:{revision}:{binding}"))
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

#[cfg(test)]
#[path = "spec_reconciliation/review_regressions.rs"]
mod review_regressions;
