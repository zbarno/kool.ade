//! Two installations share Git while keeping task stores and conversations
//! local. Unix exercises the spec publication coordinator with fake `gh`;
//! setup uses temporary Git repositories (moderate, no Docker/network).

use super::*;
use crate::{
    app::welcome::attempt_connect,
    core::planning_work::{Work, WorkKind, load as load_work, save as save_work},
    persistence::runtime_target_approvals::{self, Binding},
};
use std::{path::Path, process::Command};
use tempfile::TempDir;

#[cfg(not(unix))]
#[path = "cross_installation_remote/legacy.rs"]
mod legacy;
#[cfg(unix)]
#[path = "cross_installation_remote/publication.rs"]
mod publication;

const SPEC: &str = "docs/koolade/product/overview.md";

/// Production boundary: app connection gives each installation an independent
/// local store, then the real spec publication coordinator gates both PRs on
/// the pinned integration SHA. The invariant is stale publication rejection
/// followed by a refreshed, explicitly resolved publication with both edits.
/// Direct Git merge tests cannot prove this provider/consent sequence. Uses
/// local Git, a bare remote, and fake `gh` on Unix; no Docker/network, moderate
/// temporary-repository setup.
#[test]
fn separate_installations_fetch_merged_specs_without_sharing_local_authority() {
    let temp = TempDir::new().unwrap();
    let remote = temp.path().join("shared-code.git");
    let first_parent = temp.path().join("installation-one");
    let second_parent = temp.path().join("installation-two");
    let first_code = first_parent.join("shared-product");
    let second_code = second_parent.join("shared-product");
    let first_home = temp.path().join("install-one");
    let second_home = temp.path().join("install-two");
    std::fs::create_dir_all(&first_parent).unwrap();
    std::fs::create_dir_all(&second_parent).unwrap();

    git(
        temp.path(),
        &[
            "init",
            "--quiet",
            "--bare",
            "--initial-branch=main",
            remote.to_str().unwrap(),
        ],
    );
    git(
        temp.path(),
        &[
            "clone",
            "--quiet",
            remote.to_str().unwrap(),
            first_code.to_str().unwrap(),
        ],
    );
    set_identity(&first_code);
    crate::core::state::test_support::seed_published_product_document(&first_code).unwrap();
    git(
        &first_code,
        &["push", "--quiet", "--set-upstream", "origin", "main"],
    );
    git(&first_code, &["remote", "set-head", "origin", "main"]);
    let initial_sha = git(&first_code, &["rev-parse", "HEAD"]);
    git(
        temp.path(),
        &[
            "clone",
            "--quiet",
            remote.to_str().unwrap(),
            second_code.to_str().unwrap(),
        ],
    );
    set_identity(&second_code);
    git(&second_code, &["remote", "set-head", "origin", "main"]);

    let first = {
        let _home = crate::test_support::KooladeHomeGuard::set(&first_home);
        attempt_connect(first_code.to_str().unwrap()).unwrap()
    };
    let second = {
        let _home = crate::test_support::KooladeHomeGuard::set(&second_home);
        attempt_connect(second_code.to_str().unwrap()).unwrap()
    };
    assert_eq!(
        crate::persistence::project_slug(&first_code),
        crate::persistence::project_slug(&second_code),
        "both checkouts must resolve to the shared origin identity"
    );
    assert_eq!(
        first.state.planning_store.project_id,
        second.state.planning_store.project_id
    );
    assert_ne!(
        first.state.planning_store.root,
        second.state.planning_store.root
    );
    assert_eq!(
        first.state.published_spec_pin.as_ref().unwrap().commit_sha,
        initial_sha
    );
    assert_eq!(
        first.state.published_spec_pin, second.state.published_spec_pin,
        "both installations must pin the same fetched published commit"
    );
    assert_eq!(first.state.spec_text, second.state.spec_text);

    let mut task = Work::new(
        String::new(),
        "Private task in installation one".into(),
        "Only installation one owns this request.".into(),
        "Todo".into(),
    );
    task.kind = WorkKind::Feature;
    task.key = format!("task:{}", task.uid);
    task.task_intent_uid = Some(task.uid.clone());
    let task_uid = task.uid.clone();
    save_work(&first.state.planning_store, &[task]).unwrap();
    assert_eq!(load_work(&first.state.planning_store).unwrap().len(), 1);
    assert!(load_work(&second.state.planning_store).unwrap().is_empty());

    let project_slug = crate::persistence::project_slug(&first_code);
    let runtime_approval = approval(
        &first.state.planning_store.project_id,
        &task_uid,
        &initial_sha,
    );
    {
        let _home = crate::test_support::KooladeHomeGuard::set(&first_home);
        let mut chats = crate::persistence::task_chats::TaskChats::default();
        chats.ensure_loaded(&project_slug);
        chats
            .append(
                &project_slug,
                &task_uid,
                vec![crate::domain::ChatMessage::new(
                    crate::domain::ChatRole::User,
                    "Private conversation marker for installation one.",
                    None,
                )],
            )
            .unwrap();
        runtime_target_approvals::save(&runtime_approval).unwrap();
    }
    {
        let _home = crate::test_support::KooladeHomeGuard::set(&second_home);
        let mut chats = crate::persistence::task_chats::TaskChats::default();
        chats.ensure_loaded(&project_slug);
        assert!(chats.messages.is_empty());
        assert!(!runtime_target_approvals::contains(&runtime_approval).unwrap());
    }

    #[cfg(unix)]
    let repository_id = first
        .state
        .published_spec_pin
        .as_ref()
        .unwrap()
        .repository_id
        .clone();
    #[cfg(unix)]
    publication::publish_conflicting_updates(
        &first_code,
        &second_code,
        &repository_id,
        first.state.published_spec_pin.as_ref().unwrap(),
        second.state.published_spec_pin.as_ref().unwrap(),
        &temp.path().join("bin"),
    );
    #[cfg(not(unix))]
    legacy::publish_conflicting_updates(&first_code, &second_code);
    git(&first_code, &["fetch", "--quiet", "origin"]);
    git(&second_code, &["fetch", "--quiet", "origin"]);
    let merged_sha = git(&first_code, &["rev-parse", "refs/remotes/origin/main"]);
    assert_eq!(
        merged_sha,
        git(&second_code, &["rev-parse", "refs/remotes/origin/main"]),
        "both installations must fetch the exact merged integration revision"
    );
    assert_ne!(merged_sha, initial_sha);

    let first_refreshed = {
        let _home = crate::test_support::KooladeHomeGuard::set(&first_home);
        let mut store = first.state.planning_store.clone();
        store.published_spec_pin = None;
        PlannerState::load_with_store(&first_code, &store).unwrap()
    };
    let second_refreshed = {
        let _home = crate::test_support::KooladeHomeGuard::set(&second_home);
        let mut store = second.state.planning_store.clone();
        store.published_spec_pin = None;
        PlannerState::load_with_store(&second_code, &store).unwrap()
    };
    assert_eq!(
        first_refreshed
            .published_spec_pin
            .as_ref()
            .unwrap()
            .commit_sha,
        merged_sha
    );
    assert_eq!(
        first_refreshed.published_spec_pin,
        second_refreshed.published_spec_pin
    );
    let merged_text = first_refreshed.spec_text.as_deref().unwrap();
    assert!(merged_text.contains("Published from installation one."));
    assert!(merged_text.contains("Published from installation two."));
    assert_eq!(second_refreshed.spec_text.as_deref(), Some(merged_text));
    assert_eq!(load_work(&first.state.planning_store).unwrap().len(), 1);
    assert!(load_work(&second.state.planning_store).unwrap().is_empty());
    assert_eq!(git(&first_code, &["status", "--porcelain"]), "");
    assert_eq!(git(&second_code, &["status", "--porcelain"]), "");
    assert_eq!(git(&remote, &["rev-parse", "refs/heads/main"]), merged_sha);
}

fn approval(project_id: &uuid::Uuid, task_id: &str, source_sha: &str) -> Binding {
    Binding {
        workspace_id: project_id.hyphenated().to_string(),
        task_id: task_id.to_owned(),
        repository_id: "primary-code".into(),
        source_sha: source_sha.to_owned(),
        target_id: "synthetic-runtime-target".into(),
        proposal_sha256: "b".repeat(64),
        receipt_id: uuid::Uuid::new_v4().hyphenated().to_string(),
    }
}

fn set_identity(repo: &Path) {
    git(repo, &["config", "user.name", "Koolade Test"]);
    git(
        repo,
        &["config", "user.email", "koolade-test@example.invalid"],
    );
}

fn git(repo: &Path, args: &[&str]) -> String {
    let output = run_git(repo, args);
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn run_git(repo: &Path, args: &[&str]) -> std::process::Output {
    Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .unwrap()
}
