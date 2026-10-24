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

struct UnavailableReviewer {
    calls: AtomicUsize,
    identity: ReviewExecutionIdentity,
}

impl AiHarness for UnavailableReviewer {
    fn label(&self) -> String {
        "unavailable reviewer fixture".into()
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
            "synthetic reviewer process crash".into(),
        ))
    }

    fn current_execution_identity(&self) -> Option<ReviewExecutionIdentity> {
        Some(self.identity.clone())
    }
}

#[test]
fn reviewer_crash_cannot_create_a_passing_receipt() {
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

    let state: Implementation = serde_json::from_value(serde_json::json!({
        "ticket": "task-ticket",
        "task_uid": "00000000-0000-4000-8000-000000000002",
        "ticket_text": "# Task\n\nFeature: F1\n\n## Acceptance criteria\n\n- File contains updated.\n",
        "approved_specification": "- File contains updated.\n",
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

    let dir = root.path().join("implementation-state");
    std::fs::create_dir(&dir).unwrap();
    std::fs::write(dir.join("verified-report.json"), "validated test receipt\n").unwrap();
    let (progress, _progress_rx) = mpsc::channel();
    let runner = Runner {
        gh: "git".into(),
        runtime_config_source: None,
        deadline: Instant::now() + Duration::from_secs(30),
        cancel: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        progress,
        authorization_verifier: None,
    };
    let harness = UnavailableReviewer {
        calls: AtomicUsize::new(0),
        identity: ReviewExecutionIdentity {
            execution_id: "00000000-0000-4000-8000-000000000010".into(),
            session_id: "00000000-0000-4000-8000-000000000011".into(),
            runtime_qa_screenshot_review: None,
        },
    };

    let error = review_verified(&dir, &state, None, &harness, &runner).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("synthetic reviewer process crash")
    );
    assert_eq!(harness.calls.load(Ordering::SeqCst), 1);
    assert!(
        load_receipt(&dir, state.task_uid.as_deref().unwrap())
            .unwrap()
            .is_none()
    );
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
