//! User-facing opportunity and self-improvement path for #174/#175.
//! The proposal is inert until a persisted Pursue decision; a published or
//! merged PR remains distinct from activating a newly built application.

use std::process::Command;

use koolade::{
    artifacts::planning_store::{PlanningStore, StoreMode},
    core::{
        opportunity::{
            OpportunityDecisionKindV1, OpportunityExploreModeV1, OpportunitySaveResult,
            OpportunityStatusV1, OpportunityStore,
        },
        self_improvement::{
            SelfImprovementEventV1 as Event, SelfImprovementPhaseV1 as Phase, SelfImprovementStore,
        },
    },
};
use sha2::{Digest, Sha256};

#[path = "opportunity_self_improvement_acceptance/support.rs"]
mod support;
use support::{apply, git, proposal, pursue_decision, self_improvement_run, uid};

#[test]
fn opportunity_requires_opt_in_and_user_pursuit_then_clone_review_qa_pr_and_activation_gates() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    std::fs::create_dir(&source).unwrap();
    assert!(
        Command::new("git")
            .args(["init", "--quiet", "--initial-branch=main"])
            .arg(&source)
            .status()
            .unwrap()
            .success()
    );
    std::fs::write(source.join("product.txt"), "approved source\n").unwrap();
    git(&source, &["add", "product.txt"]);
    git(
        &source,
        &["commit", "--quiet", "-m", "synthetic product baseline"],
    );
    let source_sha = git(&source, &["rev-parse", "HEAD"]);
    let source_bytes = std::fs::read(source.join("product.txt")).unwrap();
    let source_status = git(&source, &["status", "--porcelain"]);

    let planning = PlanningStore::new(
        uuid::Uuid::new_v4(),
        temp.path().join("planning"),
        StoreMode::ManagedLocal,
    );
    let opportunities = OpportunityStore::new(&planning);
    assert_eq!(
        opportunities.discovery_settings().unwrap().exploration,
        OpportunityExploreModeV1::Disabled
    );
    let idea = proposal(&source_sha);
    assert!(
        opportunities.save_advisory(idea.clone()).is_err(),
        "scheduled discovery is disabled by default"
    );
    let defaults = opportunities.discovery_settings().unwrap();
    opportunities
        .save_discovery_settings(
            &koolade::core::opportunity::OpportunityDiscoverySettingsV1 {
                exploration: OpportunityExploreModeV1::OptedIn,
                ..defaults.clone()
            },
            defaults.revision,
        )
        .unwrap();
    let saved = opportunities.save_advisory(idea).unwrap();
    let OpportunitySaveResult::Created {
        proposal,
        record_revision,
    } = saved
    else {
        panic!("first scheduled opportunity should be created")
    };
    assert_eq!(proposal.status, OpportunityStatusV1::AwaitingDecision);

    let decision = pursue_decision(&proposal);
    let candidate_run = self_improvement_run(&decision.uid, &proposal, &source_sha);
    let runs = SelfImprovementStore::new(&planning);
    assert!(
        runs.create_from_pursued(&opportunities, &decision.uid, candidate_run.clone())
            .is_err()
    );
    assert!(!temp.path().join("self-improvement-clone").exists());
    assert_eq!(
        std::fs::read(source.join("product.txt")).unwrap(),
        source_bytes
    );
    assert_eq!(git(&source, &["status", "--porcelain"]), source_status);

    opportunities
        .record_user_decision(record_revision, &decision, "local-controller")
        .unwrap();
    assert_eq!(
        opportunities.decision(&decision.uid).unwrap().decision,
        OpportunityDecisionKindV1::Pursue
    );
    let mut run = runs
        .create_from_pursued(&opportunities, &decision.uid, candidate_run)
        .unwrap();
    assert_eq!(run.phase, Phase::Planning);
    assert!(
        runs.apply_event(
            &run.run_uid,
            run.revision,
            &uid(),
            Event::CloneAllocated { clone_uid: uid() }
        )
        .is_err(),
        "clone allocation is blocked before reviewed plan approval"
    );
    let task_uid = uid();
    run = apply(
        &runs,
        run,
        Event::PlanApproved {
            approval_sha256: "d".repeat(64),
            review_uid: uid(),
            source_ref: "koolade/self-improvement-test".into(),
            destination_ref: "main".into(),
            task_uids: vec![task_uid.clone()],
            owner_task_uid: task_uid,
            parent_task_uids: vec![],
        },
    );
    assert_eq!(run.phase, Phase::Ready);
    let clone_uid = uid();
    run = apply(&runs, run, Event::CloneAllocated { clone_uid });
    assert_eq!(run.phase, Phase::Implementing);

    let clone = temp.path().join("self-improvement-clone");
    let source_argument = source.display().to_string();
    let clone_argument = clone.display().to_string();
    let output = Command::new("git")
        .args([
            "clone",
            "--quiet",
            "--no-hardlinks",
            "--",
            &source_argument,
            &clone_argument,
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "isolated task clone failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    std::fs::write(
        clone.join("product.txt"),
        "approved source\nplanned product change\n",
    )
    .unwrap();
    git(&clone, &["add", "product.txt"]);
    git(
        &clone,
        &[
            "commit",
            "--quiet",
            "-m",
            "implement approved product change",
        ],
    );
    let implementation_head = git(&clone, &["rev-parse", "HEAD"]);
    assert_ne!(implementation_head, source_sha);
    let changed_bytes = std::fs::read(clone.join("product.txt")).unwrap();
    let subject_sha256 = format!("{:x}", Sha256::digest(&changed_bytes));
    run = apply(
        &runs,
        run,
        Event::ImplementationFinished {
            head_sha: implementation_head.clone(),
            subject_sha256: subject_sha256.clone(),
        },
    );
    assert!(
        runs.apply_event(
            &run.run_uid,
            run.revision,
            &uid(),
            Event::QaPassed {
                receipt_uid: uid(),
                subject_sha256: subject_sha256.clone(),
            },
        )
        .is_err(),
        "runtime QA cannot skip independent code review"
    );
    run = apply(
        &runs,
        run,
        Event::CodeReviewPassed {
            receipt_uid: uid(),
            subject_sha256: subject_sha256.clone(),
        },
    );
    assert_eq!(run.phase, Phase::Qa);
    assert!(
        runs.apply_event(
            &run.run_uid,
            run.revision,
            &uid(),
            Event::PrPrepared {
                work_item_uid: uid(),
            },
        )
        .is_err(),
        "PR preparation cannot skip runtime QA and spec reconciliation"
    );
    run = apply(
        &runs,
        run,
        Event::QaPassed {
            receipt_uid: uid(),
            subject_sha256: subject_sha256.clone(),
        },
    );
    run = apply(
        &runs,
        run,
        Event::SpecReconciled {
            receipt_uid: uid(),
            subject_sha256,
        },
    );
    run = apply(
        &runs,
        run,
        Event::PrPrepared {
            work_item_uid: uid(),
        },
    );
    run = apply(
        &runs,
        run,
        Event::PrOpened {
            url: "https://github.com/zbarno/kool.ade/pull/999999".into(),
        },
    );
    run = apply(
        &runs,
        run,
        Event::Merged {
            sha: implementation_head,
        },
    );
    assert_eq!(run.phase, Phase::MergedNotActivated);
    assert!(run.activated_version.is_none());
    assert!(
        runs.apply_event(
            &run.run_uid,
            run.revision,
            &uid(),
            Event::Activated {
                version: "unprobed build".into(),
                readiness_probe_passed: false,
            }
        )
        .is_err()
    );
    let after_merge = runs.load(&run.run_uid).unwrap();
    assert_eq!(after_merge.phase, Phase::MergedNotActivated);
    assert!(after_merge.activated_version.is_none());

    assert_eq!(
        std::fs::read(source.join("product.txt")).unwrap(),
        source_bytes
    );
    assert_eq!(git(&source, &["status", "--porcelain"]), source_status);
    assert_ne!(
        std::fs::read(clone.join("product.txt")).unwrap(),
        source_bytes
    );
}
