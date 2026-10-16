use super::*;
#[path = "resilience/fixtures.rs"]
mod fixtures;
use fixtures::interrupted;
pub(super) use fixtures::{interrupted_with_build_ignore, interrupted_with_configuration};

struct StopAfterVerification;

impl AiHarness for StopAfterVerification {
    fn label(&self) -> String {
        "verification interruption fixture".into()
    }

    fn check_available(&self) -> Result<String, AppError> {
        Ok("fixture".into())
    }

    fn execute(&self, _: &PlanningRequest) -> Result<crate::harness::HarnessOutcome, AppError> {
        Ok(outcome(serde_json::json!({
            "schemaVersion":2,"status":"blocked","blocker_disposition":"environment_prerequisite",
            "summary":"Verification is interrupted for this fixture.","acceptance_criteria":[],
            "verification":[],"remaining":["Resume verification."],"human_choices":[]
        })))
    }
}

#[test]
fn failed_verification_resumes_with_unchanged_generated_files_and_old_snapshot() {
    let s = interrupted();
    let (path, snapshot) = super::merge_recovery::recovery_snapshot(&s);
    let old = fs::read(&path).unwrap();
    let (calls, agent) = super::merge_recovery::agent();
    let result = run_with_agent(&s, &agent, Some("Resume verification")).unwrap();
    assert_eq!(result.status, ImplementationStatus::AwaitingReview);
    assert_eq!(
        calls.load(Ordering::SeqCst),
        2,
        "integration needs one correction after its first gate run creates generated outputs"
    );
    assert_eq!(fs::read(path).unwrap(), old);
    let generated_repository = result
        .task_repositories
        .iter()
        .find(|repository| repository.join("build/cache.txt").exists())
        .expect("verified generated output should remain in its preserved task clone");
    assert_eq!(
        fs::read_to_string(generated_repository.join("build/cache.txt")).unwrap(),
        "cache"
    );
    assert!(
        s.git(generated_repository, &["ls-files", "build/cache.txt"])
            .is_empty()
    );
    assert!(
        s.git(
            super::merge_recovery::repository_cache(&s).as_path(),
            &[
                "show",
                &format!(
                    "{}^3:.cache/preexisting.txt",
                    snapshot["stash_commit"].as_str().unwrap()
                )
            ]
        )
        .contains("preserve preexisting")
    );
}

#[test]
fn modified_output_is_preserved_and_prior_snapshot_is_archived_on_resume() {
    let s = interrupted();
    let (path, mut snapshot) = super::merge_recovery::recovery_snapshot(&s);
    let worktree = super::merge_recovery::task_worktree(&s);
    let repository_cache = super::merge_recovery::repository_cache(&s);
    let private_ref = snapshot["private_ref"].as_str().unwrap();
    let refspec = format!("{private_ref}:{private_ref}");
    s.git(
        &worktree,
        &[
            "fetch",
            "--no-tags",
            repository_cache.to_str().unwrap(),
            &refspec,
        ],
    );
    snapshot["schema_version"] = serde_json::json!(1);
    snapshot.as_object_mut().unwrap().remove("task_repository");
    snapshot["worktree"] = serde_json::json!(worktree.to_string_lossy());
    let old = serde_json::to_vec_pretty(&snapshot).unwrap();
    fs::write(&path, &old).unwrap();
    let worktree = super::merge_recovery::migrated_task_repository(&s);
    fs::write(worktree.join("build/cache.txt"), "operator edit\n").unwrap();
    crate::core::implementation::mark_resume_started(&s.repo, &s.ticket).unwrap();
    let error = run_with_agent(&s, &StopAfterVerification, Some("Resume verification"))
        .unwrap_err()
        .to_string();
    assert!(error.contains("interrupted"), "{error}");
    let history = path
        .parent()
        .unwrap()
        .join("reconciliation-recovery-history");
    let archived = fs::read_dir(history)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert_eq!(fs::read(archived).unwrap(), old);
    let (_, new) = super::merge_recovery::recovery_snapshot(&s);
    let stash = new["stash_commit"].as_str().unwrap();
    assert_eq!(
        s.git(
            super::merge_recovery::repository_cache(&s).as_path(),
            &["show", &format!("{stash}^3:build/cache.txt")]
        ),
        "operator edit"
    );
    assert_eq!(
        s.git(
            super::merge_recovery::repository_cache(&s).as_path(),
            &["rev-parse", snapshot["private_ref"].as_str().unwrap()]
        ),
        snapshot["stash_commit"].as_str().unwrap()
    );
    let (_, agent) = super::merge_recovery::agent();
    assert_eq!(
        run_with_agent(&s, &agent, Some("Resume verification"))
            .unwrap()
            .status,
        ImplementationStatus::AwaitingReview
    );
}

#[test]
fn incomplete_recovery_phase_cannot_grant_another_attempt() {
    let s = interrupted();
    let (path, mut snapshot) = super::merge_recovery::recovery_snapshot(&s);
    snapshot["phase"] = serde_json::json!("snapshot_saved");
    fs::write(&path, serde_json::to_vec(&snapshot).unwrap()).unwrap();
    let worktree = super::merge_recovery::migrated_task_repository(&s);
    fs::write(worktree.join("build/cache.txt"), "operator edit\n").unwrap();
    let (_, agent) = super::merge_recovery::agent();
    crate::core::implementation::mark_resume_started(&s.repo, &s.ticket).unwrap();
    assert!(run_with_agent(&s, &agent, Some("Resume")).is_err());
    assert_eq!(
        fs::read_to_string(worktree.join("build/cache.txt")).unwrap(),
        "operator edit\n"
    );
    assert!(
        !path
            .parent()
            .unwrap()
            .join("reconciliation-recovery-history")
            .exists()
    );
}

#[test]
fn reconciliation_ready_report_defers_checks_without_weakening_task_validation() {
    let mut value = complete_report("test -f local.txt");
    value["verification"] = serde_json::json!([]);
    let report = crate::core::implementation::report::parse_report(&value.to_string()).unwrap();
    let contract = crate::core::implementation::initial_reconciliation::CONTRACT;
    assert!(
        crate::core::implementation::report::validate_reconciliation_report(&report, contract)
            .is_ok()
    );
    assert!(crate::core::implementation::report::validate_report(&report, contract).is_err());
}

#[test]
fn inherited_eof_whitespace_from_local_history_is_preserved() {
    assert_inherited_eof_whitespace_preserved(true);
}

#[test]
fn inherited_eof_whitespace_from_remote_history_is_preserved() {
    assert_inherited_eof_whitespace_preserved(false);
}

fn assert_inherited_eof_whitespace_preserved(local_side: bool) {
    let s = Sandbox::new();
    let remote = s.advance_remote();
    if !local_side {
        let peer = s.latest_peer();
        fs::write(peer.join("upstream.txt"), "latest upstream\n\n").unwrap();
        s.git(&peer, &["add", "upstream.txt"]);
        s.git(&peer, &["commit", "-qm", "preserve inherited whitespace"]);
        s.git(&peer, &["push", "-q", "origin", "main"]);
    }
    fs::write(
        s.repo.join("local.txt"),
        if local_side {
            "local change\n\n"
        } else {
            "local change\n"
        },
    )
    .unwrap();
    s.git(&s.repo, &["add", "local.txt"]);
    s.git(&s.repo, &["commit", "-qm", "local change"]);
    let (_, agent) = super::merge_recovery::agent();
    let result = run_with_agent(&s, &agent, None).unwrap();
    let path = if local_side {
        "local.txt"
    } else {
        "upstream.txt"
    };
    assert!(
        fs::read_to_string(result.task_repository.join(path))
            .unwrap()
            .ends_with("\n\n")
    );
    assert!(
        s.git(
            &result.task_repository,
            &["merge-base", "--is-ancestor", &remote, &result.base_commit]
        )
        .is_empty()
    );
}

#[test]
fn ordinary_user_context_does_not_authorize_snapshot_rotation() {
    let s = interrupted();
    let (path, _) = super::merge_recovery::recovery_snapshot(&s);
    let old = fs::read(&path).unwrap();
    let worktree = super::merge_recovery::migrated_task_repository(&s);
    fs::write(worktree.join("build/cache.txt"), "operator edit\n").unwrap();
    let (_, agent) = super::merge_recovery::agent();
    let error = run_with_agent(&s, &agent, Some("Use the existing API conventions"))
        .unwrap_err()
        .to_string();
    assert!(error.contains("Explicitly resume"), "{error}");
    assert_eq!(fs::read(path).unwrap(), old);
    assert_eq!(
        fs::read_to_string(worktree.join("build/cache.txt")).unwrap(),
        "operator edit\n"
    );
}
