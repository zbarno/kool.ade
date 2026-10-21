use crate::support::*;
use koolade::{
    artifacts::planning_store::PlanningStore,
    core::lifecycle::{GateDecision, LifecycleStore, TaskEvent, TaskPhase, TaskRecord},
    harness::prompt_catalog::PromptRole,
};

pub(super) fn reviewed_task_after_repair(
    store: &PlanningStore,
    feature_uid: &str,
    approved_hash: &str,
) -> (String, TaskRecord) {
    let reopened = LifecycleStore::new(store);
    let parent_uid = id();
    let child_uid = id();
    let unrelated_uid = id();
    reopened
        .create_task(&task(feature_uid, &parent_uid, approved_hash, None), &id())
        .unwrap();
    reopened
        .create_task(
            &task(
                feature_uid,
                &child_uid,
                approved_hash,
                Some(parent_uid.clone()),
            ),
            &id(),
        )
        .unwrap();
    reopened
        .create_task(
            &task(feature_uid, &unrelated_uid, approved_hash, None),
            &id(),
        )
        .unwrap();
    assert_eq!(
        reopened.load_task(&parent_uid).unwrap().subtask_uids,
        [child_uid]
    );

    // Independent task update proceeds while parent implementation is under
    // review; record-level revisions avoid a workspace-wide serialization.
    let mut parent = reopened.load_task(&parent_uid).unwrap();
    let unrelated = reopened.load_task(&unrelated_uid).unwrap();
    let other_store = PlanningStore::new(store.project_id, store.root.clone(), store.mode);
    let thread = std::thread::spawn(move || {
        let other = LifecycleStore::new(&other_store);
        other.apply_task_event(&unrelated_uid, unrelated.revision, &id(), TaskEvent::Start)
    });
    parent = reopened
        .apply_task_event(&parent_uid, parent.revision, &id(), TaskEvent::Start)
        .unwrap();
    thread.join().unwrap().unwrap();
    parent = reopened
        .apply_task_event(
            &parent_uid,
            parent.revision,
            &id(),
            TaskEvent::ImplementationFinished {
                code_sha256: "a".repeat(64),
            },
        )
        .unwrap();
    let first_review = role_receipt(
        PromptRole::CodeReviewer,
        GateDecision::ChangesRequested,
        &parent_uid,
        &"a".repeat(64),
    );
    parent = reopened
        .apply_task_event(
            &parent_uid,
            parent.revision,
            &id(),
            TaskEvent::CodeReview {
                receipt: first_review,
                reviewed_head_sha: "b".repeat(40),
                review_subject_sha256: "c".repeat(64),
            },
        )
        .unwrap();
    parent = reopened
        .apply_task_event(&parent_uid, parent.revision, &id(), TaskEvent::BeginRepair)
        .unwrap();
    parent = reopened
        .apply_task_event(
            &parent_uid,
            parent.revision,
            &id(),
            TaskEvent::ImplementationFinished {
                code_sha256: "d".repeat(64),
            },
        )
        .unwrap();
    let passed_review = role_receipt(
        PromptRole::CodeReviewer,
        GateDecision::Approved,
        &parent_uid,
        &"d".repeat(64),
    );
    parent = reopened
        .apply_task_event(
            &parent_uid,
            parent.revision,
            &id(),
            TaskEvent::CodeReview {
                receipt: passed_review,
                reviewed_head_sha: "e".repeat(40),
                review_subject_sha256: "f".repeat(64),
            },
        )
        .unwrap();
    assert_eq!(parent.phase, TaskPhase::AwaitingRuntimeQa);
    (parent_uid, parent)
}
