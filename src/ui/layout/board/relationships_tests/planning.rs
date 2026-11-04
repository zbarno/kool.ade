use super::*;

#[test]
fn task_map_planning_work_projection_follows_explicit_parent_uids() {
    let mut parent = crate::core::planning_work::Work::new(
        "feature:F12".into(),
        "Feature planning".into(),
        "Plan feature".into(),
        String::new(),
    );
    parent.feature_uid = Some("feature-uid".into());
    let mut child = crate::core::planning_work::Work::new(
        "question:F12".into(),
        "Release decision".into(),
        "Resolve decision".into(),
        String::new(),
    );
    child.parent_uid = Some(parent.uid.clone());
    let unrelated = crate::core::planning_work::Work::new(
        "feature:F13".into(),
        "Unrelated feature".into(),
        "Plan another feature".into(),
        String::new(),
    );

    let relations = planning_work_relations(&[parent.clone(), child.clone(), unrelated]);
    let related = &relations["feature-uid"];
    assert_eq!(related.len(), 2);
    assert!(related.iter().any(|node| node.0 == parent.uid));
    assert!(
        related
            .iter()
            .any(|node| node.0 == child.uid && node.4.as_deref() == Some(parent.uid.as_str()))
    );
}

#[test]
fn ambiguous_planning_work_uids_are_not_navigable() {
    let original = crate::core::planning_work::Work::new(
        "feature:F12".into(),
        "Feature planning".into(),
        "Plan feature".into(),
        String::new(),
    );
    let mut duplicate = original.clone();
    duplicate.key = "different-record".into();
    let keys = unique_planning_work_keys(&[original.clone(), duplicate.clone()]);

    assert!(!keys.contains_key(&original.uid));
    assert!(keys.is_empty());
}

#[test]
fn task_map_planning_items_follow_blocker_ids_and_current_authority() {
    use crate::domain::item::{Authority, ItemKind, ItemStatus, OpenItem, Priority};
    let mut prerequisite = OpenItem::new(
        "CLR-001".into(),
        Priority::Blocking,
        ItemKind::Question,
        "Product".into(),
        "Choose a policy".into(),
        "The implementation depends on it".into(),
    );
    prerequisite.feature_uid = Some("feature-uid".into());
    prerequisite.status = ItemStatus::Resolved;
    let mut decision = OpenItem::new(
        "CLR-002".into(),
        Priority::High,
        ItemKind::Ambiguity,
        "Product".into(),
        "Confirm the fallback".into(),
        "The chosen policy defines fallback behavior".into(),
    );
    decision.blocked_by = vec![prerequisite.id.clone()];
    decision.authority = Authority::Review;
    let unrelated = OpenItem::new(
        "CLR-003".into(),
        Priority::Normal,
        ItemKind::Question,
        "General".into(),
        "Unrelated question".into(),
        "No relation to this feature".into(),
    );

    let relations = planning_item_relations(
        &[prerequisite.clone(), decision.clone(), unrelated],
        &std::collections::HashMap::new(),
        |_| false,
    );
    let related = &relations["feature-uid"];
    assert_eq!(related.len(), 2);
    assert!(
        related
            .iter()
            .any(|node| node.planning_item_id == prerequisite.id && node.status == "Resolved")
    );
    assert!(related.iter().any(|node| {
        node.planning_item_id == decision.id
            && node.status == "Needs user review"
            && node.blocked_by == vec![prerequisite.id.clone()]
    }));

    let mut saved_decision = decision.clone();
    saved_decision.feature_uid = Some("feature-uid".into());
    let answer_saved =
        planning_item_relations(&[saved_decision], &std::collections::HashMap::new(), |_| {
            true
        });
    assert_eq!(answer_saved["feature-uid"][0].status, "Answer saved");
}

#[test]
fn legacy_feature_id_links_are_visible_and_feature_uid_conflicts_are_flagged() {
    use crate::domain::item::{ItemKind, OpenItem, Priority};
    let mut feature = crate::core::planning_work::Work::new(
        "feature:F12".into(),
        "Feature planning".into(),
        "Plan feature".into(),
        String::new(),
    );
    feature.feature_id = Some("F12".into());
    feature.feature_uid = Some("feature-uid".into());
    let feature_uids = unique_feature_uids_by_id(&[feature]);
    let mut legacy = OpenItem::new(
        "CLR-010".into(),
        Priority::Normal,
        ItemKind::Question,
        "Product".into(),
        "Legacy linked question".into(),
        "Old artifact has no feature UID".into(),
    );
    legacy.feature_id = Some("F12".into());
    let mut conflict = OpenItem::new(
        "CLR-011".into(),
        Priority::Normal,
        ItemKind::Question,
        "Product".into(),
        "Conflicting feature reference".into(),
        "Its UID and feature ID disagree".into(),
    );
    conflict.feature_id = Some("F12".into());
    conflict.feature_uid = Some("different-feature-uid".into());

    let relations = planning_item_relations(&[legacy, conflict.clone()], &feature_uids, |_| false);
    let linked = &relations["feature-uid"];
    assert!(linked.iter().any(|node| node.planning_item_id == "CLR-010"
        && node.legacy_feature_link
        && !node.feature_reference_conflict));
    assert!(
        linked.iter().any(|node| {
            node.planning_item_id == conflict.id && node.feature_reference_conflict
        })
    );
    assert!(
        relations["different-feature-uid"]
            .iter()
            .any(|node| node.planning_item_id == conflict.id && node.feature_reference_conflict)
    );
}

#[test]
fn ambiguous_planning_item_ids_are_not_navigable() {
    let item = crate::domain::item::OpenItem::new(
        "CLR-001".into(),
        crate::domain::Priority::Normal,
        crate::domain::ItemKind::Question,
        "General".into(),
        "One question".into(),
        "Context".into(),
    );
    assert!(unique_planning_item_keys(&[item.clone(), item.clone()]).is_empty());
}

#[test]
fn task_map_projects_only_explicit_opportunity_feature_and_task_uids() {
    let feature_uid = uuid::Uuid::new_v4().to_string();
    let task_uid = uuid::Uuid::new_v4().to_string();
    let opportunity_uid = uuid::Uuid::new_v4().to_string();
    let mut board = crate::ui::planning_board::ViewModel::default();
    board.opportunities.push((
        opportunity_fixture(&opportunity_uid, &feature_uid, &task_uid),
        1,
    ));
    let relations = opportunity_relations(&board);

    let feature_relation = &relations[&feature_uid][0];
    assert_eq!(feature_relation.relation, "Explicit feature relation");
    assert_eq!(feature_relation.status, "Awaiting decision");
    assert!(feature_relation.navigable);
    assert_eq!(
        feature_relation.conversation_key,
        crate::core::task_conversation::opportunity_conversation_key(&opportunity_uid)
    );
    assert_eq!(relations[&task_uid][0].relation, "Explicit task relation");
    assert!(!relations.contains_key("unrelated-feature"));

    board.opportunities.push((
        opportunity_fixture(&opportunity_uid, &feature_uid, &task_uid),
        2,
    ));
    assert!(!opportunity_relations(&board)[&feature_uid][0].navigable);
}

fn opportunity_fixture(
    uid: &str,
    feature_uid: &str,
    task_uid: &str,
) -> crate::core::opportunity::OpportunityV1 {
    use crate::core::opportunity::*;
    OpportunityV1 {
        schema_version: 1,
        opportunity_uid: uid.into(),
        workspace_uid: uuid::Uuid::new_v4().to_string(),
        origin: OpportunityOriginV1::ReviewFinding,
        kind: OpportunityKindV1::Quality,
        target: OpportunityTargetV1 {
            kind: OpportunityTargetKindV1::ActiveUserProject,
            repository_uid: uuid::Uuid::new_v4().to_string(),
            canonical_remote: "https://github.com/example/project".into(),
            source_sha: "a".repeat(40),
        },
        related_feature_uids: vec![feature_uid.into()],
        related_task_uids: vec![task_uid.into()],
        discovered_at_ms: 1,
        observation_revision: 1,
        evidence_refs: Vec::new(),
        problem_or_opportunity: "Improve task recovery".into(),
        user_value: "Tasks recover reliably".into(),
        expected_outcome: "Recovery evidence is visible".into(),
        possible_approaches: vec!["Show the persisted state".into()],
        recommended_approach: "Show persisted evidence".into(),
        alternatives: Vec::new(),
        estimated_scope: OpportunityScopeV1::Small,
        impact: "Clearer recovery".into(),
        cost_or_risk: "Small UI change".into(),
        affected_systems: vec!["task details".into()],
        current_capability_state: "Links are not shown".into(),
        generic_workaround: None,
        proposed_approach: "Add a typed map relation".into(),
        dedup_fingerprint: "b".repeat(64),
        status: OpportunityStatusV1::AwaitingDecision,
        revision: 1,
    }
}
