use super::*;

struct Paths {
    root: PathBuf,
    ticket: String,
    source: PathBuf,
    target: PathBuf,
    source_activity: Vec<u8>,
    previous_activity: Vec<u8>,
}

fn setup(tag: &str, conflicting_report: bool) -> Paths {
    let root = repo(tag);
    let batch = format!(
        "{}/CHG-900-migration",
        crate::artifacts::layout::canonical::TASKS
    );
    let ticket = format!("{batch}/CHG-900-TASK-preserve-implementation.md");
    let text = "# CHG-900-TASK-preserve-implementation — Preserve implementation state\n\n## Ticket goal\n\nKeep all implementation evidence attached to this story.\n";
    fs::create_dir_all(root.join(&batch)).unwrap();
    fs::write(root.join(&ticket), text).unwrap();
    commit_all(&root, "add migration story");

    let source_key = crate::core::implementation::key_for_ticket(
        "planning/tasks/CHG-900-migration/CHG-900-TASK-preserve-implementation.md",
    );
    let target_key = crate::core::implementation::key_for_ticket(&ticket);
    assert_ne!(source_key, target_key);
    let source = root
        .join(crate::artifacts::layout::canonical::IMPLEMENTATION)
        .join(source_key);
    let target = root
        .join(crate::artifacts::layout::canonical::IMPLEMENTATION)
        .join(target_key);
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(&target).unwrap();
    fs::write(source.join("run.lock"), []).unwrap();
    fs::write(target.join("run.lock"), []).unwrap();
    fs::write(source.join("report.json"), b"source report").unwrap();
    fs::write(target.join("previous-note.txt"), b"destination evidence").unwrap();
    fs::write(source.join("shared.txt"), b"same evidence").unwrap();
    fs::write(target.join("shared.txt"), b"same evidence").unwrap();
    if conflicting_report {
        fs::write(target.join("report.json"), b"different destination report").unwrap();
    }

    let previous = progress(10, "Needs attention");
    let current = progress(20, "Implementing");
    let previous_activity = serde_json::to_vec(&previous).unwrap();
    let source_activity = serde_json::to_vec(&current).unwrap();
    fs::write(target.join("activity.json"), &previous_activity).unwrap();
    fs::write(source.join("activity.json"), &source_activity).unwrap();
    let state = crate::core::implementation::Implementation {
        ticket: ticket.clone(),
        task_uid: None,
        ticket_text: text.to_owned(),
        approved_specification: None,
        approved_product_context: None,
        completed_dependency_context: None,
        branch: "koolade/legacy-state-key".into(),
        source_branch: None,
        destination_branch: None,
        base: "master".into(),
        base_commit: "base".into(),
        worktree: root.join("worktree"),
        status: crate::core::implementation::ImplementationStatus::Blocked,
        detail: "Resume this preserved task state.".into(),
        pr_url: None,
        verified_head: None,
        auto_merge: false,
        merged_commit: None,
        pr_state: None,
        pr_checked_at: None,
        pr_check_attempted_at: None,
        pr_check_error: None,
        independent_check: None,
        cleanup: Default::default(),
    };
    fs::write(
        source.join("state.json"),
        crate::core::implementation::serialize_state(&state).unwrap(),
    )
    .unwrap();
    Paths {
        root,
        ticket,
        source,
        target,
        source_activity,
        previous_activity,
    }
}

fn progress(updated_ms: i64, activity: &str) -> crate::harness::LiveProgress {
    crate::harness::LiveProgress {
        telemetry: crate::harness::ActivityTelemetry {
            started_ms: Some(updated_ms),
            updated_ms: Some(updated_ms),
            finished_ms: None,
            updates: 1,
            samples: vec![(updated_ms / 10_000, 1)],
        },
        selected_route: None,
        model_calls: Vec::new(),
        checklist: Vec::new(),
        checklist_revision: 0,
        posts: vec![],
        thoughts: String::new(),
        response: String::new(),
        specification: None,
        activity: Some(activity.to_owned()),
    }
}

#[test]
fn evidence_only_destination_merges_restartably_and_keeps_older_activity() {
    let paths = setup("implementation-evidence-merge", false);

    // Simulate interruption after evidence and activity were copied, but
    // before the migrated implementation state was written.
    let archive_name = format!(
        "activity-migration-{:016x}.json",
        crate::persistence::fnv1a64(&paths.previous_activity)
    );
    fs::write(paths.target.join("report.json"), b"source report").unwrap();
    fs::write(paths.target.join("activity.json"), &paths.source_activity).unwrap();
    fs::write(paths.target.join(archive_name), &paths.previous_activity).unwrap();
    assert!(!paths.target.join("state.json").exists());

    run(&paths.root).unwrap();

    let state = crate::core::implementation::load(&paths.root, &paths.ticket).unwrap();
    assert_eq!(
        state.status,
        crate::core::implementation::ImplementationStatus::Blocked
    );
    assert!(state.task_uid.is_some());
    assert!(!paths.source.exists());
    assert_eq!(
        fs::read(paths.target.join("activity.json")).unwrap(),
        paths.source_activity
    );
    assert_eq!(
        fs::read(paths.target.join("report.json")).unwrap(),
        b"source report"
    );
    assert_eq!(
        fs::read(paths.target.join("previous-note.txt")).unwrap(),
        b"destination evidence"
    );
    assert_eq!(
        fs::read(paths.target.join("shared.txt")).unwrap(),
        b"same evidence"
    );
    let archived = fs::read_dir(&paths.target)
        .unwrap()
        .flatten()
        .map(|entry| entry.path())
        .find(|path| {
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("activity-migration-"))
        })
        .expect("older activity snapshot remains as migration evidence");
    assert_eq!(fs::read(archived).unwrap(), paths.previous_activity);
    assert!(run(&paths.root).unwrap().is_empty());
    let _ = fs::remove_dir_all(paths.root);
}

#[test]
fn conflicting_same_name_implementation_evidence_stops_before_mutation() {
    let paths = setup("implementation-evidence-conflict", true);

    let error = run(&paths.root).unwrap_err().to_string();

    assert!(error.contains("Conflicting implementation evidence at report.json"));
    assert!(paths.source.join("state.json").is_file());
    assert!(!paths.target.join("state.json").exists());
    assert_eq!(
        fs::read(paths.source.join("activity.json")).unwrap(),
        paths.source_activity
    );
    assert_eq!(
        fs::read(paths.target.join("activity.json")).unwrap(),
        paths.previous_activity
    );
    assert!(
        !paths
            .root
            .join(crate::artifacts::layout::canonical::MANIFEST)
            .exists()
    );
    assert!(
        !paths
            .root
            .join(".git/koolade-artifact-migration.pending.json")
            .exists()
    );
    let _ = fs::remove_dir_all(paths.root);
}
