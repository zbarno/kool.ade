use super::*;

#[test]
fn newest_saved_report_controls_interrupted_blocker_recovery() {
    let s = Sandbox::new();
    let dir = state_dir(&s.repo, &s.ticket).unwrap();
    fs::create_dir_all(&dir).unwrap();
    let blocked = serde_json::json!({
        "status":"blocked", "summary":"The history needs a decision.",
        "blocker_disposition":"human_action",
        "acceptance_criteria":[], "verification":[],
        "remaining":["Adjudicator: approve the realized footprint."]
    });
    fs::write(dir.join("001-report.json"), blocked.to_string()).unwrap();
    assert!(
        latest_external_blocker(&s.repo, &s.ticket)
            .unwrap()
            .contains("approve the realized footprint")
    );
    let complete = serde_json::json!({
        "status":"complete", "blocker_disposition":"none", "summary":"Done", "acceptance_criteria":[],
        "verification":[], "remaining":[]
    });
    fs::write(dir.join("002-report.json"), complete.to_string()).unwrap();
    assert!(latest_external_blocker(&s.repo, &s.ticket).is_none());
}
#[test]
fn automatic_corrections_preserve_work_and_publish_only_after_verification() {
    for mode in [
        "repair_markdown",
        "repair_schema",
        "repair_criterion",
        "repair_verification",
        "repair_blocked",
    ] {
        let s = Sandbox::new();
        let calls = Arc::new(AtomicUsize::new(0));
        let result = s.run(mode, calls.clone()).unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 2, "{mode}");
        assert_eq!(result.status, ImplementationStatus::AwaitingReview);
        let dir = state_dir(&s.repo, &s.ticket).unwrap();
        let files = fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        let incomplete_response = matches!(mode, "repair_markdown" | "repair_schema");
        assert_eq!(
            files
                .iter()
                .filter(|name| name.ends_with("-report.json") && *name != "verified-report.json")
                .count(),
            if incomplete_response { 1 } else { 2 }
        );
        assert_eq!(
            files
                .iter()
                .filter(|name| name.ends_with("-response.txt"))
                .count(),
            usize::from(incomplete_response),
            "only incomplete final responses need a separate copy"
        );
        assert_eq!(
            files
                .iter()
                .filter(|name| name.ends_with("-correction.txt"))
                .count(),
            1
        );
        if mode == "repair_verification" {
            assert_eq!(
                files
                    .iter()
                    .filter(|name| name.ends_with("-verification.json"))
                    .count(),
                2
            );
            assert!(result.task_repository.join("missing-file").exists());
        }
        assert_eq!(
            fs::read_to_string(s.root.join("pr-created"))
                .unwrap()
                .lines()
                .count(),
            1
        );
    }
}

#[test]
fn correction_limit_blocker_and_cancellation_never_publish() {
    for (mode, expected_calls, error_text) in [
        ("fail", 6, "Automatic correction limit"),
        ("blocked", 6, "Automatic correction limit"),
        ("repair_cancel", 2, "Implementation cancelled"),
    ] {
        let s = Sandbox::new();
        let calls = Arc::new(AtomicUsize::new(0));
        let error = s.run(mode, calls.clone()).unwrap_err().to_string();
        assert!(error.contains(error_text), "{error}");
        assert_eq!(calls.load(Ordering::SeqCst), expected_calls);
        assert!(!s.root.join("pr-created").exists());
        let state = load(&s.repo, &s.ticket).unwrap();
        assert!(state.task_repository.join("implemented.txt").exists());
        assert!(state.detail.contains(error_text));
    }
}

#[test]
fn resume_after_exhaustion_gets_full_budget_without_nested_history() {
    let s = Sandbox::new();
    let calls = Arc::new(AtomicUsize::new(0));
    let first = s.run("fail", calls.clone()).unwrap_err().to_string();
    let original = load(&s.repo, &s.ticket).unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 6);
    let second = s.run("fail", calls.clone()).unwrap_err().to_string();
    assert_eq!(
        calls.load(Ordering::SeqCst),
        12,
        "resume gets all six attempts again"
    );
    assert_eq!(second.matches("Automatic correction limit").count(), 1);
    assert!(!second.contains("Correction history:"));
    assert!(second.contains("Latest failure:"));
    assert!(!s.root.join("pr-created").exists());
    let dir = state_dir(&s.repo, &s.ticket).unwrap();
    assert!(fs::read_dir(&dir).unwrap().flatten().any(|entry| {
        entry
            .file_name()
            .to_string_lossy()
            .ends_with("-resume-context.txt")
            && fs::read_to_string(entry.path()).unwrap() == first
    }));
    let resumed = s
        .run("fresh_budget", Arc::new(AtomicUsize::new(0)))
        .unwrap();
    assert_ne!(resumed.task_repository, original.task_repository);
    assert!(
        resumed
            .task_repositories
            .contains(&original.task_repository)
    );
    assert!(resumed.branch.starts_with("koolade/integration/"));
    assert_eq!(resumed.base_commit, original.base_commit);
    assert_eq!(resumed.status, ImplementationStatus::AwaitingReview);
}

#[test]
fn legacy_nested_exhaustion_keeps_only_latest_actionable_context() {
    let legacy = "Automatic correction limit. Correction history: Automatic correction limit. Correction history:\nAttempt 1 (report): missing field verification\nAttempt 6 (report): latest missing field status\nSELF-REPAIR REQUIRED: old instruction";
    let context = resume_failure_context(legacy);
    assert!(context.contains("latest missing field status"));
    assert!(!context.contains("exhaust"));
    assert!(!context.contains("Correction history"));
    assert!(!context.contains("old instruction"));
    assert!(!context.contains("missing field verification"));
}

#[test]
fn publication_failure_retries_without_reimplementing() {
    let s = Sandbox::new();
    let calls = Arc::new(AtomicUsize::new(0));
    fs::write(s.root.join("offline"), "").unwrap();
    assert!(s.run("complete", calls.clone()).is_err());
    assert!(load(&s.repo, &s.ticket).unwrap().verified_head.is_some());
    fs::remove_file(s.root.join("offline")).unwrap();
    assert_eq!(
        s.run("complete", calls.clone()).unwrap().status,
        ImplementationStatus::AwaitingReview
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}
#[test]
fn changed_ticket_and_concurrent_run_are_rejected() {
    let s = Sandbox::new();
    let calls = Arc::new(AtomicUsize::new(0));
    assert!(s.run("cancel", calls.clone()).is_err());
    let dir = state_dir(&s.repo, &s.ticket).unwrap();
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(dir.join("run.lock"))
        .unwrap();
    lock.lock().unwrap();
    assert!(
        s.run("complete", calls.clone())
            .unwrap_err()
            .to_string()
            .contains("already being implemented")
    );
    drop(lock);
    fs::write(s.repo.join(&s.ticket), "# Different scope").unwrap();
    assert!(
        s.run("complete", calls.clone())
            .unwrap_err()
            .to_string()
            .contains("Ticket changed")
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}
#[test]
fn mismatched_saved_task_repository_is_not_modified() {
    let s = Sandbox::new();
    let calls = Arc::new(AtomicUsize::new(0));
    assert!(s.run("cancel", calls.clone()).is_err());
    let mut state = load(&s.repo, &s.ticket).unwrap();
    state.task_repository = s.repo.clone();
    save(&state_dir(&s.repo, &s.ticket).unwrap(), &state).unwrap();
    assert!(
        s.run("complete", calls.clone())
            .unwrap_err()
            .to_string()
            .contains("allocation")
    );
    assert!(!s.repo.join("implemented.txt").exists());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}
