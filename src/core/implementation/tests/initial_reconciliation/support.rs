use super::*;

pub(in crate::core::implementation::tests) fn save_clone_reconciliation_state(
    sandbox: &Sandbox,
    ticket: &str,
    worktree: &Path,
    base: &str,
    local: &str,
    remote: &str,
    common: &str,
) -> std::path::PathBuf {
    use crate::core::implementation::repository_cache::RepositoryCache;
    use std::time::{Duration, Instant};

    super::super::sandbox::approve_ticket_git_intent(sandbox, ticket, base, base, local);
    let dir = state_dir(&sandbox.planning_store.root, ticket).unwrap();
    fs::create_dir_all(&dir).unwrap();
    let (progress, _updates) = mpsc::channel();
    let runner = Runner {
        gh: sandbox.gh.to_string_lossy().into_owned(),
        runtime_config_source: None,
        deadline: Instant::now() + Duration::from_secs(60),
        cancel: Arc::new(AtomicBool::new(false)),
        progress,
        authorization_verifier: None,
    };
    let planning = sandbox.planning_store.clone();
    let cache = RepositoryCache::open(&sandbox.repo, &planning, "root", &runner).unwrap();
    cache
        .import_local_branch(&sandbox.repo, "main", local, &runner)
        .unwrap();
    let source_branch = cache.pin_source(remote, &runner).unwrap();
    let task_key = crate::core::implementation::key_for_ticket(ticket);
    let project_id =
        crate::core::implementation::task_repository::project_id(&sandbox.planning_store.root)
            .unwrap();
    let task_repository = crate::core::implementation::task_repository::task_path(
        &sandbox.planning_store.root,
        "root",
        ticket,
    )
    .unwrap();
    fs::create_dir_all(task_repository.parent().unwrap()).unwrap();
    let identity = crate::core::implementation::repository_cache::load_task_git_identity(
        &dir,
        &sandbox.repo,
        &runner,
    )
    .unwrap();
    cache
        .create_clone(
            &source_branch,
            remote,
            &format!("koolade/{task_key}"),
            &task_repository,
            &identity,
            &runner,
        )
        .unwrap();
    sandbox.git(
        &task_repository,
        &["fetch", "--no-tags", sandbox.repo.to_str().unwrap(), local],
    );
    sandbox.git(
        &task_repository,
        &["merge", "--no-ff", "--no-commit", "--no-edit", local],
    );
    copy_workspace_files(worktree, &task_repository);
    for path in sandbox
        .git(worktree, &["diff", "--cached", "--name-only"])
        .lines()
    {
        if !path.is_empty() {
            sandbox.git(&task_repository, &["add", "--", path]);
        }
    }
    crate::core::implementation::initial_reconciliation::save_clone_plan(
        &dir,
        base,
        local,
        remote,
        common,
        &[],
        crate::core::implementation::initial_reconciliation::CloneRepositoryPlan {
            repository_id: "root".into(),
            repository_identity: cache.identity.clone(),
            source_ref: base.into(),
            destination_branch: base.into(),
        },
    )
    .unwrap();
    for (side, commit) in [("local", local), ("remote", remote)] {
        sandbox.git(
            &sandbox.repo,
            &[
                "update-ref",
                &format!("refs/koolade-reconciliations/{task_key}/{side}"),
                commit,
            ],
        );
    }
    let raw_ticket_text = String::from_utf8(sandbox.planning_store.read(ticket).unwrap()).unwrap();
    let ticket_text = crate::artifacts::task_docs::visible_content(&raw_ticket_text);
    let task_uid = crate::domain::ArtifactIdentity::from_markdown(&raw_ticket_text)
        .unwrap()
        .map(|identity| identity.uid);
    let state: Implementation = serde_json::from_value(serde_json::json!({
        "ticket": ticket,
        "ticket_text": ticket_text,
        "approved_specification": ticket_text,
        "task_uid": task_uid,
        "branch": format!("koolade/{task_key}"),
        "source_branch": base,
        "destination_branch": base,
        "base": base,
        "base_commit": remote,
        "source_ref": "main",
        "source_commit": local,
        "repository_id": "root",
        "project_id": project_id,
        "workspace_uid": planning.project_id.hyphenated().to_string(),
        "repository_identity": cache.identity,
        "push_repository": cache.push_identity_url,
        "repository_cache": cache.path,
        "task_repository_allocation_key": task_key,
        "task_repository": task_repository,
        "task_repository_kind": "clone",
        "task_repository_ready": true,
        "task_repositories": [task_repository],
        "status": "preparing",
        "detail": ""
    }))
    .unwrap();
    save(&dir, &state).unwrap();
    dir
}

fn copy_workspace_files(source: &Path, destination: &Path) {
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        if entry.file_name() == ".git" {
            continue;
        }
        let target = destination.join(entry.file_name());
        let metadata = entry.file_type().unwrap();
        if metadata.is_dir() {
            fs::create_dir_all(&target).unwrap();
            copy_workspace_files(&entry.path(), &target);
        } else if metadata.is_file() {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

pub(in crate::core::implementation::tests) fn complete_report(command: &str) -> serde_json::Value {
    let criteria = crate::core::implementation::initial_reconciliation::CONTRACT
        .lines()
        .filter_map(|line| line.strip_prefix("- "))
        .map(|criterion| serde_json::json!({"criterion":criterion,"evidence":"Both histories were checked and the combined baseline verification passed."}))
        .collect::<Vec<_>>();
    serde_json::json!({
        "schemaVersion":2,
        "status":"complete",
        "blocker_disposition":"none",
        "summary":"Both starting histories are integrated and the combined baseline passed its checks.",
        "acceptance_criteria":criteria,
        "verification":[command],
        "remaining":[],
        "human_choices":[]
    })
}

pub(in crate::core::implementation::tests) fn outcome(
    value: serde_json::Value,
) -> crate::harness::HarnessOutcome {
    crate::harness::HarnessOutcome {
        final_text: value.to_string(),
        envelope: None,
        stderr_tail: String::new(),
    }
}
