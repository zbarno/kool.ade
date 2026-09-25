//! Contract checks for generated explanations and their cache.
use super::*;

#[test]
fn source_choice_ids_must_match_generated_options() {
    let report = Report {
        status: "blocked".into(),
        summary: "Mismatch".into(),
        acceptance_criteria: vec![],
        verification: vec![],
        remaining: vec!["Adjudicator: choose (a) accept or (b) correct".into()],
    };
    let brief = Brief {
        problem: "Published history conflicts with the promised file list.".into(),
        options: vec![OptionBrief {
            id: "a".into(),
            label: "Accept".into(),
            meaning: "Accept the actual list".into(),
            consequence: "The baseline changes".into(),
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
    });
    assert!(validate(&both, &report).is_ok());
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
            assert!(request.read_only);
            assert!(request.prompt_body.contains("Disk is full"));
            assert!(
                request
                    .prompt_body
                    .contains("Free space in the task worktree")
            );
            Ok(HarnessOutcome { final_text: serde_json::json!({
                "problem": "The disk is full, so Packet cannot finish writing this task.",
                "options": [],
                "steps": [{"owner":"Project owner","action":"Free space in the task worktree."}],
                "after": "Resume the task after space is available."
            }).to_string(), envelope: None, stderr_tail: String::new() })
        }
    }
    let detail = "Disk is full\n\n### Next action(s)\n- Free space in the task worktree";
    let brief = run_detail(
        Path::new("/tmp"),
        "planning/tasks/demo/task.md",
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
            assert!(request.read_only);
            assert!(!request.implementation);
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
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(HarnessOutcome { final_text: serde_json::json!({
                "problem": "The provider ran out of daily requests, so this task must pause.",
                "options": [
                    {"id":"a","label":"Wait for reset","meaning":"Use tomorrow's quota","consequence":"The task waits one day."},
                    {"id":"b","label":"Request more","meaning":"Ask for more daily requests","consequence":"The account may cost more."}
                ],
                "steps": [{"owner":"Account owner","action":"Choose when to obtain capacity."}],
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
    let ticket = "planning/tasks/demo/001-task.md";
    let dir = implementation::state_dir(&repo, ticket).unwrap();
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("123-report.json");
    let report = Report {
        status: "blocked".into(),
        summary: "Provider quota reset is tomorrow".into(),
        acceptance_criteria: vec![],
        verification: vec![],
        remaining: vec![
            "Account owner: choose (a) wait or (b) request more; see docs/quota.md".into(),
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
        3,
        "changed referenced document invalidates the brief"
    );
    fs::remove_dir_all(repo).unwrap();
}
