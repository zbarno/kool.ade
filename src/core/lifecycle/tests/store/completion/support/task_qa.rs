use super::super::super::Fixture;
use crate::core::lifecycle::{GateActor, GateDecision, LifecycleStore, TaskEvent, TaskRecord};
use crate::runtime_qa::QaRunRequestV1;

pub(in crate::core::lifecycle::tests::store::completion) fn finish_task_through_qa(
    fixture: &Fixture,
    lifecycle: &LifecycleStore<'_>,
    task_uid: &str,
) -> (TaskRecord, QaRunRequestV1) {
    let mut task = lifecycle
        .apply_task_event(
            task_uid,
            lifecycle.load_task(task_uid).unwrap().revision,
            &uuid::Uuid::new_v4().to_string(),
            TaskEvent::Start,
        )
        .unwrap();
    let code_sha = "c".repeat(64);
    task = lifecycle
        .apply_task_event(
            task_uid,
            task.revision,
            &uuid::Uuid::new_v4().to_string(),
            TaskEvent::ImplementationFinished {
                code_sha256: code_sha.clone(),
            },
        )
        .unwrap();
    let mut review = super::super::super::super::receipt(
        GateActor::IndependentReviewer,
        GateDecision::Approved,
        task_uid,
    );
    review.subject_sha256 = code_sha.clone();
    task = lifecycle
        .apply_task_event(
            task_uid,
            task.revision,
            &uuid::Uuid::new_v4().to_string(),
            super::super::super::super::code_review_event(review),
        )
        .unwrap();
    task = lifecycle
        .block_runtime_qa_unavailable(
            task_uid,
            task.revision,
            &uuid::Uuid::new_v4().to_string(),
            "No approved startup recipe is available".into(),
        )
        .unwrap();
    assert!(lifecycle.pending_runtime_qa().unwrap().is_empty());
    assert!(
        lifecycle
            .runtime_qa_candidates()
            .unwrap()
            .iter()
            .any(|item| item.task_uid == task_uid)
    );
    let request = super::super::super::super::qa_request(&task);
    task = lifecycle
        .queue_runtime_qa(
            task_uid,
            task.revision,
            &uuid::Uuid::new_v4().to_string(),
            request.clone(),
        )
        .unwrap();
    let queued = lifecycle.pending_runtime_qa().unwrap();
    assert!(queued.iter().any(|item| {
        item.task_uid == task_uid
            && item
                .qa_execution
                .as_ref()
                .and_then(|state| state.request.as_ref())
                .is_some_and(|queued_request| queued_request.qa_run_id == request.qa_run_id)
    }));
    let reopened_store = fixture.store();
    let reopened = LifecycleStore::new(&reopened_store);
    assert!(reopened.pending_runtime_qa().unwrap().iter().any(|item| {
        item.qa_execution
            .as_ref()
            .and_then(|state| state.request.as_ref())
            .is_some_and(|queued_request| queued_request.qa_run_id == request.qa_run_id)
    }));
    let qa_event = super::super::super::super::completed_qa_event(&task, &request);
    task = lifecycle
        .apply_task_event(
            task_uid,
            task.revision,
            &uuid::Uuid::new_v4().to_string(),
            qa_event,
        )
        .unwrap();
    (task, request)
}
