use super::*;

struct GeneratedOutputAgent {
    calls: Arc<AtomicUsize>,
}

impl AiHarness for GeneratedOutputAgent {
    fn label(&self) -> String {
        "generated integration output fixture".into()
    }

    fn check_available(&self) -> Result<String, AppError> {
        Ok("fixture".into())
    }

    fn execute(
        &self,
        request: &PlanningRequest,
    ) -> Result<crate::harness::HarnessOutcome, AppError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if request.prompt_body.contains("INITIAL BASE RECONCILIATION") {
            return Ok(outcome(complete_report(
                "test -f local.txt && test -f upstream.txt",
            )));
        }
        fs::write(request.repo_root.join("implemented.txt"), "implemented\n").unwrap();
        fs::create_dir_all(request.repo_root.join("Source")).unwrap();
        fs::write(request.repo_root.join("Source/changed.txt"), "changed\n").unwrap();
        fs::create_dir_all(request.repo_root.join(".cache")).unwrap();
        fs::write(
            request.repo_root.join(".cache/injected.rs"),
            "untrusted ignored source\n",
        )
        .unwrap();
        Ok(outcome(serde_json::json!({
            "schemaVersion":2,
            "status":"complete",
            "blocker_disposition":"none",
            "summary":"Implemented the requested task on the reconciled base.",
            "acceptance_criteria":[{"criterion":"File contains implemented.","evidence":"implemented.txt contains the expected content."}],
            "verification":["mkdir -p .cache && printf source > .cache/report-source.rs && test \"$(cat implemented.txt)\" = implemented"],
            "remaining":[],
            "human_choices":[]
        })))
    }
}

#[test]
fn verified_reconciliation_and_generated_output_survive_publication_retry() {
    let s = resilience::interrupted_with_build_ignore();
    let dir = state_dir(&s.repo, &s.ticket).unwrap();
    assert!(dir.join("base-reconciliation.json").exists());
    assert!(dir.join("base-reconciliation-generated.json").exists());
    let calls = Arc::new(AtomicUsize::new(0));
    let agent = GeneratedOutputAgent {
        calls: calls.clone(),
    };
    fs::write(s.root.join("offline"), "").unwrap();

    let error = run_with_agent(&s, &agent, None).unwrap_err().to_string();

    assert!(error.contains("simulated GitHub unavailable"), "{error}");
    let integrated = crate::core::implementation::load(&s.repo, &s.ticket).unwrap();
    assert!(integrated.branch.starts_with("koolade/integration/"));
    assert!(
        crate::core::implementation::initial_reconciliation::integrated_candidate_matches(
            &dir,
            &integrated
        )
        .unwrap()
    );
    assert!(
        integrated.task_repository.join(".cache/ready").exists(),
        "integration verification output should remain in the integrated clone"
    );
    assert!(quarantine_contains(&dir, ".cache/injected.rs"));
    assert!(
        integrated
            .task_repositories
            .iter()
            .all(|repository| !repository.join(".cache/injected.rs").exists())
    );
    let calls_after_failure = calls.load(Ordering::SeqCst);
    assert!(calls_after_failure > 0);
    assert!(!s.root.join("pr-created").exists());

    fs::remove_file(s.root.join("offline")).unwrap();
    let result = run_with_agent(&s, &agent, None).unwrap();

    assert_eq!(result.status, ImplementationStatus::AwaitingReview);
    assert_eq!(
        calls.load(Ordering::SeqCst),
        calls_after_failure,
        "verified work is reused"
    );
    assert_eq!(
        fs::read_to_string(s.root.join("pr-created"))
            .unwrap()
            .lines()
            .count(),
        1
    );
    let generated_repository = result
        .task_repositories
        .iter()
        .find(|repository| repository.join("build/cache.txt").exists())
        .expect("verified generated output remains in its preserved task clone");
    assert_eq!(
        fs::read_to_string(generated_repository.join("build/cache.txt")).unwrap(),
        "cache"
    );
    assert!(
        result
            .task_repositories
            .iter()
            .all(|repository| !repository.join(".cache/report-source.rs").exists())
    );
}

#[test]
fn generated_output_survives_publication_retry_without_reconciliation_plan() {
    let s = Sandbox::new();
    fs::write(s.repo.join(".gitignore"), ".cache/\n").unwrap();
    fs::write(
        s.repo.join("AGENTS.md"),
        "# Required quality checks\n\n```sh\ntest ! -e .cache/injected.rs && case \"$(git branch --show-current)\" in koolade/integration/*) mkdir -p .cache && printf ready > .cache/ready;; esac && test \"$(cat implemented.txt)\" = implemented\n```\n",
    )
    .unwrap();
    fs::create_dir_all(s.repo.join("Source")).unwrap();
    fs::write(
        s.repo.join("Source/AGENTS.md"),
        "# Required quality checks\n\n```sh\ntest -f changed.txt\n```\n",
    )
    .unwrap();
    s.git(
        &s.repo,
        &["add", ".gitignore", "AGENTS.md", "Source/AGENTS.md"],
    );
    s.git(&s.repo, &["commit", "-qm", "ignore generated cache"]);
    s.git(&s.repo, &["push", "-q", "origin", "main"]);
    let dir = state_dir(&s.repo, &s.ticket).unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let agent = GeneratedOutputAgent {
        calls: calls.clone(),
    };
    fs::write(s.root.join("offline"), "").unwrap();

    let error = run_with_agent(&s, &agent, None).unwrap_err().to_string();

    assert!(error.contains("simulated GitHub unavailable"), "{error}");
    assert!(!dir.join("base-reconciliation.json").exists());
    let generated: serde_json::Value =
        serde_json::from_slice(&fs::read(dir.join("base-reconciliation-generated.json")).unwrap())
            .unwrap();
    assert!(
        generated["files"][".cache/report-source.rs"].is_null(),
        "report-only output must not receive generated-output provenance"
    );
    assert!(quarantine_contains(&dir, ".cache/injected.rs"));
    let verified: serde_json::Value =
        serde_json::from_slice(&fs::read(dir.join("verified-report.json")).unwrap()).unwrap();
    assert!(
        verified["verification"]
            .as_array()
            .unwrap()
            .iter()
            .any(|command| { command == "cd -- 'Source' && test -f changed.txt" })
    );
    let integrated = crate::core::implementation::load(&s.repo, &s.ticket).unwrap();
    assert!(integrated.branch.starts_with("koolade/integration/"));
    assert!(
        crate::core::implementation::initial_reconciliation::integrated_candidate_matches(
            &dir,
            &integrated
        )
        .unwrap()
    );
    assert!(integrated.task_repository.join(".cache/ready").exists());
    assert!(
        !integrated
            .task_repository
            .join(".cache/report-source.rs")
            .exists()
    );
    assert!(
        integrated
            .task_repositories
            .iter()
            .all(|repository| !repository.join(".cache/injected.rs").exists())
    );
    let calls_after_failure = calls.load(Ordering::SeqCst);
    assert!(calls_after_failure > 0);

    fs::remove_file(s.root.join("offline")).unwrap();
    let result = run_with_agent(&s, &agent, None).unwrap();

    assert_eq!(result.status, ImplementationStatus::AwaitingReview);
    assert_eq!(calls.load(Ordering::SeqCst), calls_after_failure);
    assert!(
        !result
            .task_repository
            .join(".cache/report-source.rs")
            .exists()
    );
    assert_eq!(
        fs::read_to_string(s.root.join("pr-created"))
            .unwrap()
            .lines()
            .count(),
        1
    );
}

fn quarantine_contains(task_dir: &Path, relative: &str) -> bool {
    task_dir
        .join("generated-output-quarantine")
        .read_dir()
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .any(|batch| batch.path().join(relative).is_file())
}
