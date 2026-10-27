use crate::app::session::Project;

mod code_review;
mod runtime_qa_evidence;
mod task_relations;
mod ticket;

pub(super) use ticket::for_ticket;
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
    use super::{phase_index, ticket::specification_deltas_for_task};
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
