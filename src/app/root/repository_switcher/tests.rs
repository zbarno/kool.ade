use crate::{
    app::{KooladeApp, root::Screen, session::test_support},
    core::{
        managed_workspace::{CodeRepositoryRegistration, WorkspaceManager},
        project_repos::ProjectManifest,
    },
};
use std::{
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

fn git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn init_repo(root: &Path, remote: &str) {
    std::fs::create_dir_all(root).unwrap();
    git(root, &["init", "-q", "-b", "main"]);
    git(root, &["config", "user.name", "Switch test"]);
    git(root, &["config", "user.email", "switch@example.test"]);
    git(root, &["remote", "add", "origin", remote]);
    std::fs::write(root.join("README.md"), "fixture\n").unwrap();
    git(root, &["add", "README.md"]);
    git(root, &["commit", "-qm", "fixture"]);
}

#[test]
fn workspace_menu_opens_the_id_mapped_checkout_in_a_new_process() {
    let _environment = crate::test_support::lock_process_environment();
    let _shield = crate::core::gitops::test_support::shield("workspace-repository-switcher");
    let base = std::env::temp_dir().join(format!(
        "koolade_switcher_{}_{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let worker = base.join("worker");
    let remote = "https://example.test/worker.git";
    init_repo(&worker, remote);
    let koolade_home = base.join("koolade-home");
    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    environment.set("KOOLADE_HOME", &koolade_home);
    let manager = WorkspaceManager::from_state_root();
    let workspace = manager.create("Repository switcher test").unwrap();
    let root = workspace.planning_repo.clone();
    manager
        .register_code_repository(
            workspace.id,
            CodeRepositoryRegistration {
                id: "worker".into(),
                role: "Worker".into(),
                remote: Some(remote.into()),
                checkout: worker.clone(),
            },
        )
        .unwrap();
    let manifest_path = workspace.store.layout().project_manifest();
    let mut manifest = ProjectManifest::load(&workspace.store).unwrap();
    manifest.repositories[0].display_name = Some("API".into());
    crate::core::project_repos::save_display_names(&workspace.store, manifest).unwrap();

    let probe = base.join("capture-cwd.sh");
    let captured = base.join("child-cwd.txt");
    std::fs::write(
        &probe,
        "#!/bin/sh\nprobe_dir=${0%/*}\nprintf '%s' \"$(pwd -P)\" > \"$probe_dir/child-cwd.txt.tmp\" && mv \"$probe_dir/child-cwd.txt.tmp\" \"$probe_dir/child-cwd.txt\"\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&probe, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    let mut project = test_support::project_from(&root);
    project.managed_workspace = Some(workspace);
    let mut app = KooladeApp {
        screen: Screen::Connected(Box::new(project)),
        spawn_target_override: Some(probe),
        ..Default::default()
    };
    app.open_registered_repository("worker");

    let deadline = Instant::now() + Duration::from_secs(10);
    let child_cwd = loop {
        if let Ok(child_cwd) = std::fs::read_to_string(&captured)
            && !child_cwd.is_empty()
        {
            break child_cwd;
        }
        assert!(
            Instant::now() < deadline,
            "spawned process did not record its working directory"
        );
        std::thread::sleep(Duration::from_millis(20));
    };
    let child_cwd = PathBuf::from(child_cwd);
    assert_eq!(child_cwd, worker.canonicalize().unwrap());
    let shared = std::fs::read_to_string(manifest_path).unwrap();
    assert!(shared.contains("API"));
    assert!(!shared.contains(&worker.to_string_lossy().to_string()));
    drop(environment);
    let _ = std::fs::remove_dir_all(base);
}
