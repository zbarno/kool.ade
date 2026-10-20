use serde::Serialize;

#[derive(Serialize)]
pub(super) struct ReviewSubject<'a> {
    pub(super) task_uid: &'a str,
    pub(super) feature_uid: &'a str,
    pub(super) approved_plan_hash: &'a str,
    pub(super) task_spec_revision: &'a str,
    pub(super) task_clone_head_sha: &'a str,
    pub(super) base_sha: &'a str,
    pub(super) full_diff_hash: &'a str,
    pub(super) linked_subtask_ids: &'a [String],
    pub(super) affected_paths: &'a [String],
    pub(super) requirement_matrix: &'a [String],
    pub(super) test_execution_receipts: &'a [String],
    pub(super) build_artifacts: &'a [String],
    pub(super) runtime_qa_receipt_ids: &'a [String],
    pub(super) existing_review_attempt_ids: &'a [String],
}
