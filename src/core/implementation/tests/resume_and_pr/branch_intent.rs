use super::*;

struct AdvanceDestination<'a> {
    sandbox: &'a Sandbox,
    calls: Arc<AtomicUsize>,
}

impl AiHarness for AdvanceDestination<'_> {
    fn label(&self) -> String {
        "destination advance fixture".into()
    }

    fn check_available(&self) -> Result<String, AppError> {
        Ok("fixture".into())
    }

    fn execute(
        &self,
        request: &PlanningRequest,
    ) -> Result<crate::harness::HarnessOutcome, AppError> {
        let outcome = Fixture {
            mode: "complete",
            calls: self.calls.clone(),
        }
        .execute(request)?;
        self.sandbox.advance_remote();
        Ok(outcome)
    }
}

fn save_branch_intent(sandbox: &Sandbox, source: &str, destination: &str) {
    let batch = crate::domain::ArtifactIdentity::new("BATCH-1", "Branch task batch");
    let mut identity = crate::domain::ArtifactIdentity::new("F38-TASK-1", "Branch task");
    identity.parent_uid = Some(batch.uid);
    let targets = crate::core::workflow::BranchTargets {
        source: source.into(),
        destination: destination.into(),
    };
    let metadata = crate::artifacts::task_docs::TaskMetadata::new(&identity, "root", vec![])
        .unwrap()
        .with_branch_targets(Some(&targets))
        .unwrap();
    let identity_marker = serde_json::to_string(&identity).unwrap();
    let metadata_json = serde_json::to_string(&metadata).unwrap();
    let document = format!(
        "---\nkoolade-task: {metadata_json}\n---\n\n<!-- koolade-artifact-id:v1 {identity_marker} -->\n\n# Branch task\n\n## Acceptance criteria\n\n- File contains implemented.\n"
    );
    fs::write(sandbox.repo.join(&sandbox.ticket), document).unwrap();
    sandbox.git(&sandbox.repo, &["add", &sandbox.ticket]);
    sandbox.git(
        &sandbox.repo,
        &["commit", "-qm", "save selected branch intent"],
    );
    sandbox.git(&sandbox.repo, &["push", "-q", "origin", "main"]);
}

#[test]
fn selected_source_starts_the_task_and_distinct_destination_is_reconciled_and_used_for_pr() {
    let sandbox = Sandbox::new();
    sandbox.git(&sandbox.repo, &["branch", "release/2.1"]);
    sandbox.git(&sandbox.repo, &["checkout", "release/2.1"]);
    fs::write(sandbox.repo.join("source.txt"), "source only\n").unwrap();
    sandbox.git(&sandbox.repo, &["add", "."]);
    sandbox.git(&sandbox.repo, &["commit", "-qm", "source update"]);
    sandbox.git(&sandbox.repo, &["push", "-q", "origin", "release/2.1"]);
    sandbox.git(&sandbox.repo, &["checkout", "main"]);
    sandbox.git(&sandbox.repo, &["branch", "integration"]);
    sandbox.git(&sandbox.repo, &["checkout", "integration"]);
    fs::write(sandbox.repo.join("destination.txt"), "destination only\n").unwrap();
    sandbox.git(&sandbox.repo, &["add", "."]);
    sandbox.git(&sandbox.repo, &["commit", "-qm", "destination update"]);
    sandbox.git(&sandbox.repo, &["push", "-q", "origin", "integration"]);
    let destination_head = sandbox.git(&sandbox.repo, &["rev-parse", "HEAD"]);
    sandbox.git(&sandbox.repo, &["checkout", "main"]);

    save_branch_intent(&sandbox, "release/2.1", "integration");

    let state = sandbox
        .run_with_publication_policy(
            "complete",
            Arc::new(AtomicUsize::new(0)),
            PublicationMode::CreatePullRequest,
            None,
            false,
        )
        .unwrap();
    assert_eq!(state.source_branch.as_deref(), Some("release/2.1"));
    assert_eq!(state.destination_branch.as_deref(), Some("integration"));
    assert_eq!(state.base, "integration");
    assert_eq!(state.base_commit, destination_head);
    assert!(state.task_repository.join("destination.txt").exists());
    assert!(state.task_repository.join("source.txt").exists());
    assert!(state.task_repository.join("implemented.txt").exists());
    let cache = state.repository_cache.as_deref().unwrap();
    let base_ref = format!("refs/koolade-auto-bases/{}", key(&state.ticket));
    assert_eq!(
        sandbox.git(cache, &["rev-parse", &base_ref]),
        destination_head
    );
    assert!(
        sandbox
            .git(&sandbox.repo, &["for-each-ref", "refs/koolade-auto-bases"])
            .is_empty()
    );
    assert!(
        !common(&sandbox.repo)
            .unwrap()
            .join("koolade-auto-publish.lock")
            .exists()
    );
    assert!(cache.join("koolade-auto-publish.lock").exists());
    let args = fs::read_to_string(sandbox.root.join("pr-args")).unwrap();
    assert!(
        args.lines()
            .collect::<Vec<_>>()
            .windows(2)
            .any(|pair| pair == ["--base", "integration"])
    );
}

#[test]
fn local_only_source_branch_is_supported_when_destination_is_origin_backed() {
    let sandbox = Sandbox::new();
    sandbox.git(&sandbox.repo, &["branch", "local/source"]);
    sandbox.git(&sandbox.repo, &["checkout", "local/source"]);
    fs::write(sandbox.repo.join("local-source.txt"), "local source\n").unwrap();
    sandbox.git(&sandbox.repo, &["add", "."]);
    sandbox.git(
        &sandbox.repo,
        &["commit", "-qm", "local-only source update"],
    );
    let source_commit = sandbox.git(&sandbox.repo, &["rev-parse", "HEAD"]);
    sandbox.git(&sandbox.repo, &["checkout", "main"]);
    save_branch_intent(&sandbox, "local/source", "main");
    let state = sandbox
        .run_with_publication_policy(
            "complete",
            Arc::new(AtomicUsize::new(0)),
            PublicationMode::CreatePullRequest,
            None,
            false,
        )
        .unwrap();
    assert_eq!(state.source_ref.as_deref(), Some("local/source"));
    assert_eq!(state.source_commit.as_deref(), Some(source_commit.as_str()));
    assert!(state.task_repository.join("local-source.txt").exists());
    assert!(state.task_repository.join("implemented.txt").exists());
    assert!(sandbox.root.join("pr-created").exists());
}

#[test]
fn same_source_and_destination_reconciles_destination_updates_before_opening_pr() {
    let sandbox = Sandbox::new();
    save_branch_intent(&sandbox, "main", "main");
    let calls = Arc::new(AtomicUsize::new(0));
    let harness = AdvanceDestination {
        sandbox: &sandbox,
        calls,
    };
    let (tx, _rx) = mpsc::channel();
    let state = run_with_project_options(
        &sandbox.repo,
        &sandbox.repo,
        &sandbox.ticket,
        RunOptions {
            harness: &harness,
            cancel: Arc::new(AtomicBool::new(false)),
            progress: tx,
            gh: sandbox.gh.to_str().unwrap(),
            publication_mode: PublicationMode::CreatePullRequest,
            require_independent_checks: false,
            user_context: None,
            auto_publish_gate: None,
        },
    )
    .unwrap();
    assert_eq!(state.source_branch.as_deref(), Some("main"));
    assert_eq!(state.destination_branch.as_deref(), Some("main"));
    assert!(state.task_repository.join("upstream.txt").exists());
    assert!(state.task_repository.join("implemented.txt").exists());
    assert!(sandbox.root.join("pr-created").exists());
    let args = fs::read_to_string(sandbox.root.join("pr-args")).unwrap();
    assert!(
        args.lines()
            .collect::<Vec<_>>()
            .windows(2)
            .any(|pair| pair == ["--base", "main"])
    );
}

#[test]
fn missing_selected_source_needs_attention_without_falling_back_to_checkout_head() {
    let sandbox = Sandbox::new();
    save_branch_intent(&sandbox, "release/deleted", "main");
    let calls = Arc::new(AtomicUsize::new(0));
    let error = sandbox
        .run_with_publication_policy(
            "complete",
            calls.clone(),
            PublicationMode::CreatePullRequest,
            None,
            false,
        )
        .unwrap_err()
        .to_string();
    assert!(error.contains("Selected source branch 'release/deleted'"));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(load(&sandbox.repo, &sandbox.ticket).is_none());
}

#[test]
fn missing_selected_destination_needs_attention_during_final_reconciliation() {
    let sandbox = Sandbox::new();
    save_branch_intent(&sandbox, "main", "release/deleted");
    let error = sandbox
        .run_with_publication_policy(
            "complete",
            Arc::new(AtomicUsize::new(0)),
            PublicationMode::CreatePullRequest,
            None,
            false,
        )
        .unwrap_err()
        .to_string();
    assert!(error.contains("Selected destination branch 'release/deleted'"));
    assert!(!sandbox.root.join("pr-created").exists());
}

#[test]
fn pinned_source_commit_allows_resume_after_source_branch_is_deleted() {
    let sandbox = Sandbox::new();
    sandbox.git(&sandbox.repo, &["branch", "release/2.1"]);
    sandbox.git(&sandbox.repo, &["push", "-q", "origin", "release/2.1"]);
    save_branch_intent(&sandbox, "release/2.1", "main");
    let calls = Arc::new(AtomicUsize::new(0));
    assert!(sandbox.run("cancel", calls.clone()).is_err());
    let saved = load(&sandbox.repo, &sandbox.ticket).unwrap();
    let pinned_source = saved.source_commit.clone().unwrap();
    sandbox.git(&sandbox.repo, &["push", "origin", ":release/2.1"]);
    sandbox.git(&sandbox.repo, &["branch", "-D", "release/2.1"]);
    let resumed = sandbox.run("complete", calls.clone()).unwrap();
    assert_eq!(resumed.source_ref.as_deref(), Some("release/2.1"));
    assert_eq!(
        resumed.source_commit.as_deref(),
        Some(pinned_source.as_str())
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}
