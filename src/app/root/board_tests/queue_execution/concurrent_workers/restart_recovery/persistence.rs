pub(super) struct PartialClones {
    pub(super) project_id: String,
    pub(super) paths: Vec<std::path::PathBuf>,
    pub(super) shas: Vec<String>,
    repository_cache: std::path::PathBuf,
    repository_identity: String,
    push_repository: Option<String>,
}

pub(super) fn git(root: &std::path::Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .unwrap();
    assert!(output.status.success(), "git command failed");
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

pub(super) fn prepare_partial_clones(
    store: &crate::artifacts::planning_store::PlanningStore,
    repository: &std::path::Path,
    documents: &[crate::artifacts::task_docs::TaskDocument],
    tasks: &mut [crate::core::planning_work::Work],
) -> PartialClones {
    let project_id = crate::persistence::project_slug(&store.root.canonicalize().unwrap());
    let clone_root = crate::persistence::project_dir(&project_id).join("task-repositories/root");
    std::fs::create_dir_all(&clone_root).unwrap();
    // Model a mapped code repository with one shared remote: production cache
    // refresh resolves destination refs from this origin, not from each task
    // clone. Keep the bare remote local to the fixture.
    let remote = store.root.join("restart-code-remote.git");
    let output = std::process::Command::new("git")
        .args(["clone", "-q", "--bare"])
        .arg(repository)
        .arg(&remote)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "bare remote creation failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    git(
        repository,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    let mut manifest = crate::core::project_repos::ProjectManifest::load(store).unwrap();
    manifest.repositories[0].remote = "https://github.com/fixture/root.git".into();
    std::fs::write(
        store
            .root
            .join(crate::artifacts::planning_store::paths::PROJECT_MANIFEST),
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
    for document in documents {
        let allocation = crate::core::implementation::key_for_ticket(&document.path);
        let branch = format!("koolade/{allocation}");
        git(repository, &["branch", &branch, "main"]);
    }
    git(repository, &["push", "origin", "--all"]);
    git(repository, &["remote", "set-head", "origin", "main"]);
    let (repository_cache, repository_identity, push_repository) =
        crate::core::implementation::prepare_repository_cache_for_test(store, repository, "root")
            .unwrap();
    let origin = repository.to_string_lossy().into_owned();
    let mut paths = Vec::new();
    let mut shas = Vec::new();
    for (index, progress) in [
        "first child partial output",
        "second child partial output",
        "pending integration output",
    ]
    .into_iter()
    .enumerate()
    {
        let allocation = crate::core::implementation::key_for_ticket(&documents[index].path);
        let path = clone_root.join(&allocation);
        let output = std::process::Command::new("git")
            .args(["clone", "-q", &origin, path.to_str().unwrap()])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "clone failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        git(&path, &["config", "user.name", "Recovery Test"]);
        git(&path, &["config", "user.email", "recovery@example.test"]);
        let branch = format!("koolade/{allocation}");
        git(&path, &["checkout", "-q", "-b", &branch]);
        std::fs::write(path.join("partial-progress.txt"), progress).unwrap();
        let sha = git(&path, &["rev-parse", "HEAD"]);
        let intent = crate::core::git_intent::GitIntentV1::approve(
            "root".into(),
            branch,
            sha.clone(),
            "main".into(),
            crate::core::git_intent::PublicationModeV1::HoldForReview,
            None,
        )
        .unwrap();
        intent.validate().unwrap();
        persist_git_intent(store, &documents[index], &intent);
        tasks[index].source_branch = Some(intent.source_ref.clone());
        tasks[index].destination_branch = Some(intent.destination_ref.clone());
        tasks[index].git_intent = Some(intent.clone());
        tasks[index].git_intents.insert("root".into(), intent);
        assert!(git(&path, &["status", "--porcelain"]).contains("partial-progress.txt"));
        paths.push(path);
        shas.push(sha);
    }
    PartialClones {
        project_id,
        paths,
        shas,
        repository_cache,
        repository_identity,
        push_repository,
    }
}

pub(super) fn save_partial_states(
    store: &crate::artifacts::planning_store::PlanningStore,
    documents: &[crate::artifacts::task_docs::TaskDocument],
    tasks: &[crate::core::planning_work::Work],
    clones: &PartialClones,
    states: &mut std::collections::BTreeMap<String, crate::core::implementation::Implementation>,
) -> Vec<crate::core::implementation::Implementation> {
    let template = states
        .values()
        .next()
        .expect("fixture implementation template")
        .clone();
    let mut saved = Vec::new();
    for index in 0..3 {
        let mut state = template.clone();
        state.ticket = documents[index].path.clone();
        state.task_uid = Some(tasks[index].uid.clone());
        state.project_id = Some(clones.project_id.clone());
        state.workspace_uid = Some(store.project_id.hyphenated().to_string());
        state.repository_id = Some("root".into());
        let stored = String::from_utf8(store.read(&state.ticket).unwrap()).unwrap();
        state.ticket_text = crate::artifacts::task_docs::visible_content(&stored);
        let allocation = crate::core::implementation::key_for_ticket(&state.ticket);
        state.task_repository_allocation_key = Some(allocation.clone());
        state.branch = format!("koolade/{allocation}");
        state.source_branch = Some(state.branch.clone());
        state.source_ref = Some(state.branch.clone());
        state.destination_branch = Some("main".into());
        state.source_commit = Some(clones.shas[index].clone());
        state.repository_cache = Some(clones.repository_cache.clone());
        state.repository_identity = Some(clones.repository_identity.clone());
        state.push_repository = clones.push_repository.clone();
        state.base = "main".into();
        state.base_commit = clones.shas[index].clone();
        state.task_repository = clones.paths[index].clone();
        state.task_repository_kind = crate::core::implementation::TaskRepositoryKind::Clone;
        state.task_repository_ready = true;
        state.task_repositories = vec![clones.paths[index].clone()];
        state.task_repository_commits =
            std::collections::BTreeMap::from([("root".into(), clones.shas[index].clone())]);
        state.pr_url = None;
        state.pr_state = None;
        state.pr_details = None;
        state.status = match index {
            0 => crate::core::implementation::ImplementationStatus::Interrupted,
            2 => crate::core::implementation::ImplementationStatus::WaitingToMerge,
            _ => crate::core::implementation::ImplementationStatus::Implementing,
        };
        let state_dir = crate::core::implementation::state_dir_for_task(
            &store.root,
            &state.ticket,
            state.task_uid.as_deref(),
        )
        .unwrap();
        crate::core::implementation::save(&state_dir, &state).unwrap();
        states.insert(state.ticket.clone(), state.clone());
        saved.push(state);
    }
    saved
}

pub(super) fn persist_git_intent(
    store: &crate::artifacts::planning_store::PlanningStore,
    document: &crate::artifacts::task_docs::TaskDocument,
    intent: &crate::core::git_intent::GitIntentV1,
) {
    let metadata = document
        .metadata
        .as_ref()
        .expect("fixture task metadata")
        .clone()
        .with_git_intent(Some(intent))
        .unwrap();
    let stored = String::from_utf8(store.read(&document.path).unwrap()).unwrap();
    let embedded = crate::artifacts::task_docs::embed(&stored, &metadata).unwrap();
    store
        .atomic_write(&document.path, embedded.as_bytes())
        .unwrap();
}

pub(super) fn persist_fixture_identities(
    store: &crate::artifacts::planning_store::PlanningStore,
    documents: &[crate::artifacts::task_docs::TaskDocument],
) {
    for doc in documents {
        let mut markdown = String::from_utf8(store.read(&doc.path).unwrap()).unwrap();
        assert!(
            crate::domain::ArtifactIdentity::from_markdown(&markdown)
                .unwrap()
                .is_none(),
            "fixture identity marker is inserted once"
        );
        let identity = serde_json::to_string(doc.identity.as_ref().unwrap()).unwrap();
        markdown.push_str(&format!("\n<!-- koolade-artifact-id:v1 {identity} -->\n"));
        store.atomic_write(&doc.path, markdown.as_bytes()).unwrap();
    }
}

pub(super) fn reload_task_documents(
    store: &crate::artifacts::planning_store::PlanningStore,
    paths: &[String],
) -> Vec<crate::artifacts::task_docs::TaskDocument> {
    paths
        .iter()
        .map(|path| {
            let markdown = String::from_utf8(store.read(path).unwrap()).unwrap();
            let identity = crate::domain::ArtifactIdentity::from_markdown(&markdown)
                .unwrap()
                .expect("persisted task identity");
            let metadata = crate::artifacts::task_docs::parse_metadata(&markdown)
                .unwrap()
                .expect("persisted task metadata");
            assert_eq!(identity.uid, metadata.uid);
            crate::artifacts::task_docs::TaskDocument {
                path: path.clone(),
                title: identity.title.clone(),
                text: markdown,
                identity: Some(identity),
                metadata: Some(metadata),
                task_state: None,
                metadata_error: None,
            }
        })
        .collect()
}

pub(super) fn assert_implementation_recovery(
    actual: &std::collections::BTreeMap<String, crate::core::implementation::Implementation>,
    saved: &[crate::core::implementation::Implementation],
    task_uids: &[String],
) {
    let git = |root: &std::path::Path, args: &[&str]| {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .unwrap();
        assert!(output.status.success());
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    };
    for (index, saved) in saved.iter().enumerate() {
        let restored = actual
            .get(&saved.ticket)
            .expect("reloaded implementation state");
        assert_eq!(
            restored.task_uid.as_deref(),
            Some(task_uids[index].as_str())
        );
        assert_eq!(restored.task_repositories, saved.task_repositories);
        assert_eq!(restored.base_commit, saved.base_commit);
        assert_eq!(restored.source_commit, saved.source_commit);
        assert_eq!(restored.branch, saved.branch);
        assert_eq!(
            restored.task_repository_kind,
            crate::core::implementation::TaskRepositoryKind::Clone
        );
        assert_eq!(restored.status, saved.status);
        assert_ne!(
            restored.status,
            crate::core::implementation::ImplementationStatus::Completed
        );
        assert!(
            git(&restored.task_repository, &["status", "--porcelain"])
                .contains("partial-progress.txt")
        );
    }
    assert_eq!(
        actual[&saved[2].ticket].status,
        crate::core::implementation::ImplementationStatus::WaitingToMerge
    );
}
