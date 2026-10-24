use super::*;

pub(super) struct DonePrWorkCase<'a, 'store> {
    pub(super) reopened: &'a LifecycleStore<'store>,
    pub(super) reopened_store: &'a PlanningStore,
    pub(super) done: &'a koolade::core::lifecycle::TaskRecord,
    pub(super) task_uid: &'a str,
    pub(super) child_uid: &'a str,
    pub(super) expected_revision: u64,
    pub(super) spec_receipt: GateReceipt,
    pub(super) work_uid: &'a str,
}

pub(super) fn assert_done_task_has_one_pr_work_item(case: DonePrWorkCase<'_, '_>) {
    let DonePrWorkCase {
        reopened,
        reopened_store,
        done,
        task_uid,
        child_uid,
        expected_revision,
        spec_receipt,
        work_uid,
    } = case;
    let work = reopened.load_pr_preparation(work_uid).unwrap();
    assert_eq!(work.task_uid, task_uid);
    assert_eq!(
        work.included_task_uids,
        [task_uid.to_owned(), child_uid.to_owned()]
    );
    assert_eq!(
        work.status,
        PullRequestPreparationStatus::AwaitingAuthorization
    );
    assert!(work.publication_authorization.is_none());
    assert_eq!(
        reopened.load_task(&done.task_uid).unwrap(),
        *done,
        "Done and its PR work item survive app restart"
    );
    assert_eq!(
        reopened_store
            .list_record_paths("state/lifecycle_pr_work_items")
            .unwrap()
            .len(),
        1
    );
    assert!(
        reopened
            .load_task(child_uid)
            .unwrap()
            .pr_work_item_uid
            .is_none()
    );
    assert!(
        reopened
            .reconcile_specification(task_uid, expected_revision, &id(), spec_receipt, work_uid)
            .is_err(),
        "stale replay must not create another PR work item"
    );
    assert_eq!(
        reopened_store
            .list_record_paths("state/lifecycle_pr_work_items")
            .unwrap()
            .len(),
        1
    );
}
