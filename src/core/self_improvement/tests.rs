use super::{
    SelfImprovementEventV1 as Event, SelfImprovementParentTaskProgressV1,
    SelfImprovementPhaseV1 as Phase, SelfImprovementRunV1,
};

fn uid(last: u8) -> String {
    format!("00000000-0000-4000-8000-{last:012x}")
}

fn sha(byte: char) -> String {
    std::iter::repeat_n(byte, 64).collect()
}

fn commit(byte: char) -> String {
    std::iter::repeat_n(byte, 40).collect()
}

fn run() -> SelfImprovementRunV1 {
    SelfImprovementRunV1 {
        schema_version: 1,
        run_uid: uid(1),
        opportunity_uid: uid(2),
        approved_decision_uid: uid(3),
        target_repository_uid: uid(4),
        canonical_remote: "https://github.com/zbarno/kool.ade".into(),
        source_ref: None,
        source_sha: commit('a'),
        destination_ref: None,
        feature_uid: uid(5),
        task_uids: vec![],
        owner_task_uid: None,
        parent_task_progress: vec![],
        clone_uid: None,
        plan_approval_hash: None,
        independent_plan_review_uid: None,
        implementation_head_sha: None,
        implementation_subject_sha256: None,
        code_review_uids: vec![],
        runtime_qa_uids: vec![],
        spec_receipt_uid: None,
        pr_work_item_uid: None,
        pr_url: None,
        merged_sha: None,
        activated_version: None,
        phase: Phase::ApprovedToPlan,
        revision: 1,
    }
}

#[test]
fn self_improvement_requires_an_explicit_reviewed_plan_before_clone_allocation() {
    let mut run = run();
    run.validate().unwrap();
    assert!(
        run.apply(1, Event::CloneAllocated { clone_uid: uid(7) })
            .is_err()
    );
    run.apply(1, Event::BeginPlanning).unwrap();
    assert!(
        run.apply(2, Event::CloneAllocated { clone_uid: uid(7) })
            .is_err()
    );
    run.apply(
        2,
        Event::PlanApproved {
            approval_sha256: sha('b'),
            review_uid: uid(8),
            source_ref: "feature/approved-source".into(),
            destination_ref: "main".into(),
            task_uids: vec![uid(6)],
            owner_task_uid: uid(6),
            parent_task_uids: vec![uid(6)],
        },
    )
    .unwrap();
    run.apply(3, Event::CloneAllocated { clone_uid: uid(7) })
        .unwrap();
    assert_eq!(run.phase, Phase::Implementing);
}

#[test]
fn self_improvement_tracks_progress_for_every_independent_parent_task() {
    let mut run = run();
    run.apply(1, Event::BeginPlanning).unwrap();
    run.apply(
        2,
        Event::PlanApproved {
            approval_sha256: sha('b'),
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
}

#[test]
fn self_improvement_requires_reviews_qa_specs_before_pr_and_activation() {
    let mut run = run();
    run.apply(1, Event::BeginPlanning).unwrap();
    run.apply(
        2,
        Event::PlanApproved {
            approval_sha256: sha('b'),
            review_uid: uid(8),
            source_ref: "feature/approved-source".into(),
            destination_ref: "main".into(),
            task_uids: vec![uid(6)],
            owner_task_uid: uid(6),
            parent_task_uids: vec![uid(6)],
        },
    )
    .unwrap();
    run.apply(3, Event::CloneAllocated { clone_uid: uid(7) })
        .unwrap();
    run.apply(
        4,
        Event::ImplementationFinished {
            head_sha: commit('c'),
            subject_sha256: sha('c'),
        },
    )
    .unwrap();
    assert!(
        run.apply(
            5,
            Event::QaPassed {
                receipt_uid: uid(9),
                subject_sha256: sha('c')
            }
        )
        .is_err()
    );
    run.apply(
        5,
        Event::CodeReviewPassed {
            receipt_uid: uid(9),
            subject_sha256: sha('c'),
        },
    )
    .unwrap();
    run.apply(
        6,
        Event::QaPassed {
            receipt_uid: uid(10),
            subject_sha256: sha('c'),
        },
    )
    .unwrap();
    run.apply(
        7,
        Event::SpecReconciled {
            receipt_uid: uid(11),
            subject_sha256: sha('c'),
        },
    )
    .unwrap();
    run.apply(
        8,
        Event::PrPrepared {
            work_item_uid: uid(12),
        },
    )
    .unwrap();
    run.apply(
        9,
        Event::PrOpened {
            url: "https://github.com/zbarno/kool.ade/pull/175".into(),
        },
    )
    .unwrap();
    assert!(run.clone().apply(10, Event::NeedsAttention).is_err());
    let mut closed_unmerged = run.clone();
    closed_unmerged
        .apply(
            10,
            Event::PrClosedUnmerged {
                url: "https://github.com/zbarno/kool.ade/pull/175".into(),
            },
        )
        .unwrap();
    assert_eq!(closed_unmerged.phase, Phase::PrClosedUnmerged);
    assert!(
        closed_unmerged
            .apply(11, Event::Merged { sha: commit('d') })
            .is_err()
    );
    let mut reopened = closed_unmerged.clone();
    reopened.apply(11, Event::PrReopened).unwrap();
    assert_eq!(reopened.phase, Phase::PrOpened);
    reopened
        .apply(
            12,
            Event::PrClosedUnmerged {
                url: "https://github.com/zbarno/kool.ade/pull/175".into(),
            },
        )
        .unwrap();
    assert_eq!(reopened.phase, Phase::PrClosedUnmerged);
    assert!(run.apply(10, Event::Cancel).is_err());
    run.apply(10, Event::Merged { sha: commit('d') }).unwrap();
    assert_eq!(run.phase, Phase::MergedNotActivated);
    assert!(run.apply(11, Event::Cancel).is_err());
    assert!(
        run.apply(
            11,
            Event::ImplementationFinished {
                head_sha: commit('e'),
                subject_sha256: sha('e'),
            }
        )
        .is_err()
    );
    let forged_activation = serde_json::from_value(serde_json::json!({
        "activated": {
            "evidence": {
                "version": "0.2.0",
                "mergedSha": commit('e'),
                "buildCommitSha": commit('e'),
                "readinessProbeUid": uid(14)
            }
        }
    }))
    .unwrap();
    assert!(run.apply(11, forged_activation).is_err());
    assert_eq!(run.phase, Phase::MergedNotActivated);
    assert!(
        run.apply(
            12,
            Event::ImplementationFinished {
                head_sha: commit('f'),
                subject_sha256: sha('f'),
            }
        )
        .is_err()
    );
}

#[path = "tests/persistence.rs"]
mod persistence;
