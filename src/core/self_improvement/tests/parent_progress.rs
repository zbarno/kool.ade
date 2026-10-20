use super::*;

#[test]
fn self_improvement_tracks_progress_for_every_independent_parent_task() {
    let mut run = run();
    run.apply(1, Event::BeginPlanning).unwrap();
    run.apply(
        2,
        Event::PlanApproved {
            approval_sha256: sha('b'),
            capability_id: Some("self-improvement-lifecycle".into()),
            review_uid: uid(8),
            source_ref: "feature/approved-source".into(),
            destination_ref: "main".into(),
            task_uids: vec![uid(6), uid(7)],
            owner_task_uid: uid(6),
            parent_task_uids: vec![uid(6), uid(7)],
        },
    )
    .unwrap();
    assert_eq!(
        run.parent_task_progress
            .iter()
            .map(|progress| progress.task_uid.clone())
            .collect::<Vec<_>>(),
        vec![uid(6), uid(7)]
    );

    let first = SelfImprovementParentTaskProgressV1 {
        task_uid: uid(6),
        phase: crate::core::lifecycle::TaskPhase::PrOpened,
        implementation_sha256: Some(sha('c')),
        code_review_uid: Some(uid(9)),
        runtime_qa_uid: Some(uid(10)),
        spec_receipt_uid: Some(uid(11)),
        pr_work_item_uid: Some(uid(12)),
        pr_status: Some(crate::core::lifecycle::PullRequestPreparationStatus::Opened),
        pr_url: Some("https://github.com/zbarno/kool.ade/pull/1".into()),
        pr_head_sha: Some(commit('d')),
        pr_base_branch: Some("main".into()),
        pr_merged_commit_sha: None,
        pr_merged_at: None,
    };
    run.apply(3, Event::ParentTaskProgress(first.clone()))
        .unwrap();
    let second = SelfImprovementParentTaskProgressV1 {
        task_uid: uid(7),
        phase: crate::core::lifecycle::TaskPhase::Ready,
        implementation_sha256: None,
        code_review_uid: None,
        runtime_qa_uid: None,
        spec_receipt_uid: None,
        pr_work_item_uid: None,
        pr_status: None,
        pr_url: None,
        pr_head_sha: None,
        pr_base_branch: None,
        pr_merged_commit_sha: None,
        pr_merged_at: None,
    };
    run.apply(4, Event::ParentTaskProgress(second.clone()))
        .unwrap();

    assert_eq!(run.parent_task_progress, vec![first, second]);
    assert_eq!(
        run.parent_task_progress[0].pr_url.as_deref(),
        Some("https://github.com/zbarno/kool.ade/pull/1")
    );
    assert_eq!(
        run.parent_task_progress[1].phase,
        crate::core::lifecycle::TaskPhase::Ready
    );

    let mut distinct_prs = run.clone();
    let mut second_pr = distinct_prs.parent_task_progress[1].clone();
    second_pr.pr_work_item_uid = Some(uid(13));
    second_pr.pr_url = Some("https://github.com/zbarno/kool.ade/pull/2".into());
    distinct_prs
        .apply(5, Event::ParentTaskProgress(second_pr))
        .expect("independent parents may publish distinct pull requests");

    let mut duplicate_pr = run.parent_task_progress[1].clone();
    duplicate_pr.pr_work_item_uid = Some(uid(14));
    duplicate_pr.pr_url = Some("https://GITHUB.com:443/zbarno/kool.ade/pull/1".into());
    let mut invalid = run.clone();
    assert!(
        invalid
            .apply(5, Event::ParentTaskProgress(duplicate_pr))
            .is_err(),
        "two parent tasks cannot share one publication work item or pull request, even when URL spelling differs"
    );
}
