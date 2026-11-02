pub(super) fn spec_receipt_uid(
    task_uid: &str,
    code_sha256: &str,
    reviewed_head_sha: &str,
    spec_sha256: &str,
    proposal_sha256: &str,
) -> String {
    super::super::stable_uid(
        task_uid,
        &format!(
            "spec-reconcile-receipt:v3:{code_sha256}:{reviewed_head_sha}:{spec_sha256}:{proposal_sha256}"
        ),
    )
}

pub(super) fn pr_work_item_uid(
    task_uid: &str,
    existing_uid: Option<&str>,
    approved_plan_sha256: &str,
    code_sha256: &str,
) -> String {
    existing_uid.map(str::to_owned).unwrap_or_else(|| {
        super::super::stable_uid(
            task_uid,
            &format!("lifecycle-pr-work:v1:{approved_plan_sha256}:{code_sha256}"),
        )
    })
}
