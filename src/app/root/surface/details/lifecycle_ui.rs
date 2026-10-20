use crate::{app::session::Project, core::lifecycle::LifecycleStore};
mod task_relations;
use task_relations::relation_for_planning_uid;

mod runtime_qa_evidence;

pub(super) fn for_ticket(
    project: &Project,
    ticket: &str,
) -> (
    Option<crate::ui::task_detail::LifecycleView>,
    Option<String>,
) {
    let Some(document) = project
        .task_documents
        .iter()
        .find(|document| document.path == ticket)
    else {
        return (None, None);
    };
    let Some(planning_item_uid) = document
        .identity
        .as_ref()
        .map(|identity| identity.uid.as_str())
    else {
        return (None, None);
    };
    let lifecycle = LifecycleStore::new(&project.state.planning_store);
    let task = match lifecycle.task_for_planning_item(planning_item_uid) {
        Ok(Some(task)) => task,
        Ok(None) => return (None, None),
        Err(error) => return (None, Some(error.to_string())),
    };
    let feature = match lifecycle.load_feature(&task.feature_uid) {
        Ok(feature) => feature,
        Err(error) => return (None, Some(error.to_string())),
    };
    let spec_revision = match lifecycle.load_spec_revision(&task.feature_uid) {
        Ok(revision) => revision,
        Err(error) => return (None, Some(error.to_string())),
    };
    let pending_spec_triggers = match lifecycle.list_spec_sync_queue(&task.feature_uid, false) {
        Ok(entries) => entries.len(),
        Err(error) => return (None, Some(error.to_string())),
    };
    let parent_task = task
        .parent_task_uid
        .as_deref()
        .and_then(|parent_uid| lifecycle.load_task(parent_uid).ok())
        .and_then(|parent| {
            relation_for_planning_uid(project, Some(parent.planning_item_uid.as_deref()?))
        });
    let dependencies = document
        .metadata
        .as_ref()
        .into_iter()
        .flat_map(|metadata| metadata.dependency_uids.iter())
        .filter_map(|uid| relation_for_planning_uid(project, Some(uid)))
        .collect();
    let subtasks = project
        .task_documents
        .iter()
        .filter(|candidate| {
            candidate.metadata.as_ref().is_some_and(|metadata| {
                metadata.parent_task_uid.as_deref() == Some(planning_item_uid)
            })
        })
        .filter_map(|child| {
            relation_for_planning_uid(
                project,
                child
                    .metadata
                    .as_ref()
                    .map(|metadata| metadata.uid.as_str()),
            )
        })
        .collect();
    let specification_deltas = match lifecycle.spec_revision_history(&task.feature_uid) {
        Ok(history) => specification_deltas_for_task(history, &task.task_uid),
        Err(error) => return (None, Some(error.to_string())),
    };
    let pr_preparation = task
        .pr_work_item_uid
        .as_deref()
        .map(|uid| lifecycle.load_pr_preparation(uid))
        .transpose();
    let pr_preparation = match pr_preparation {
        Ok(preparation) => preparation,
        Err(error) => return (None, Some(error.to_string())),
    };
    let current_spec_hash =
        crate::core::lifecycle::product_spec::current_product_hash(&project.state.planning_store)
            .ok();
    let (runtime_qa_evidence, runtime_qa_evidence_error) =
        runtime_qa_evidence::for_task(project, &task);
    let current_implementation = match lifecycle.load_current_implementation(&task.feature_uid) {
        Ok(current) => current,
        Err(error) => return (None, Some(error.to_string())),
    };
    let deployment = match lifecycle.load_deployment_evidence(&task.feature_uid) {
        Ok(deployment) => deployment,
        Err(error) => return (None, Some(error.to_string())),
    };
    let deployed_spec_hash = deployment
        .as_ref()
        .zip(current_implementation.as_ref())
        .and_then(|(deployment, current)| deployment.deployed_spec_hash(current))
        .map(str::to_owned);
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
    (
        Some(crate::ui::task_detail::LifecycleView {
            feature_uid: task.feature_uid.clone(),
            feature_phase: feature.phase,
            task_phase: task.phase,
            parent_task_uid: task.parent_task_uid.clone(),
            parent_task,
            dependencies,
            subtasks,
            task_revision: task.revision,
            code_review: task.code_review,
            runtime_qa: task.runtime_qa,
            runtime_qa_run_id: task
                .qa_execution
                .as_ref()
                .filter(|execution| {
                    execution.status == crate::core::lifecycle::QaExecutionStatusV1::Completed
                })
                .and_then(|execution| execution.request.as_ref())
                .map(|request| request.qa_run_id.clone()),
            spec_reconciliation: task.spec_reconciliation,
            current_spec_hash,
            implementation_head_sha: spec_revision
                .as_ref()
                .and_then(|revision| revision.implementation_head_sha.clone())
                .or(task.reviewed_code_head_sha),
            approved_target_hash: spec_revision
                .as_ref()
                .and_then(|revision| revision.approved_target_hash.clone())
                .or(feature.approved_plan_sha256),
            deployed_spec_hash,
            deployment_status,
            deployment_observed_at_ms: deployment
                .as_ref()
                .zip(current_implementation.as_ref())
                .filter(|(deployment, current)| deployment.matches_current(current))
                .map(|(deployment, _)| deployment.observed_at_ms),
            spec_revision: spec_revision.as_ref().map(|revision| revision.seq),
            spec_status: spec_revision.map(|revision| revision.reconciliation_status),
            pending_spec_triggers,
            specification_deltas,
            pull_request_status: pr_preparation.as_ref().map(|pr| pr.status),
            merged_commit_sha: pr_preparation.and_then(|pr| pr.merged_commit_sha),
            runtime_qa_evidence,
            runtime_qa_evidence_error,
            runtime_qa_applicability: task
                .qa_execution
                .as_ref()
                .and_then(|execution| execution.applicability.clone()),
        }),
        None,
    )
}

fn specification_deltas_for_task(
    history: Vec<(
        crate::core::lifecycle::SpecRevisionV1,
        Vec<crate::core::lifecycle::SpecDeltaRecordV1>,
    )>,
    task_uid: &str,
) -> Vec<(u64, crate::core::lifecycle::SpecDeltaRecordV1)> {
    history
        .into_iter()
        .flat_map(|(revision, deltas)| {
            deltas
                .into_iter()
                .filter(|delta| {
                    delta.affected_task_ids.iter().any(|uid| uid == task_uid)
                        || delta.followup_ids.iter().any(|uid| uid == task_uid)
                })
                .map(move |delta| (revision.seq, delta))
        })
        .collect()
}

pub(in crate::app::root::surface) fn board_phases(
    project: &Project,
    tasks: &[crate::core::lifecycle::TaskRecord],
) -> (
    std::collections::BTreeMap<String, crate::core::lifecycle::TaskPhase>,
    Option<String>,
) {
    let mappings = tasks.iter().filter_map(|task| {
        task.planning_item_uid
            .as_ref()
            .map(|planning_item_uid| (planning_item_uid.clone(), task.phase))
    });
    let (phases, has_ambiguous) = phase_index(mappings);
    let by_path = project
        .task_documents
        .iter()
        .filter_map(|document| {
            let planning_item_uid = document.identity.as_ref()?.uid.as_str();
            let Some(Some(phase)) = phases.get(planning_item_uid) else {
                return None;
            };
            Some((document.path.clone(), *phase))
        })
        .collect();
    let error = has_ambiguous.then(|| {
        "Lifecycle badges are hidden for planning items with multiple lifecycle task records."
            .to_owned()
    });
    (by_path, error)
}

fn phase_index(
    mappings: impl IntoIterator<Item = (String, crate::core::lifecycle::TaskPhase)>,
) -> (
    std::collections::BTreeMap<String, Option<crate::core::lifecycle::TaskPhase>>,
    bool,
) {
    let mut phases = std::collections::BTreeMap::new();
    let mut ambiguous = false;
    for (planning_item_uid, phase) in mappings {
        match phases.entry(planning_item_uid) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(Some(phase));
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                entry.insert(None);
                ambiguous = true;
            }
        }
    }
    (phases, ambiguous)
}

#[cfg(test)]
mod tests {
    use super::{phase_index, specification_deltas_for_task};
    use crate::core::lifecycle::{
        SpecDeltaClassificationV1, SpecDeltaRecordV1, SpecRevisionV1, TaskPhase,
    };

    fn delta(affected: Vec<&str>, followups: Vec<&str>) -> SpecDeltaRecordV1 {
        SpecDeltaRecordV1 {
            requirement_id: "REQ-TRACE".into(),
            event_id: "event-uid".into(),
            old_target: Some("old".into()),
            proposed_target: Some("new".into()),
            observed_current: None,
            classification: SpecDeltaClassificationV1::UserDecision,
            evidence_refs: vec!["doc:trace".into()],
            decision_refs: Vec::new(),
            test_refs: Vec::new(),
            affected_task_ids: affected.into_iter().map(str::to_owned).collect(),
            needs_user_reapproval: true,
            followup_ids: followups.into_iter().map(str::to_owned).collect(),
            operator_decision: None,
        }
    }

    fn revision(
        seq: u64,
        deltas: Vec<SpecDeltaRecordV1>,
    ) -> (SpecRevisionV1, Vec<SpecDeltaRecordV1>) {
        (
            SpecRevisionV1 {
                schema_version: 1,
                uid: "feature-uid".into(),
                feature_uid: "feature-uid".into(),
                revision: seq,
                seq,
                content_hash: "hash".into(),
                source_revision: 1,
                produced_by_run_id: None,
                origin_events: Vec::new(),
                decision_refs: Vec::new(),
                approved_target_hash: None,
                implementation_head_sha: None,
                reconciliation_status:
                    crate::core::lifecycle::SpecReconciliationStatusV1::NeedsHumanDecision,
                outcome_reason: None,
                created_at_ms: 1,
                updated_at_ms: 1,
            },
            deltas,
        )
    }

    #[test]
    fn task_delta_projection_links_both_affected_and_followup_tasks() {
        let affected_task = "00000000-0000-4000-8000-000000000001";
        let followup_task = "00000000-0000-4000-8000-000000000002";
        let history = vec![
            revision(2, vec![delta(vec![affected_task], vec![])]),
            revision(3, vec![delta(vec![], vec![followup_task])]),
        ];
        let affected = specification_deltas_for_task(history.clone(), affected_task);
        assert_eq!(affected.len(), 1);
        assert_eq!(affected[0].0, 2);
        let followup = specification_deltas_for_task(history, followup_task);
        assert_eq!(followup.len(), 1);
        assert_eq!(followup[0].0, 3);
        assert_eq!(followup[0].1.requirement_id, "REQ-TRACE");
    }

    #[test]
    fn duplicate_planning_item_mappings_are_marked_ambiguous() {
        let (phases, ambiguous) = phase_index([
            ("item-1".into(), TaskPhase::AwaitingReview),
            ("item-1".into(), TaskPhase::PrMerged),
        ]);
        assert!(ambiguous);
        assert_eq!(phases.get("item-1"), Some(&None));
    }

    #[test]
    fn distinct_planning_items_keep_their_lifecycle_phase() {
        let (phases, ambiguous) = phase_index([
            ("item-1".into(), TaskPhase::AwaitingReview),
            ("item-2".into(), TaskPhase::PrMerged),
        ]);
        assert!(!ambiguous);
        assert_eq!(phases.get("item-1"), Some(&Some(TaskPhase::AwaitingReview)));
        assert_eq!(phases.get("item-2"), Some(&Some(TaskPhase::PrMerged)));
    }
}
