use super::*;

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

fn interrupted() -> Sandbox {
    let s = Sandbox::new();
    fs::write(s.repo.join("AGENTS.md"), "# Instructions\n\n- Required quality gates: `mkdir -p .cache; if test -f .cache/ready; then test -f build/cache.txt; else mkdir -p build; printf cache > build/cache.txt; printf ready > .cache/ready; exit 1; fi`.\n").unwrap();
    s.git(&s.repo, &["add", "AGENTS.md"]);
    s.git(
        &s.repo,
        &["commit", "-qm", "require generated artifact fixture"],
    );
    s.git(&s.repo, &["push", "-q", "origin", "main"]);
    let (remote, local, common) = super::merge_recovery::make_divergent(&s);
    let dir = state_dir(&s.repo, &s.ticket).unwrap();
    fs::create_dir_all(&dir).unwrap();
    crate::core::implementation::initial_reconciliation::save_plan(
        &dir,
        "main",
        &local,
        &remote,
        &common,
        &[],
    )
    .unwrap();
    let worktree = super::merge_recovery::task_worktree(&s);
    fs::create_dir_all(worktree.parent().unwrap()).unwrap();
    let branch = format!(
        "koolade/{}",
        crate::core::implementation::key_for_ticket(&s.ticket)
    );
    s.git(
        &s.repo,
        &[
            "worktree",
            "add",
            "-b",
            &branch,
            worktree.to_str().unwrap(),
            &remote,
        ],
    );
    s.git(
        &worktree,
        &["merge", "--no-ff", "--no-commit", "--no-edit", &local],
    );
    fs::create_dir_all(worktree.join(".cache")).unwrap();
    fs::write(
        worktree.join(".cache/preexisting.txt"),
        "preserve preexisting cache\n",
    )
    .unwrap();
    let error = run_with_agent(&s, &StopAfterVerification, None)
        .unwrap_err()
        .to_string();
    assert!(error.contains("interrupted"), "{error}");
    assert!(dir.join("base-reconciliation-generated.json").exists());
    s
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
        1,
        "only implementation needs an agent"
    );
    assert_eq!(fs::read(path).unwrap(), old);
    assert_eq!(
        fs::read_to_string(result.worktree.join("build/cache.txt")).unwrap(),
        "cache"
    );
    assert!(
        s.git(&result.worktree, &["ls-files", "build/cache.txt"])
            .is_empty()
    );
    assert!(
        s.git(
            &s.repo,
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
    let (path, snapshot) = super::merge_recovery::recovery_snapshot(&s);
    let old = fs::read(&path).unwrap();
    let worktree = super::merge_recovery::task_worktree(&s);
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
        s.git(&s.repo, &["show", &format!("{stash}^3:build/cache.txt")]),
        "operator edit"
    );
    assert_eq!(
        s.git(
            &s.repo,
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
fn incomplete_or_mismatched_recovery_history_cannot_grant_another_attempt() {
    for field in ["phase", "target", "private_ref"] {
        let s = interrupted();
        let (path, mut snapshot) = super::merge_recovery::recovery_snapshot(&s);
        snapshot[field] = serde_json::json!(if field == "phase" {
            "snapshot_saved"
        } else {
            "invalid"
        });
        fs::write(&path, serde_json::to_vec(&snapshot).unwrap()).unwrap();
        let worktree = super::merge_recovery::task_worktree(&s);
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
fn inherited_eof_whitespace_in_either_history_is_preserved() {
    for local_side in [true, false] {
        let s = Sandbox::new();
        let remote = s.advance_remote();
        if !local_side {
            let peer = s.root.join("peer");
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
            fs::read_to_string(result.worktree.join(path))
                .unwrap()
                .ends_with("\n\n")
        );
        assert!(
            s.git(
                &result.worktree,
                &["merge-base", "--is-ancestor", &remote, &result.base_commit]
            )
            .is_empty()
        );
    }
}

#[test]
fn ordinary_user_context_does_not_authorize_snapshot_rotation() {
    let s = interrupted();
    let (path, _) = super::merge_recovery::recovery_snapshot(&s);
    let old = fs::read(&path).unwrap();
    let worktree = super::merge_recovery::task_worktree(&s);
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
