use super::*;
use crate::core::lifecycle::{GateActor, GateDecision, GateReceipt};

#[path = "all_done_publication/fixture.rs"]
mod fixture;

struct ReviewReceiptContext<'a> {
    workspace_uid: &'a str,
    feature_uid: &'a str,
    task_uid: &'a str,
    code_hash: &'a str,
    author_run: &'a str,
    reviewer_run: &'a str,
    author_session: &'a str,
    reviewer_session: &'a str,
}

fn independent_receipt(
    role: crate::harness::prompt_catalog::PromptRole,
    context: ReviewReceiptContext<'_>,
    evidence_refs: Vec<String>,
) -> GateReceipt {
    let manifest =
        crate::harness::prompt_catalog::render(&crate::harness::prompt_catalog::RoleInputV1 {
            role,
            workspace_uid: context.workspace_uid.into(),
            feature_uid: context.feature_uid.into(),
            task_uid: Some(context.task_uid.into()),
            original_goal: "Complete the approved recovery task".into(),
            approved_spec_ref: None,
            approved_spec_hash: None,
            decision_refs: vec![],
            evidence_manifest: vec![],
            execution_policy_ref: "lifecycle-fixture/read-only/v1".into(),
            output_schema_version: role.output_schema_version().into(),
            payload: crate::harness::prompt_catalog::RolePayloadV1::Review {
                subject_revision: "fixture-revision".into(),
                subject_hash: context.code_hash.into(),
                author_run_id: context.author_run.into(),
                reviewer_run_id: context.reviewer_run.into(),
                author_session_id: Some(context.author_session.into()),
                reviewer_session_id: Some(context.reviewer_session.into()),
                requirements_matrix: vec!["The approved recovery behavior".into()],
                explicit_exclusions: vec![],
            },
        })
        .unwrap()
        .manifest;
    GateReceipt {
        receipt_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
        subject_uid: context.task_uid.into(),
        subject_sha256: context.code_hash.into(),
        actor: GateActor::IndependentReviewer,
        decision: GateDecision::Approved,
        recorded_at_unix_ms: 1,
        evidence_refs,
        prompt_sha256: Some(manifest.identity_sha256()),
        prompt_manifest: Some(manifest),
        reviewer_run_id: Some(context.reviewer_run.into()),
        author_run_id: Some(context.author_run.into()),
        reviewer_session_id: Some(context.reviewer_session.into()),
        author_session_id: Some(context.author_session.into()),
    }
}

/// Production rendering boundary: a Done TaskIntent and Done planning children
/// must still show the pending PR authorization action. The lifecycle reducer
/// tests gate transitions; this verifies production store projection and Board
/// painting. Local file stores and egui only; moderate setup. Snapshots carry
/// valid persisted receipts, but this test does not replay lifecycle events.
#[test]
fn done_planning_children_keep_pr_authorization_action_visible_on_board() {
    let (mut app, root_uid, task_uid, pr_uid) = fixture::build();
    let board = app.planning_board();
    let root = board
        .planning_work
        .iter()
        .find(|work| work.uid == root_uid)
        .unwrap();
    assert_eq!(
        board.task_intent_lifecycle(root),
        crate::core::task_intent::TaskIntentLifecycle::PreparingPullRequest,
        "root={:?}; feature={:?}; cycles={:?}; work={:?}; document owners={:?}; features={:?}; lifecycle tasks={:?}; phases={:?}; PR statuses={:?}; PR work={:?}",
        (root.status, root.feature_uid.as_deref()),
        root.kind,
        root.task_intent_cycles,
        board
            .planning_work
            .iter()
            .map(|work| (
                &work.uid,
                &work.parent_uid,
                &work.task_intent_uid,
                work.status,
                &work.task_intent_cycle_uid
            ))
            .collect::<Vec<_>>(),
        board
            .task_documents
            .iter()
            .map(|doc| (
                &doc.path,
                doc.metadata
                    .as_ref()
                    .and_then(|metadata| metadata.task_intent_uid.as_deref())
            ))
            .collect::<Vec<_>>(),
        board
            .lifecycle_features
            .iter()
            .map(|feature| (&feature.feature_uid, feature.phase))
            .collect::<Vec<_>>(),
        board
            .lifecycle_feature_tasks
            .iter()
            .map(|task| (
                &task.task_uid,
                &task.feature_uid,
                &task.planning_item_uid,
                task.phase,
                &task.pr_work_item_uid
            ))
            .collect::<Vec<_>>(),
        board.lifecycle_tasks,
        board.lifecycle_pr_statuses,
        board
            .lifecycle_pr_work
            .iter()
            .map(|work| (&work.parent_path, work.preparation.status))
            .collect::<Vec<_>>()
    );
    let done_children = board
        .planning_work
        .iter()
        .filter(|work| work.parent_uid.as_deref() == Some(root_uid.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(done_children.len(), 2);
    assert!(
        done_children
            .iter()
            .all(|work| { work.status == crate::core::planning_work::WorkStatus::Done })
    );
    assert!(board.lifecycle_feature_tasks.iter().any(|task| {
        task.task_uid == task_uid
            && task.phase == crate::core::lifecycle::TaskPhase::Done
            && task.pr_work_item_uid.as_deref() == Some(pr_uid.as_str())
    }));
    assert!(board.lifecycle_pr_work.iter().any(|work| {
        work.preparation.work_item_uid == pr_uid
            && work.preparation.status
                == crate::core::lifecycle::PullRequestPreparationStatus::AwaitingAuthorization
    }));

    let context = super::mockup_layout::styled_context();
    let mut output = None;
    for _ in 0..5 {
        output = Some(frame(&mut app, &context, vec![]));
    }
    let text = canvas_text(&output.unwrap());
    assert!(text.contains("TASK INTENT"), "rendered board: {text}");
    assert!(
        text.contains("Preparing pull request"),
        "rendered board: {text}"
    );
    assert!(
        text.contains("Awaiting your publication approval"),
        "publication gate remains visible: {text}"
    );
    assert!(
        text.contains("Review PR authorization"),
        "publication gate retains its actionable Board path: {text}"
    );
}
