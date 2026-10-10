use crate::core::managed_workspace::WorkspaceManager;

use super::support::{TestDirectory, code_checkout, registration};

#[test]
fn adding_code_repositories_keeps_the_planning_identity_and_head() {
    let temp = TestDirectory::new("register-repositories");
    let manager = temp.manager("operator");
    let created = manager.create("Several code checkouts").unwrap();
    let (alpha, alpha_remote) = code_checkout(&temp.path, "alpha", true);
    let (beta, beta_remote) = code_checkout(&temp.path, "beta", true);

    let after_alpha = manager
        .register_code_repository(
            created.id,
            registration("alpha", alpha.clone(), alpha_remote),
        )
        .unwrap();
    let after_beta = manager
        .register_code_repository(created.id, registration("beta", beta.clone(), beta_remote))
        .unwrap();

    assert_eq!(after_alpha.id, created.id);
    assert_eq!(after_beta.id, created.id);
    assert_eq!(after_beta.repositories.repositories.len(), 2);
    assert_eq!(after_beta.planning_repo, created.planning_repo);
    assert_eq!(after_beta.manifest.workspace_id, created.id);
    assert_eq!(
        super::support::git_ok(
            Some(&after_beta.planning_repo),
            &["branch", "--show-current"]
        )
        .stdout,
        b"main\n"
    );
    assert_eq!(after_beta.association.code_checkouts.len(), 2);
    assert!(!alpha.join(".koolade-packet").exists());
    assert!(!beta.join(".koolade-packet").exists());
}

#[test]
fn registering_optional_remote_succeeds_without_a_git_origin() {
    let temp = TestDirectory::new("optional-remote");
    let manager = temp.manager("operator");
    let workspace = manager.create("Remote optional").unwrap();
    let (checkout, remote) = code_checkout(&temp.path, "local-only", false);
    assert!(remote.is_none());

    let registered = manager
        .register_code_repository(
            workspace.id,
            registration("local-only", checkout.clone(), None),
        )
        .unwrap();

    assert_eq!(registered.repositories.repositories.len(), 1);
    assert!(registered.repositories.repositories[0].remote.is_empty());
    assert_eq!(
        registered.association.code_checkouts[0].path,
        checkout.canonicalize().unwrap()
    );
}

#[test]
fn retrying_after_a_failed_checkpoint_commits_the_registry_change() {
    let temp = TestDirectory::new("registry-checkpoint-retry");
    let manager = temp.manager("operator");
    let workspace = manager.create("Checkpoint retry").unwrap();
    let (checkout, remote) = code_checkout(&temp.path, "retry", true);
    let registration = registration("retry", checkout.clone(), remote);
    let mut first_attempt = manager.open(workspace.id).unwrap();
    let mut fail_checkpoint =
        |_: &std::path::Path, _: &str, _: &[String]| anyhow::bail!("injected commit failure");

    let error = crate::core::managed_workspace::repositories::register_with_checkpoint(
        &manager,
        &mut first_attempt,
        registration.clone(),
        &mut fail_checkpoint,
    )
    .unwrap_err();
    assert!(format!("{error:#}").contains("injected commit failure"));
    drop(first_attempt);

    let mut retry = manager.open(workspace.id).unwrap();
    let mut commit = |planning_repo: &std::path::Path, message: &str, paths: &[String]| {
        crate::core::gitops::commit(planning_repo, message, paths)
            .map(|_| ())
            .map_err(anyhow::Error::from)
    };
    crate::core::managed_workspace::repositories::register_with_checkpoint(
        &manager,
        &mut retry,
        registration,
        &mut commit,
    )
    .unwrap();

    let reopened = manager.open(workspace.id).unwrap();
    assert_eq!(reopened.repositories.repositories.len(), 1);
    assert_eq!(reopened.association.code_checkouts.len(), 1);
    assert_eq!(
        super::support::git_ok(
            Some(&workspace.planning_repo),
            &["log", "-1", "--format=%s"]
        )
        .stdout,
        b"Kool.ad/e: register code repository\n"
    );
    assert!(
        super::support::git_ok(
            Some(&workspace.planning_repo),
            &["status", "--short", "--", "config/repositories.json"]
        )
        .stdout
        .is_empty()
    );
}

#[test]
fn duplicate_repository_id_or_remote_fails_without_changing_shared_registry() {
    let temp = TestDirectory::new("duplicate-repository");
    let manager = temp.manager("operator");
    let workspace = manager.create("Unique code repositories").unwrap();
    let (alpha, alpha_remote) = code_checkout(&temp.path, "alpha", true);
    let (beta, beta_remote) = code_checkout(&temp.path, "beta", true);
    let (gamma, _) = code_checkout(&temp.path, "gamma", true);
    let alpha_remote = alpha_remote.unwrap();
    let beta_remote = beta_remote.unwrap();
    super::support::git_ok(
        Some(&gamma),
        &["remote", "set-url", "origin", &alpha_remote],
    );
    manager
        .register_code_repository(
            workspace.id,
            registration("alpha", alpha.clone(), Some(alpha_remote.clone())),
        )
        .unwrap();

    let id_conflict = manager
        .register_code_repository(
            workspace.id,
            registration("alpha", beta.clone(), Some(beta_remote)),
        )
        .unwrap_err();
    assert!(
        format!("{id_conflict:#}").contains("different remote"),
        "{id_conflict:#}"
    );
    let remote_conflict = manager
        .register_code_repository(
            workspace.id,
            registration("gamma", gamma, Some(alpha_remote)),
        )
        .unwrap_err();
    assert!(format!("{remote_conflict:#}").contains("already registered"));

    let reloaded = manager.open(workspace.id).unwrap();
    assert_eq!(reloaded.repositories.repositories.len(), 1);
    assert_eq!(reloaded.repositories.repositories[0].id, "alpha");
}

#[test]
fn disconnect_only_removes_local_checkout_mapping() {
    let temp = TestDirectory::new("disconnect");
    let manager = temp.manager("operator");
    let workspace = manager.create("Disconnect safely").unwrap();
    let (checkout, remote) = code_checkout(&temp.path, "alpha", true);
    manager
        .register_code_repository(
            workspace.id,
            registration("alpha", checkout.clone(), remote),
        )
        .unwrap();

    let disconnected = manager.disconnect_checkout(workspace.id, "alpha").unwrap();

    assert!(disconnected.association.code_checkouts.is_empty());
    assert_eq!(disconnected.repositories.repositories.len(), 1);
    assert!(checkout.is_dir());
    assert!(checkout.join(".git").is_dir());
}

#[test]
fn empty_registry_does_not_invent_a_root_repository() {
    let temp = TestDirectory::new("no-root");
    let manager = temp.manager("operator");
    let workspace = manager.create("No root mapping").unwrap();
    let reopened = manager.open(workspace.id).unwrap();

    assert!(reopened.repositories.repositories.is_empty());
    assert!(reopened.association.code_checkouts.is_empty());
    assert!(
        !WorkspaceManager::new(temp.path.join("other"))
            .association_path(workspace.id)
            .exists()
    );
}
