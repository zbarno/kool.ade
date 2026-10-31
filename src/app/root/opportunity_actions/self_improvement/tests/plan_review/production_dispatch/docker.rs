use super::setup::ReadyTask;
use crate::{
    core::{
        implementation::{Implementation, ImplementationStatus},
        self_improvement::{SelfImprovementEventV1, SelfImprovementStore},
    },
    execution::{
        contract::{ExecutionModeV1, ResourceBudgetV1, WorkspaceRef},
        docker::{
            backend::{OwnedTaskWorkspaceResolver, TaskWorkspaceResolver},
            images::RuntimeProfile,
        },
        session::ExecutionSession,
    },
};
use std::{path::Path, process::Command, time::Duration};

pub(super) fn execute(ready: &ReadyTask, root: &Path) {
    let run = &ready.run;
    let source_sha = run.source_sha.as_str();
    let source_ref = run.source_ref.as_deref().expect("approved source ref");
    let destination_ref = run
        .destination_ref
        .as_deref()
        .expect("approved destination ref");
    let state_root = root.join("state-root");
    std::fs::create_dir_all(&state_root).unwrap();
    let project_id = crate::persistence::project_slug(&state_root.canonicalize().unwrap());
    let workspace_uid = ready.planning.project_id.hyphenated().to_string();
    let manifest = crate::core::project_repos::ProjectManifest::load(&ready.planning).unwrap();
    let repository_id = manifest
        .repositories
        .iter()
        .find(|repository| {
            crate::core::project_repos::stable_uid(&workspace_uid, "repository", &repository.id)
                == run.target_repository_uid
        })
        .map(|repository| repository.id.as_str())
        .expect("the approved stable repository UID maps to the manifest");
    let allocation = crate::core::implementation::key_for_ticket(&ready.ticket);
    let task_repository = crate::persistence::project_dir(&project_id)
        .join("task-repositories")
        .join(repository_id)
        .join(&allocation);
    std::fs::create_dir_all(task_repository.parent().unwrap()).unwrap();
    let clone = Command::new("git")
        .args(["clone", "--quiet", "--no-hardlinks", "--"])
        .arg(&ready.repo_root)
        .arg(&task_repository)
        .output()
        .unwrap();
    assert!(
        clone.status.success(),
        "approved task clone failed: {}",
        String::from_utf8_lossy(&clone.stderr)
    );
    git(
        &task_repository,
        &["remote", "set-url", "origin", &run.canonical_remote],
    );
    git(
        &task_repository,
        &["checkout", "--quiet", "--detach", source_sha],
    );
    assert_eq!(git(&task_repository, &["rev-parse", "HEAD"]), source_sha);

    let ticket_path = ready.planning.root.join(&ready.ticket);
    assert!(
        !ready.repo_root.join(&ready.ticket).exists(),
        "task tickets stay in the separate planning repository"
    );
    let text = std::fs::read_to_string(ticket_path).unwrap();
    let implementation: Implementation = serde_json::from_value(serde_json::json!({
        "ticket": ready.ticket,
        "task_uid": ready.metadata.uid,
        "ticket_text": text,
        "branch": source_ref,
        "source_branch": source_ref,
        "destination_branch": destination_ref,
        "source_ref": source_ref,
        "source_commit": source_sha,
        "repository_id": repository_id,
        "project_id": project_id,
        "task_repository_allocation_key": allocation,
        "base": destination_ref,
        "base_commit": source_sha,
        "task_repository": task_repository,
        "task_repository_kind": "clone",
        "task_repository_ready": true,
        "task_repositories": [task_repository],
        "status": serde_json::to_value(ImplementationStatus::Preparing).unwrap(),
        "detail": "",
        "pr_url": null,
        "verified_head": null
    }))
    .unwrap();
    let state_dir = state_root.join("task-state");
    std::fs::create_dir_all(&state_dir).unwrap();
    let state_path = state_dir.join("state.json");
    std::fs::write(
        &state_path,
        crate::core::implementation::serialize_state(&implementation).unwrap(),
    )
    .unwrap();
    let verifier = crate::core::implementation::authorize_resolved_task_source_for_test(
        &ready.planning,
        &state_root,
        state_path,
        &ready.metadata,
        &implementation,
    )
    .unwrap()
    .expect("app-generated Evolve task receives execution authorization");
    let generation = verifier.generation();
    verifier.verify(&generation).unwrap();

    let workspace = WorkspaceRef {
        project_id: ready.planning.project_id.hyphenated().to_string(),
        task_id: run.owner_task_uid.clone().unwrap(),
        source_sha: source_sha.to_owned(),
    };
    let resolver =
        OwnedTaskWorkspaceResolver::for_current_user(workspace.clone(), &task_repository)
            .expect("the task clone is allocated in current-user project storage");
    assert_eq!(
        TaskWorkspaceResolver::resolve_owned_clone(&resolver, &workspace).unwrap(),
        task_repository.canonicalize().unwrap()
    );
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR").expect("rootless Docker runtime dir");
    let docker_host = format!("unix://{runtime_dir}/docker.sock");
    assert!(std::env::var_os("DOCKER_CONTEXT").is_none());
    assert_eq!(
        std::env::var("DOCKER_HOST").as_deref(),
        Ok(docker_host.as_str())
    );
    let engine = Command::new("docker")
        .args(["info", "--format", "{{json .SecurityOptions}}"])
        .output()
        .expect("Docker CLI is available in the rootless integration job");
    assert!(
        engine.status.success()
            && String::from_utf8_lossy(&engine.stdout).contains("name=rootless"),
        "Evolve dispatch requires rootless Docker: {}",
        String::from_utf8_lossy(&engine.stderr)
    );

    let session = ExecutionSession::connect_local_with_authorization(
        workspace,
        &task_repository,
        ExecutionModeV1::Implementation,
        RuntimeProfile::Rust,
        ResourceBudgetV1::default(),
        Some(verifier),
    )
    .unwrap_or_else(|error| panic!("app-authorized Evolve Docker session failed: {error:#}"));
    let output = session
        .run_tool_command(
            "touch .self-improvement-authorized",
            Duration::from_secs(30),
        )
        .unwrap_or_else(|error| panic!("authorized Evolve command failed: {error:#}"));
    assert_eq!(output.result.exit_code, Some(0), "{}", output.stderr);

    let runs = SelfImprovementStore::new(&ready.planning);
    let current = runs.load(&run.run_uid).unwrap();
    let cancelled = runs
        .apply_event(
            &run.run_uid,
            current.revision,
            &uuid::Uuid::new_v4().to_string(),
            SelfImprovementEventV1::Cancel,
        )
        .unwrap();
    assert_eq!(
        cancelled.phase,
        crate::core::self_improvement::SelfImprovementPhaseV1::Cancelled
    );
    let denied =
        match session.run_tool_command("touch .self-improvement-denied", Duration::from_secs(30)) {
            Err(error) => error,
            Ok(_) => panic!("cancellation must revoke the Evolve Docker authorization"),
        };
    assert_eq!(
        denied.kind,
        crate::execution::errors::ExecutionErrorKind::PolicyDenied
    );
    assert!(
        denied.message.contains("task dispatch is not authorized"),
        "unexpected post-cancellation denial: {}",
        denied.message
    );
    assert!(
        task_repository
            .join(".self-improvement-authorized")
            .is_file()
    );
    assert!(!task_repository.join(".self-improvement-denied").exists());
}

fn git(repository: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(repository)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}
