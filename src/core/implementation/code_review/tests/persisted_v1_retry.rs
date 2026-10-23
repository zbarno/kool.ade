use super::*;
use crate::harness::{AiHarness, ReviewExecutionIdentity};
use std::{
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

struct NoReviewCalls {
    calls: AtomicUsize,
    identity: ReviewExecutionIdentity,
}

impl AiHarness for NoReviewCalls {
    fn label(&self) -> String {
        "no-review-call fixture".into()
    }

    fn check_available(&self) -> Result<String, crate::error::AppError> {
        Ok("fixture".into())
    }

    fn execute(
        &self,
        _request: &crate::harness::PlanningRequest,
    ) -> Result<crate::harness::HarnessOutcome, crate::error::AppError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Err(crate::error::AppError::Other(
            "unexpected reviewer execution".into(),
        ))
    }

    fn current_execution_identity(&self) -> Option<ReviewExecutionIdentity> {
        Some(self.identity.clone())
    }
}

#[test]
fn failed_v1_receipt_for_unchanged_subject_is_not_overwritten_or_re_reviewed() {
    let root = tempfile::tempdir().unwrap();
    let task_repo = root.path().join("task-repo");
    std::fs::create_dir(&task_repo).unwrap();
    git(&task_repo, &["init", "--quiet", "--initial-branch=main"]);
    std::fs::write(task_repo.join("value.txt"), "base\n").unwrap();
    commit(&task_repo, "base");
    let base_sha = git(&task_repo, &["rev-parse", "HEAD"]);
    std::fs::write(task_repo.join("value.txt"), "updated\n").unwrap();
    commit(&task_repo, "task change");
    let head_sha = git(&task_repo, &["rev-parse", "HEAD"]);

    let task_uid = "00000000-0000-4000-8000-000000000002";
    let ticket_text =
        "# Task\n\nFeature: F1\n\n## Acceptance criteria\n\n- File contains updated.\n";
    let approved_specification = "- File contains updated.\n";
    let mut state: Implementation = serde_json::from_value(serde_json::json!({
        "ticket": "task-ticket",
        "task_uid": task_uid,
        "ticket_text": ticket_text,
        "approved_specification": approved_specification,
        "branch": "task-branch",
        "base": "main",
        "base_commit": base_sha,
        "task_repository": task_repo,
        "status": "verifying",
        "detail": "verification complete",
        "pr_url": null,
        "verified_head": head_sha
    }))
    .unwrap();
    state.approved_specification = Some(approved_specification.into());

    let dir = root.path().join("implementation-state");
    std::fs::create_dir(&dir).unwrap();
    let report = "validated test receipt\n";
    std::fs::write(dir.join("verified-report.json"), report).unwrap();
    let (author, prior_receipt) = failed_v1_receipt(&state, &dir, report);
    let receipt_path = dir.join(RECEIPT_FILE);
    let prior_bytes = serde_json::to_vec(&prior_receipt).unwrap();
    crate::artifacts::atomic_write_bytes(&receipt_path, &prior_bytes).unwrap();

    let (progress, _progress_rx) = mpsc::channel();
    let runner = Runner {
        gh: "git".into(),
        runtime_config_source: None,
        deadline: Instant::now() + Duration::from_secs(30),
        cancel: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        progress,
        authorization_verifier: None,
    };
    let harness = NoReviewCalls {
        calls: AtomicUsize::new(0),
        identity: author,
    };

    // Persisting the failed review adds a lifecycle history ID. That changes
    // the next subject hash without changing the code snapshot, so an ordinary
    // retry must still wait for a Manager disposition.
    let lifecycle_task: crate::core::lifecycle::TaskRecord =
        serde_json::from_value(serde_json::json!({
            "schema_version": 1,
            "task_uid": task_uid,
            "feature_uid": prior_receipt.feature_uid,
            "phase": "repairing",
            "revision": 2,
            "approved_plan_sha256": digest(approved_specification.as_bytes()),
            "implementation_sha256": prior_receipt.diff_hash,
            "reviewed_code_head_sha": prior_receipt.head_sha,
            "reviewed_code_subject_sha256": prior_receipt.subject_hash,
            "code_review": null,
            "review_history": [{
                "receipt_uid": "00000000-0000-4000-8000-000000000006",
                "subject_uid": task_uid,
                "subject_sha256": prior_receipt.subject_hash,
                "actor": "independent_reviewer",
                "decision": "changes_requested",
                "recorded_at_unix_ms": 1,
                "evidence_refs": ["code-review-v1.json"],
                "reviewer_run_id": "reviewer-run",
                "author_run_id": prior_receipt.author_run_id,
                "reviewer_session_id": "00000000-0000-4000-8000-000000000007",
                "author_session_id": prior_receipt.author_session_id
            }],
            "runtime_qa": null,
            "spec_reconciliation": null,
            "pr_work_item_uid": null
        }))
        .unwrap();
    let error =
        review_verified(&dir, &state, Some(&lifecycle_task), &harness, &runner).unwrap_err();
    assert!(error.to_string().contains("Manager resolution is required"));
    assert_eq!(harness.calls.load(Ordering::SeqCst), 0);
    assert_eq!(std::fs::read(&receipt_path).unwrap(), prior_bytes);
}

fn failed_v1_receipt(
    state: &Implementation,
    dir: &std::path::Path,
    report: &str,
) -> (ReviewExecutionIdentity, CodeReviewReceiptV1) {
    let author = ReviewExecutionIdentity {
        execution_id: "00000000-0000-4000-8000-000000000003".into(),
        session_id: "00000000-0000-4000-8000-000000000005".into(),
        runtime_qa_screenshot_review: None,
    };
    let feature_uid = crate::core::workflow::feature_ids_in("F1")
        .into_iter()
        .next()
        .unwrap();
    let spec = state.approved_specification.as_deref().unwrap();
    let approved_plan_hash = digest(spec.as_bytes());
    let diff_path = dir.join("expected.patch");
    let runner = Runner {
        gh: "git".into(),
        runtime_config_source: None,
        deadline: Instant::now() + Duration::from_secs(30),
        cancel: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        progress: mpsc::channel().0,
        authorization_verifier: None,
    };
    runner
        .git_to_file_bounded(
            &state.task_repository,
            &[
                "diff",
                "--binary",
                &format!(
                    "{}...{}",
                    state.base_commit,
                    state.verified_head.as_deref().unwrap()
                ),
            ],
            &diff_path,
            MAX_DIFF_BYTES,
        )
        .unwrap();
    let diff = std::fs::read(diff_path).unwrap();
    let diff_hash = digest(&diff);
    let paths = runner
        .git(
            &state.task_repository,
            &[
                "diff",
                "--name-only",
                &format!(
                    "{}...{}",
                    state.base_commit,
                    state.verified_head.as_deref().unwrap()
                ),
            ],
        )
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let requirements = requirement_matrix(&state.ticket_text, state.task_uid.as_deref().unwrap());
    let test_execution_receipts = vec![digest(report.as_bytes())];
    let subject = ReviewSubject {
        task_uid: state.task_uid.as_deref().unwrap(),
        feature_uid: &feature_uid,
        approved_plan_hash: &approved_plan_hash,
        task_spec_revision: &approved_plan_hash,
        task_clone_head_sha: state.verified_head.as_deref().unwrap(),
        base_sha: &state.base_commit,
        full_diff_hash: &diff_hash,
        linked_subtask_ids: &[],
        affected_paths: &paths,
        requirement_matrix: &requirements,
        test_execution_receipts: &test_execution_receipts,
        build_artifacts: &[],
        runtime_qa_receipt_ids: &[],
        existing_review_attempt_ids: &[],
    };
    let subject_hash = digest(&serde_json::to_vec(&subject).unwrap());
    let mut receipt = receipt_for_role(crate::harness::prompt_catalog::PromptRole::CodeReviewerV1);
    receipt.task_uid = state.task_uid.clone().unwrap();
    receipt.feature_uid = feature_uid;
    receipt.subject_hash = subject_hash.clone();
    receipt.verdict.subject_hash = subject_hash;
    receipt.diff_hash = diff_hash;
    receipt.base_sha = state.base_commit.clone();
    receipt.head_sha = state.verified_head.clone().unwrap();
    receipt.author_run_id = author.execution_id.clone();
    receipt.author_session_id = author.session_id.clone();
    receipt.verdict.pass = false;
    receipt
        .verdict
        .findings
        .push(crate::harness::prompt_catalog::ReviewFindingV1 {
            id: "legacy-finding".into(),
            requirement: "AC-1".into(),
            location_detail: None,
            rationale: String::new(),
            affected_requirement_ids: Vec::new(),
            suggested_action: None,
            severity: crate::harness::prompt_catalog::Severity::High,
            kind: crate::harness::prompt_catalog::FindingKind::Correctness,
            scope: None,
            location: "src/lib.rs:1".into(),
            evidence: vec!["verified-test-report-0".into()],
            repro: None,
            proposed_correction: "Fix the reported behavior".into(),
        });
    (author, receipt)
}

fn git(repo: &std::path::Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .unwrap();
    assert!(output.status.success(), "git {args:?} failed");
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn commit(repo: &std::path::Path, message: &str) {
    git(repo, &["add", "value.txt"]);
    let output = Command::new("git")
        .args([
            "-c",
            "user.name=Test User",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "--quiet",
            "-m",
            message,
        ])
        .current_dir(repo)
        .output()
        .unwrap();
    assert!(output.status.success(), "git commit failed");
}
