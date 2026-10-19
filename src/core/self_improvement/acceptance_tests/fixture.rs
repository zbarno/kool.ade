use super::{
    OpportunityScenario,
    support::{
        apply, assert_activation_requires_probe, git, implement_in_isolated_local_clone, proposal,
        pursue_decision, restart_store, self_improvement_run, uid,
    },
};
use std::process::Command;

use crate::{
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

pub(super) fn run_opportunity_acceptance(scenario: OpportunityScenario) {
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
    let (initial, changed, problem, value, outcome) = match scenario {
        OpportunityScenario::UnsupportedLanguage => (
            "runtime_language=odin\n",
            "runtime_language=odin\ncapability=odin_adapter\n",
            "Kool.ad/e cannot inspect an Odin project",
            "Plan and validate Odin projects through the ordinary workflow",
            "A reviewed Odin adapter is available",
        ),
        OpportunityScenario::UnrelatedUx => (
            "board_navigation=pointer_only\n",
            "board_navigation=pointer_and_keyboard\n",
            "Board navigation requires too many pointer actions",
            "Move between board columns with keyboard shortcuts",
            "A reviewed keyboard navigation improvement is available",
        ),
    };
    std::fs::write(source.join("product.txt"), initial).unwrap();
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
    let mut idea = proposal(&source_sha);
    idea.problem_or_opportunity = problem.into();
    idea.user_value = value.into();
    idea.expected_outcome = outcome.into();
    assert!(
        opportunities.save_advisory(idea.clone()).is_err(),
        "scheduled discovery is disabled by default"
    );
    let defaults = opportunities.discovery_settings().unwrap();
    opportunities
        .save_discovery_settings(
            &crate::core::opportunity::OpportunityDiscoverySettingsV1 {
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
            capability_id: Some("self-improvement-lifecycle".into()),
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
    let clone_event_uid = uid();
    let project_id = planning.project_id;
    let planning_root = planning.root.clone();
    let run_uid = run.run_uid.clone();
    let test_name = std::thread::current().name().unwrap().to_owned();
    drop(planning);
    // Production boundary: a fresh OS process reopens the managed planning
    // store after plan approval, loads the run, and records clone allocation.
    // The parent then reopens it and replays the same event to verify durable
    // recovery and idempotency beyond a same-process store reopen.
    let status = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", &test_name, "--nocapture"])
        .env(
            "KOOLADE_SELF_IMPROVEMENT_PROJECT_ID",
            project_id.to_string(),
        )
        .env("KOOLADE_SELF_IMPROVEMENT_RUN_UID", &run_uid)
        .env("KOOLADE_SELF_IMPROVEMENT_EVENT_UID", &clone_event_uid)
        .env("KOOLADE_SELF_IMPROVEMENT_CLONE_UID", &clone_uid)
        .env(super::RESTART_CHILD, &planning_root)
        .status()
        .unwrap();
    assert_eq!(
        status.code(),
        Some(86),
        "restart child must persist clone allocation and terminate immediately"
    );
    let planning = PlanningStore::new(project_id, planning_root, StoreMode::ManagedLocal);
    let runs = SelfImprovementStore::new(&planning);
    let mut run = runs.load(&run_uid).unwrap();
    assert_eq!(run.phase, Phase::Implementing);
    let resumed = runs
        .apply_event(
            &run_uid,
            run.revision,
            &clone_event_uid,
            Event::CloneAllocated {
                clone_uid: clone_uid.clone(),
            },
        )
        .unwrap();
    assert_eq!(
        resumed, run,
        "replaying the child process event must be idempotent"
    );
    run = resumed;
    assert_eq!(run.phase, Phase::Implementing);

    let (clone, implementation_head, subject_sha256) =
        implement_in_isolated_local_clone(temp.path(), &source, changed);
    assert_ne!(implementation_head, source_sha);
    let implementation_event = Event::ImplementationFinished {
        head_sha: implementation_head.clone(),
        subject_sha256: subject_sha256.clone(),
    };
    let implementation_event_uid = uid();
    run = runs
        .apply_event(
            &run.run_uid,
            run.revision,
            &implementation_event_uid,
            implementation_event.clone(),
        )
        .unwrap();
    let (planning, resumed) = restart_store(planning, &run.run_uid).unwrap();
    // Production boundary: close and reopen the managed planning store after
    // implementation, then replay the same receipt. This verifies durable
    // self-improvement recovery across a store reopen, beyond an
    // in-memory state-machine test. It uses a local temp store and Git clone;
    // no Docker or network service is required.
    let runs = SelfImprovementStore::new(&planning);
    assert_eq!(resumed, run);
    assert_eq!(
        runs.apply_event(
            &resumed.run_uid,
            resumed.revision,
            &implementation_event_uid,
            implementation_event,
        )
        .unwrap(),
        resumed,
        "replaying a committed implementation receipt after restart is idempotent"
    );
    run = resumed;
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
    // A prepared PR can survive a planning-store reopen while the controller
    // waits for an external merge. Reopen before applying merge evidence, and
    // assert the PR state and URL come from durable records.
    let (planning, mut run) = restart_store(planning, &run.run_uid).unwrap();
    let runs = SelfImprovementStore::new(&planning);
    assert_eq!(run.phase, Phase::PrOpened);
    assert_eq!(
        run.pr_url.as_deref(),
        Some("https://github.com/zbarno/kool.ade/pull/999999")
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
    assert_activation_requires_probe(&runs, &run);

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
