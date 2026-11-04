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
    let local_state_root = test_root.join("private-state");
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
    std::fs::create_dir_all(&code_root).unwrap();
    git(&code_root, &["init", "-q", "-b", "main"]);
    git(&code_root, &["config", "user.name", "Koolade Test"]);
    git(
        &code_root,
        &["config", "user.email", "koolade@example.test"],
    );
    std::fs::write(code_root.join("README.md"), "isolated UI fixture\n").unwrap();
    git(&code_root, &["add", "README.md"]);
    git(&code_root, &["commit", "-q", "-m", "UI fixture code"]);
    crate::core::state::test_support::seed_published_product_document(&code_root).unwrap();

    let batch_uid = uuid::Uuid::new_v4().hyphenated().to_string();
    let docs = ["First task", "Review task", "Merged task"]
        .iter()
        .enumerate()
        .map(|(i, title)| {
            let mut identity =
                crate::domain::ArtifactIdentity::new(&format!("TASK-{:03}", i + 1), title);
            identity.parent_uid = Some(batch_uid.clone());
            let metadata =
                crate::artifacts::task_docs::TaskMetadata::new(&identity, "root", vec![]).unwrap();
            let text = format!("# {title}\n\nUnique story detail {i}");
            let path = format!("planning/tasks/fixture/{:03}-task.md", i + 1);
            let encoded = crate::artifacts::task_docs::embed(&text, &metadata).unwrap();
            let file = local_state_root.join(&path);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, encoded).unwrap();
            crate::artifacts::task_docs::TaskDocument {
                path,
                title: title.to_string(),
                text,
                identity: Some(identity),
                metadata: Some(metadata),
                task_state: None,
                metadata_error: None,
            }
        })
        .collect::<Vec<_>>();

    let workspace_id = uuid::Uuid::new_v4();
    let repositories = crate::core::project_repos::ProjectManifest {
        repositories: vec![crate::core::project_repos::Repository {
            id: "root".into(),
            role: "Primary code".into(),
            remote: String::new(),
            display_name: None,
        }],
    };
    let project_manifest_path =
        local_state_root.join(crate::artifacts::planning_store::paths::PROJECT_MANIFEST);
    std::fs::create_dir_all(project_manifest_path.parent().unwrap()).unwrap();
    std::fs::write(
        project_manifest_path,
        serde_json::to_vec_pretty(&repositories).unwrap(),
    )
    .unwrap();
    let planning_store = crate::artifacts::planning_store::PlanningStore::for_code_repository(
        workspace_id,
        &local_state_root,
        &code_root,
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
            task_repository_kind: crate::core::implementation::TaskRepositoryKind::Clone,
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
            pr_state: PullRequestState::parse_wire(pr_state),
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
            task_turn_user_message_ids: Default::default(),
            task_turn_reply_claims: Default::default(),
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
