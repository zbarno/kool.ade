use super::*;

pub(in crate::app::root) fn fixture() -> KooladeApp {
    static NEXT_FIXTURE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let next = || NEXT_FIXTURE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let chat_root = std::env::temp_dir().join(format!(
        "koolade-board-chat-{}-{}",
        std::process::id(),
        next()
    ));
    let chat_slug = chat_root.to_string_lossy().into_owned();
    let test_root = std::env::temp_dir().join(format!(
        "koolade-board-ui-fixture-{}-{}",
        std::process::id(),
        next()
    ));
    let setup_state_root = std::env::temp_dir().join(format!(
        "koolade-board-setup-state-{}-{}",
        std::process::id(),
        next()
    ));
    std::fs::create_dir_all(&test_root).unwrap();
    let code_root = test_root.join("workspace");
    let planning_root = test_root.join("planning-repository");
    let git = |root: &std::path::Path, args: &[&str]| {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?} in {}: {}",
            root.display(),
            String::from_utf8_lossy(&output.stderr)
        );
    };
    for root in [&code_root, &planning_root] {
        std::fs::create_dir_all(root).unwrap();
        git(root, &["init", "-q", "-b", "main"]);
        git(root, &["config", "user.name", "Koolade Test"]);
        git(root, &["config", "user.email", "koolade@example.test"]);
    }
    std::fs::write(code_root.join("README.md"), "isolated UI fixture\n").unwrap();
    git(&code_root, &["add", "README.md"]);
    git(&code_root, &["commit", "-q", "-m", "UI fixture code"]);

    let docs = ["First task", "Review task", "Merged task"]
        .iter()
        .enumerate()
        .map(|(i, title)| crate::artifacts::task_docs::TaskDocument {
            path: format!("planning/tasks/fixture/{:03}-task.md", i + 1),
            title: title.to_string(),
            text: format!("# {title}\n\nUnique story detail {i}"),
            identity: None,
            metadata: None,
            task_state: None,
            metadata_error: None,
        })
        .collect::<Vec<_>>();
    for doc in &docs {
        let path = planning_root.join(&doc.path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, &doc.text).unwrap();
    }

    let workspace_id = uuid::Uuid::new_v4();
    let workspace_manifest = crate::core::managed_workspace::WorkspaceManifest::new(workspace_id);
    std::fs::write(
        planning_root.join("manifest.json"),
        serde_json::to_vec_pretty(&workspace_manifest).unwrap(),
    )
    .unwrap();
    let repositories = crate::core::project_repos::ProjectManifest {
        repositories: vec![crate::core::project_repos::Repository {
            id: "root".into(),
            role: "Primary code".into(),
            remote: String::new(),
            display_name: None,
        }],
    };
    let project_manifest_path =
        planning_root.join(crate::artifacts::planning_store::paths::PROJECT_MANIFEST);
    std::fs::create_dir_all(project_manifest_path.parent().unwrap()).unwrap();
    std::fs::write(
        project_manifest_path,
        serde_json::to_vec_pretty(&repositories).unwrap(),
    )
    .unwrap();
    git(&planning_root, &["add", "."]);
    git(
        &planning_root,
        &["commit", "-q", "-m", "UI fixture planning"],
    );
    let planning_store = crate::artifacts::planning_store::PlanningStore::new(
        workspace_id,
        &planning_root,
        crate::artifacts::planning_store::StoreMode::ManagedLocal,
    );

    let mut states = std::collections::BTreeMap::new();
    for (i, pr_state) in [(1, "OPEN"), (2, "MERGED")] {
        let record = crate::core::implementation::Implementation {
            ticket: docs[i].path.clone(),
            task_uid: None,
            workspace_uid: None,
            ticket_text: docs[i].text.clone(),
            approved_specification: None,
            approved_product_context: None,
            completed_dependency_context: None,
            manager_repair_context: None,
            branch: "koolade/fixture".into(),
            source_branch: None,
            destination_branch: None,
            source_ref: None,
            source_commit: None,
            repository_id: None,
            project_id: None,
            repository_identity: None,
            push_repository: None,
            repository_cache: None,
            task_repository_allocation_key: None,
            base: "main".into(),
            base_commit: "fixture".into(),
            task_repository: code_root.join("worktree"),
            task_repository_kind: crate::core::implementation::TaskRepositoryKind::LegacyWorktree,
            task_repository_ready: false,
            task_repositories: vec![code_root.join("worktree")],
            task_repository_commits: std::collections::BTreeMap::new(),
            status: if i == 1 {
                ImplementationStatus::AwaitingReview
            } else {
                ImplementationStatus::Completed
            },
            detail: String::new(),
            pr_url: Some(format!("https://github.com/fixture/repo/pull/{i}")),
            verified_head: Some("fixture".into()),
            auto_merge: false,
            merged_commit: None,
            pr_state: PullRequestState::parse_legacy(pr_state),
            pr_details: None,
            pr_checked_at: None,
            pr_check_attempted_at: None,
            pr_check_error: None,
            independent_check: None,
            cleanup: Default::default(),
        };
        states.insert(record.ticket.clone(), record);
    }
    KooladeApp {
        _test_temp_roots: vec![
            crate::app::root::TestTempRoot(test_root),
            crate::app::root::TestTempRoot(chat_root),
            crate::app::root::TestTempRoot(setup_state_root.clone()),
        ],
        setup_task_state_path: setup_state_root.join("machine-setup").join("tasks.json"),
        screen: Screen::Connected(Box::new(Project {
            specification_trace_navigation: None,
            task_chats: Default::default(),
            activity: Default::default(),
            state: crate::core::state::PlannerState::load_with_store(&code_root, &planning_store)
                .unwrap(),
            managed_workspace: None,
            planning_sync: crate::core::planning_sync::PlanningSyncStatus::local(),
            planning_remote_url: None,
            chat_slug,
            chat: Vec::new(),
            draft: String::new(),
            queue: Default::default(),
            queue_lock: None,
            active_implementations: Default::default(),
            implementation_states: states,
            pr_refresh: None,
            reconciliation: Default::default(),
            investigation: None,
            investigation_attempted: Default::default(),
            investigation_cooldown_until: None,
            last_pr_refresh: None,
            active_turn: None,
            task_turns: Default::default(),
            task_live: Default::default(),
            planning_work: Default::default(),
            active_planning_work: None,
            live_progress: Default::default(),
            next_question_id: None,
            git: Default::default(),
            task_documents: docs,
            archived_tasks: Default::default(),
            cancelled_work: Default::default(),
        })),
        ..Default::default()
    }
}
