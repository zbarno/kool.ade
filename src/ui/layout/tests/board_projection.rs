use super::super::*;
#[test]
fn human_and_review_items_are_projected_into_needs_attention() {
    let mut item = crate::domain::OpenItem::new(
        "CLR-001".into(),
        crate::domain::Priority::High,
        crate::domain::ItemKind::Question,
        "General".into(),
        None,
        "Choose a retention period".into(),
        String::new(),
    );
    assert_eq!(planning_column(&item, std::slice::from_ref(&item)), 3);
    item.authority = crate::domain::Authority::Review;
    assert_eq!(planning_column(&item, std::slice::from_ref(&item)), 3);
    item.authority = crate::domain::Authority::Agent;
    assert_eq!(planning_column(&item, std::slice::from_ref(&item)), 0);
    item.status = crate::domain::ItemStatus::Resolved;
    assert_eq!(planning_column(&item, std::slice::from_ref(&item)), 4);
}

#[test]
fn dependent_decision_waits_until_its_prerequisite_is_resolved() {
    let parent = crate::domain::OpenItem::new(
        "CLR-010".into(),
        crate::domain::Priority::High,
        crate::domain::ItemKind::Question,
        "General".into(),
        None,
        "Choose the account model".into(),
        String::new(),
    );
    let mut child = crate::domain::OpenItem::new(
        "CLR-011".into(),
        crate::domain::Priority::High,
        crate::domain::ItemKind::Question,
        "General".into(),
        None,
        "Choose the recovery flow".into(),
        String::new(),
    );
    child.blocked_by = vec![parent.id.clone()];
    let mut items = vec![parent, child];
    assert_eq!(planning_column(&items[1], &items), 0);
    assert_eq!(
        crate::core::routing::eligible_items(
            &items,
            &crate::domain::CurrentUser::new("Operator", vec![]),
            &crate::domain::Stakeholders::default(),
        )
        .iter()
        .map(|item| item.id.as_str())
        .collect::<Vec<_>>(),
        vec!["CLR-010"]
    );
    items[0].status = crate::domain::ItemStatus::Resolved;
    assert_eq!(planning_column(&items[1], &items), 3);
    assert!(
        crate::core::routing::eligible_items(
            &items,
            &crate::domain::CurrentUser::new("Operator", vec![]),
            &crate::domain::Stakeholders::default(),
        )
        .iter()
        .any(|item| item.id == "CLR-011")
    );
}

#[test]
fn child_work_names_the_blocking_parent() {
    let mut parent = crate::core::planning_work::Work::new(
        "task:parent".into(),
        "Plan access behavior".into(),
        "Access behavior".into(),
        String::new(),
    );
    parent.status = crate::core::planning_work::WorkStatus::NeedsAttention;
    let mut child = crate::core::planning_work::Work::new(
        "task:child".into(),
        "Plan account recovery".into(),
        "Account recovery".into(),
        String::new(),
    );
    child.parent_uid = Some(parent.uid.clone());
    assert_eq!(
        planning_parent_label(&child, &[&parent]),
        Some("Blocked by Plan access behavior".into())
    );
}
