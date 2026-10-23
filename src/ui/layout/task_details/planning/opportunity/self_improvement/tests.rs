use super::*;

fn parent(task_uid: &str, pr_url: Option<&str>) -> SelfImprovementParentTaskProgressV1 {
    SelfImprovementParentTaskProgressV1 {
        task_uid: task_uid.into(),
        phase: crate::core::lifecycle::TaskPhase::Ready,
        implementation_sha256: None,
        code_review_uid: None,
        runtime_qa_uid: None,
        spec_receipt_uid: None,
        pr_work_item_uid: None,
        pr_status: None,
        pr_url: pr_url.map(str::to_owned),
        pr_head_sha: None,
        pr_base_branch: None,
        pr_merged_commit_sha: None,
        pr_merged_at: None,
    }
}

#[test]
fn progress_exposes_each_parent_task_pull_request() {
    let links = pull_request_links(
        &[
            parent(
                "12345678-0000-4000-8000-000000000001",
                Some("https://github.com/zbarno/kool.ade/pull/101"),
            ),
            parent(
                "87654321-0000-4000-8000-000000000002",
                Some("https://github.com/zbarno/kool.ade/pull/102"),
            ),
        ],
        Some("https://github.com/zbarno/kool.ade/pull/101"),
    );
    assert_eq!(
        links,
        vec![
            (
                "Open PR for task 12345678".into(),
                "https://github.com/zbarno/kool.ade/pull/101".into(),
            ),
            (
                "Open PR for task 87654321".into(),
                "https://github.com/zbarno/kool.ade/pull/102".into(),
            ),
        ]
    );
}

#[test]
fn progress_uses_the_legacy_primary_pr_when_parent_links_are_absent() {
    assert_eq!(
        pull_request_links(&[], Some("https://github.com/zbarno/kool.ade/pull/101")),
        vec![(
            "Open pull request".into(),
            "https://github.com/zbarno/kool.ade/pull/101".into(),
        )]
    );
}

#[test]
fn progress_keeps_the_primary_pr_when_parent_records_are_partial() {
    let links = pull_request_links(
        &[parent(
            "12345678-0000-4000-8000-000000000001",
            Some("https://github.com/zbarno/kool.ade/pull/101"),
        )],
        Some("https://github.com/zbarno/kool.ade/pull/102"),
    );
    assert_eq!(links.len(), 2);
    assert_eq!(links[1].0, "Open pull request");
    assert_eq!(links[1].1, "https://github.com/zbarno/kool.ade/pull/102");
}
