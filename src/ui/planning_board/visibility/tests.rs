use super::*;
use crate::{
    core::{
        planning::*,
        planning_work::{WorkKind, WorkStatus},
    },
    domain::{ItemKind, ItemStatus, OpenItem, Priority},
};

fn question(feature_uid: &str) -> QuestionV1 {
    propose_questions(
        vec![QuestionProposal {
            feature_uid: feature_uid.into(),
            plan_revision: 1,
            decision_subject: "retention".into(),
            decision_domain: "data ownership".into(),
            origin_requirement_ids: vec!["REQ-1".into()],
            category: QuestionCategory::Data,
            priority: QuestionPriority::Blocking,
            question: "Which retention policy applies?".into(),
            reason: "Planning needs the policy.".into(),
            options: vec![QuestionOption {
                id: "keep".into(),
                label: "Keep existing".into(),
                tradeoffs: "No migration".into(),
            }],
            recommended_option_id: Some("keep".into()),
            no_answer_impact: "Planning waits".into(),
            evidence_refs: vec![],
            assumption_if_deferred: None,
            depends_on_question_ids: vec![],
        }],
        &[],
    )
    .unwrap()
    .remove(0)
}

#[test]
fn board_total_includes_archived_owned_work_until_the_parent_is_archived() {
    let mut root = Work::new(
        "task:parent".into(),
        "Request".into(),
        "Keep work visible".into(),
        String::new(),
    );
    root.kind = WorkKind::Question;
    root.task_intent_uid = Some(root.uid.clone());
    root.status = WorkStatus::Done;
    let mut child = Work::new(
        "task:child".into(),
        "Record a decision".into(),
        "Keep its history".into(),
        String::new(),
    );
    child.task_intent_uid = Some(root.uid.clone());
    child.parent_uid = Some(root.uid.clone());
    child.status = WorkStatus::Done;
    let mut decision = OpenItem::new(
        "CLR-001".into(),
        Priority::Normal,
        ItemKind::Question,
        "Product".into(),
        "Choose a policy".into(),
        String::new(),
    );
    decision.task_intent_uid = Some(root.uid.clone());
    decision.status = ItemStatus::Resolved;
    let mut board = ViewModel {
        planning_work: vec![root, child],
        planning_items: vec![decision],
        ..Default::default()
    };
    board.archived.insert("task:child".into());
    board.archived.insert("CLR-001".into());
    assert_eq!(board.visible_card_count(&[]), 3);
    board.planning_work[0].task_intent_archived_at_ms = Some(1);
    assert_eq!(board.visible_card_count(&[]), 0);
}

#[test]
fn finished_clarifications_remain_on_the_board_in_done() {
    let uid = uuid::Uuid::new_v4().to_string();
    let mut board = ViewModel::default();
    for (status, column) in [
        (QuestionStatus::Draft, 3),
        (QuestionStatus::Open, 3),
        (QuestionStatus::Declined, 3),
        (QuestionStatus::Deferred, 3),
        (QuestionStatus::Answered, 4),
        (QuestionStatus::EvidenceResolved, 4),
        (QuestionStatus::Superseded, 4),
        (QuestionStatus::Cancelled, 4),
    ] {
        let mut item = question(&uid);
        item.status = status;
        board.clarification_questions = vec![item];
        assert_eq!(question_column(status), column);
        assert_eq!(
            board.visible_card_count(&[]),
            1,
            "each question outcome remains discoverable"
        );
    }
}

#[test]
fn feature_linked_cards_require_one_immutable_intent_owner() {
    let uid = uuid::Uuid::new_v4().to_string();
    let mut root = Work::new(
        "task:feature".into(),
        "A request".into(),
        "One owner".into(),
        String::new(),
    );
    root.task_intent_uid = Some(root.uid.clone());
    root.feature_uid = Some(uid.clone());
    let mut board = ViewModel {
        planning_work: vec![root.clone()],
        ..Default::default()
    };
    assert_eq!(board.intent_for_feature(&uid).unwrap().uid, root.uid);
    assert!(
        board
            .intent_for_feature(&uuid::Uuid::new_v4().to_string())
            .is_none()
    );
    let mut another = root;
    another.uid = uuid::Uuid::new_v4().to_string();
    another.task_intent_uid = Some(another.uid.clone());
    board.planning_work.push(another);
    assert!(board.intent_for_feature(&uid).is_none());
}

#[test]
fn historical_question_stays_with_its_request_after_a_follow_up_cycle() {
    let old = uuid::Uuid::new_v4().to_string();
    let mut root = Work::new(
        "task:request".into(),
        "Request".into(),
        String::new(),
        String::new(),
    );
    root.kind = WorkKind::Question;
    root.status = WorkStatus::Done;
    root.task_intent_uid = Some(root.uid.clone());
    root.feature_uid = Some(uuid::Uuid::new_v4().to_string());
    let mut child = Work::new(
        "task:old-cycle".into(),
        "Earlier cycle".into(),
        String::new(),
        String::new(),
    );
    child.task_intent_uid = Some(root.uid.clone());
    child.parent_uid = Some(root.uid.clone());
    child.feature_uid = Some(old.clone());
    child.status = WorkStatus::Done;
    let mut board = ViewModel {
        planning_work: vec![root, child],
        clarification_questions: vec![question(&old)],
        ..Default::default()
    };
    assert!(
        board.intent_for_feature(&old).is_none(),
        "historical ownership cannot authorize current approval"
    );
    assert_eq!(
        board.question_owner(&old).unwrap().uid,
        board.planning_work[0].uid
    );
    assert_eq!(board.visible_card_count(&[]), 3);
    board.planning_work[0].task_intent_archived_at_ms = Some(1);
    assert!(!board.question_visible(&board.clarification_questions[0]));
    assert_eq!(board.visible_card_count(&[]), 0);
}
