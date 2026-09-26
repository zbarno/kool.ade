//! Contract checks for generated explanations and their cache.
use super::*;

#[test]
fn source_choice_ids_must_match_generated_options() {
    let report = Report {
        status: ReportStatus::Blocked,
        blocker_disposition: BlockerDisposition::HumanAction,
        summary: "Mismatch".into(),
        acceptance_criteria: vec![],
        verification: vec![],
        remaining: vec!["Adjudicator: choose (a) accept or (b) correct".into()],
        human_choices: Vec::new(),
    };
    let brief = Brief {
        problem: "Published history conflicts with the promised file list.".into(),
        recommendation: None,
        options: vec![OptionBrief {
            id: "a".into(),
            label: "Accept".into(),
            meaning: "Accept the actual list".into(),
            consequence: "The baseline changes".into(),
            source_evidence: Some("accept".into()),
        }],
        steps: vec![],
        after: "Resume after the choice is recorded.".into(),
    };
    assert!(validate(&brief, &report).is_err());
    let mut both = brief;
    both.options.push(OptionBrief {
        id: "b".into(),
        label: "Correct".into(),
        meaning: "Correct the rule".into(),
        consequence: "The written promise changes".into(),
        source_evidence: Some("correct".into()),
    });
    both.recommendation = Some(Recommendation {
        option_id: "b".into(),
        rationale: "The recorded evidence favors correcting the file rule.".into(),
    });
    assert!(validate(&both, &report).is_ok());
    both.recommendation.as_mut().unwrap().option_id = "z".into();
    assert!(validate(&both, &report).is_err());
}

#[test]
fn legacy_choices_are_generated_from_quotes_in_the_same_saved_action_line() {
    let alternatives = [
        "wait for the monthly reset",
        "request a temporary quota increase",
        "reduce the batch size",
    ];
    let report = Report {
        status: ReportStatus::Blocked,
        blocker_disposition: BlockerDisposition::HumanAction,
        summary: "A decision is needed".into(),
        acceptance_criteria: vec![],
        verification: vec![],
        remaining: vec![format!(
            "Account owner: choose a recovery path: {}.",
            alternatives.join("; ")
        )],
        human_choices: Vec::new(),
    };
    let brief = Brief {
        problem: "The task needs one of the listed choices before it can continue.".into(),
        recommendation: None,
        options: alternatives
            .iter()
            .enumerate()
            .map(|(index, source)| OptionBrief {
                id: format!("quota-path-{}", index + 1),
                label: format!("Recovery path {}", index + 1),
                meaning: format!("Use {source}."),
                consequence: format!("The task follows the {source} recovery."),
                source_evidence: Some((*source).into()),
            })
            .collect(),
        steps: vec![],
        after: "Packet continues after the choice is recorded.".into(),
    };
    assert!(validate(&brief, &report).is_ok());
}

#[test]
fn generated_options_are_rejected_when_the_report_has_no_choices() {
    let report = Report {
        status: ReportStatus::Blocked,
        blocker_disposition: BlockerDisposition::HumanAction,
        summary: "A provider quota stopped this task until capacity returns".into(),
        acceptance_criteria: vec![],
        verification: vec![],
        remaining: vec!["Account owner: wait for the quota reset tomorrow".into()],
        human_choices: Vec::new(),
    };
    let brief = Brief {
        problem: "The provider has no requests left, so the task must wait for tomorrow's reset."
            .into(),
        recommendation: None,
        options: vec![OptionBrief {
            id: "a".into(),
            label: "Buy more capacity".into(),
            meaning: "Ask the provider to raise the request limit.".into(),
            consequence: "This may add cost to the account.".into(),
            source_evidence: None,
        }],
        steps: vec![],
        after: "Packet can retry when quota is available.".into(),
    };
    let error = validate(&brief, &report).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("must quote distinct alternatives")
    );
}

#[test]
fn structured_issue_choices_need_no_fixed_letter_or_sentence_pattern() {
    let report = Report {
        status: ReportStatus::Blocked,
        blocker_disposition: BlockerDisposition::HumanAction,
        summary: "The frozen footprint conflicts with published history.".into(),
        acceptance_criteria: vec![],
        verification: vec![],
        remaining: vec!["Adjudicator: select the policy for this mismatch.".into()],
        human_choices: vec![
            crate::core::implementation::report::HumanChoice {
                id: "ratify-base".into(),
                label: "Use the effective base".into(),
                meaning: "Treat the published base as the starting point.".into(),
                consequence: "The expected file table will include earlier changes.".into(),
            },
            crate::core::implementation::report::HumanChoice {
                id: "reissue-rule".into(),
                label: "Correct the footprint rule".into(),
                meaning: "Rewrite the check so it measures this ticket's own changes.".into(),
                consequence: "The acceptance rule changes while published history stays intact."
                    .into(),
            },
        ],
    };
    let brief = Brief {
        problem: "The check compares two different file lists, so the published history cannot pass it as written.".into(),
        recommendation: None,
        options: report
            .human_choices
            .iter()
            .map(|choice| OptionBrief {
                id: choice.id.clone(),
                label: choice.label.clone(),
                meaning: choice.meaning.clone(),
                consequence: choice.consequence.clone(),
                source_evidence: Some(choice.label.clone()),
            })
            .collect(),
        steps: vec![],
        after: "Packet can continue once the chosen policy is recorded.".into(),
    };
    assert!(validate(&brief, &report).is_ok());
}

#[test]
fn ordinary_failure_uses_its_own_detail_as_source() {
    use crate::{
        error::AppError,
        harness::{HarnessOutcome, PlanningRequest},
    };
    struct Fixture;
    impl AiHarness for Fixture {
        fn label(&self) -> String {
            "fixture".into()
        }
        fn check_available(&self) -> Result<String, AppError> {
            Ok("ready".into())
        }
        fn execute(&self, request: &PlanningRequest) -> Result<HarnessOutcome, AppError> {
            assert_eq!(
                request.mode,
                crate::harness::ExecutionMode::DecisionExplanation
            );
            assert_eq!(request.reasoning_level, "xhigh");
            assert!(request.prompt_body.contains("Disk is full"));
            assert!(!request.prompt_body.contains("realized footprint"));
            assert!(!request.prompt_body.contains("history repair"));
            assert!(!request.prompt_body.contains("quota"));
            assert!(
                request
                    .prompt_body
                    .contains("Free space in the task worktree")
            );
            Ok(HarnessOutcome { final_text: serde_json::json!({
                "problem": "The disk is full, so Packet cannot finish writing this task.",
                "recommendation": null,
                "options": [],
                "steps": [{"owner":"Project owner","action":"Free space in the task worktree."}],
                "after": "Resume the task after space is available."
            }).to_string(), envelope: None, stderr_tail: String::new() })
        }
    }
    let detail = "Disk is full\n\n### Next action(s)\n- Free space in the task worktree";
    let brief = run_detail(
        Path::new("/tmp"),
        ".kool-ade-packet/planning/tasks/demo/task.md",
        detail,
        &Fixture,
        Arc::new(AtomicBool::new(false)),
    )
    .unwrap();
    assert!(brief.problem.contains("disk is full"));
    assert!(brief.options.is_empty());
}

#[test]
fn report_gets_a_read_only_issue_specific_brief_and_reuses_the_cache() {
    use crate::{
        error::AppError,
        harness::{HarnessOutcome, PlanningRequest},
    };
    use std::sync::atomic::AtomicUsize;

    struct Fixture {
        calls: Arc<AtomicUsize>,
    }
    impl AiHarness for Fixture {
        fn label(&self) -> String {
            "brief fixture".into()
        }
        fn check_available(&self) -> Result<String, AppError> {
            Ok("ready".into())
        }
        fn execute(&self, request: &PlanningRequest) -> Result<HarnessOutcome, AppError> {
            assert_eq!(
                request.mode,
                crate::harness::ExecutionMode::DecisionExplanation
            );
            assert_eq!(request.reasoning_level, "xhigh");
            assert!(
                request
                    .prompt_body
                    .contains("Provider quota reset is tomorrow")
            );
            assert!(request.prompt_body.contains("docs/quota.md"));
            assert!(
                request
                    .prompt_body
                    .contains("Higher quota may increase cost")
            );
            assert!(
                request
                    .prompt_body
                    .contains("RECORDED VERIFICATION RESULTS:")
            );
            assert!(
                request
                    .prompt_body
                    .contains("The published-base footprint must match")
            );
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(HarnessOutcome { final_text: serde_json::json!({
                "problem": "The provider ran out of daily requests, so this task must pause.",
                "recommendation": {"option_id":"wait","rationale":"The report says capacity returns tomorrow, so waiting avoids changing account limits."},
                "options": [
                    {"id":"wait","label":"Wait for reset","meaning":"Use tomorrow's quota","consequence":"The task waits for the reset.","source_evidence":"Wait for reset"},
                    {"id":"raise-limit","label":"Request more","meaning":"Ask for more daily requests","consequence":"The account may cost more.","source_evidence":"Request more"}
                ],
                "steps": [],
                "after": "Packet retries after capacity is available."
            }).to_string(), envelope: None, stderr_tail: String::new() })
        }
    }

    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let repo =
        std::env::temp_dir().join(format!("packet-attention-{}-{stamp}", std::process::id()));
    fs::create_dir_all(repo.join("docs")).unwrap();
    fs::write(repo.join("docs/quota.md"), "Higher quota may increase cost").unwrap();
    let ticket = ".kool-ade-packet/planning/tasks/demo/001-task.md";
    fs::create_dir_all(repo.join(".kool-ade-packet/planning/tasks/demo")).unwrap();
    fs::write(
        repo.join(ticket),
        "# Recover the release task\n\nThe published-base footprint must match the exact set introduced by this ticket.\n",
    )
    .unwrap();
    let dir = implementation::state_dir(&repo, ticket).unwrap();
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("123-report.json");
    let report = Report {
        status: ReportStatus::Blocked,
        blocker_disposition: BlockerDisposition::HumanAction,
        summary: "Provider quota reset is tomorrow".into(),
        acceptance_criteria: vec![],
        verification: vec!["Verification result: 26 paths found; the promise allows eight.".into()],
        remaining: vec!["Account owner: choose how to restore provider capacity.".into()],
        human_choices: vec![
            crate::core::implementation::report::HumanChoice {
                id: "wait".into(),
                label: "Wait for reset".into(),
                meaning: "Use tomorrow's included quota.".into(),
                consequence: "The task remains paused until the reset.".into(),
            },
            crate::core::implementation::report::HumanChoice {
                id: "raise-limit".into(),
                label: "Request more".into(),
                meaning: "Ask the provider for a higher daily request limit; see docs/quota.md."
                    .into(),
                consequence: "The account may cost more.".into(),
            },
        ],
    };
    fs::write(&path, serde_json::to_vec(&report).unwrap()).unwrap();
    let detail = format!(
        "## Waiting for user action\n\nFull report: {}",
        path.display()
    );
    assert_eq!(source_path(&repo, ticket, &detail), Some(path.clone()));
    let calls = Arc::new(AtomicUsize::new(0));
    let fixture = Fixture {
        calls: calls.clone(),
    };
    let cancel = Arc::new(AtomicBool::new(false));
    let first = run(&repo, ticket, &path, &fixture, cancel.clone()).unwrap();
    assert_eq!(first.options.len(), 2);
    assert!(
        first
            .options
            .iter()
            .any(|option| option.id == "raise-limit")
    );
    let second = run(&repo, ticket, &path, &fixture, cancel.clone()).unwrap();
    assert_eq!(second, first);
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "unchanged report reuses generated brief"
    );
    fs::write(&path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    run(&repo, ticket, &path, &fixture, cancel).unwrap();
    assert_eq!(
        calls.load(Ordering::SeqCst),
        2,
        "changed report invalidates the brief"
    );
    let mut changed_verification = report;
    changed_verification.verification =
        vec!["A replay confirmed the mismatch is repeatable.".into()];
    fs::write(
        &path,
        serde_json::to_vec_pretty(&changed_verification).unwrap(),
    )
    .unwrap();
    run(
        &repo,
        ticket,
        &path,
        &fixture,
        Arc::new(AtomicBool::new(false)),
    )
    .unwrap();
    assert_eq!(
        calls.load(Ordering::SeqCst),
        3,
        "changed verification evidence invalidates the brief"
    );
    fs::write(
        repo.join("docs/quota.md"),
        "Higher quota may increase cost today",
    )
    .unwrap();
    run(
        &repo,
        ticket,
        &path,
        &fixture,
        Arc::new(AtomicBool::new(false)),
    )
    .unwrap();
    assert_eq!(
        calls.load(Ordering::SeqCst),
        4,
        "changed referenced document invalidates the brief"
    );
    fs::remove_dir_all(repo).unwrap();
}
