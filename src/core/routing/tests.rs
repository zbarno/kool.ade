use super::*;
use crate::domain::{ItemKind, ItemStatus};

fn item(id: &str, authority: Authority, blocked_by: Vec<String>) -> OpenItem {
    let mut item = OpenItem::new(
        id.to_owned(),
        Priority::Blocking,
        ItemKind::Question,
        "General".into(),
        format!("Question {id}?"),
        "Needs a local decision.".into(),
    );
    item.authority = authority;
    item.blocked_by = blocked_by;
    item
}

#[test]
fn human_questions_route_to_local_user_without_assignment_configuration() {
    let user = CurrentUser::new("Local user");
    let question = item("Q-1", Authority::Human, Vec::new());
    assert_eq!(evaluate(&question, &user), Eligibility::LocalUser);
    assert_eq!(
        human_actionable_items(std::slice::from_ref(&question), &user).len(),
        1
    );
}

#[test]
fn agent_items_do_not_become_human_questions() {
    let user = CurrentUser::new("Local user");
    let agent_item = item("Q-1", Authority::Agent, Vec::new());
    assert_eq!(evaluate(&agent_item, &user), Eligibility::NotEligible);
    assert!(human_actionable_items(&[agent_item], &user).is_empty());
}

#[test]
fn open_prerequisites_block_questions_until_resolved() {
    let user = CurrentUser::new("Local user");
    let prerequisite = item("Q-1", Authority::Human, Vec::new());
    let dependent = item("Q-2", Authority::Human, vec!["Q-1".into()]);
    assert!(has_open_prerequisite(
        &[prerequisite.clone(), dependent.clone()],
        &dependent
    ));
    let mut resolved = prerequisite;
    resolved.status = ItemStatus::Resolved;
    assert!(!has_open_prerequisite(
        &[resolved, dependent.clone()],
        &dependent
    ));
    assert_eq!(recommended_next(&[dependent], &user).unwrap().id, "Q-2");
}
