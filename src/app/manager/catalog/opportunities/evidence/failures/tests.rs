use super::task_failure_evidence_digest;

#[test]
fn unlinked_task_failures_get_path_scoped_evidence_ids() {
    let first = task_failure_evidence_digest("workspace", "unlinked-task", "a.md");
    let second = task_failure_evidence_digest("workspace", "unlinked-task", "b.md");

    assert_ne!(first, second);
    assert_eq!(
        first,
        task_failure_evidence_digest("workspace", "unlinked-task", "a.md")
    );
}
