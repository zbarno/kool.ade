use crate::{
    app::{KooladeApp, root::Screen, session::test_support},
    core::project_repos::{ProjectManifest, Repository, map_local_checkout},
};
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

struct KooladeHome(Option<OsString>);
impl Drop for KooladeHome {
    fn drop(&mut self) {
        // SAFETY: every KOOLADE_HOME test shares the Git test shield.
        unsafe {
            if let Some(previous) = self.0.take() {
                std::env::set_var("KOOLADE_HOME", previous);
            } else {
                std::env::remove_var("KOOLADE_HOME");
            }
        }
    }
}

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
    let _shield = crate::core::gitops::test_support::shield("workspace-repository-switcher");
    let base = std::env::temp_dir().join(format!(
        "koolade_switcher_{}_{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let root = base.join("planning");
    let worker = base.join("worker");
    let remote = "https://example.test/worker.git";
    init_repo(&root, "https://example.test/planning.git");
    init_repo(&worker, remote);
    let manifest = ProjectManifest {
        repositories: vec![
            Repository {
                id: "root".into(),
                role: "Planning root".into(),
                remote: "https://example.test/planning.git".into(),
                display_name: Some("Planning".into()),
            },
            Repository {
                id: "worker".into(),
                role: "Worker".into(),
                remote: remote.into(),
                display_name: Some("API".into()),
            },
        ],
    };
    let manifest_path = crate::artifacts::layout::ArtifactLayout::new(&root).project_manifest();
    std::fs::create_dir_all(manifest_path.parent().unwrap()).unwrap();
    std::fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
    let koolade_home = base.join("koolade-home");
    let previous = std::env::var_os("KOOLADE_HOME");
    // SAFETY: `_shield` serializes KOOLADE_HOME changes across tests.
    unsafe { std::env::set_var("KOOLADE_HOME", &koolade_home) };
    let _restore = KooladeHome(previous);
    map_local_checkout(&root, "worker", &worker).unwrap();

    let probe = base.join("capture-cwd.sh");
    let captured = base.join("child-cwd.txt");
    std::fs::write(
        &probe,
        format!(
            "#!/bin/sh\nprintf '%s' \"$PWD\" > '{}'\n",
            captured.display()
        ),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&probe, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    let project = test_support::project_from(&root);
    let mut app = KooladeApp {
        screen: Screen::Connected(Box::new(project)),
        spawn_target_override: Some(probe),
        ..Default::default()
    };
    app.open_registered_repository("worker");

    let deadline = Instant::now() + Duration::from_secs(2);
    while !captured.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    let child_cwd = PathBuf::from(std::fs::read_to_string(&captured).unwrap());
    assert_eq!(child_cwd, worker.canonicalize().unwrap());
    let shared = std::fs::read_to_string(manifest_path).unwrap();
    assert!(shared.contains("API"));
    assert!(!shared.contains(&worker.to_string_lossy().to_string()));
    drop(_restore);
    let _ = std::fs::remove_dir_all(base);
}
