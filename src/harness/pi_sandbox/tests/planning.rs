use std::{
    fs,
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Command, Output},
};

use crate::{
    core::project_repos::{ProjectManifest, Repository, map_local_checkout},
    harness::pi_sandbox::PlanningSandbox,
};

use super::support::{TestTree, bwrap_available};

fn sandbox(root: &Path) -> PlanningSandbox {
    PlanningSandbox::new(root, Path::new("/bin/true")).unwrap()
}

struct PrivateState(PathBuf);

impl Drop for PrivateState {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn init_repository(path: &Path, remote: &str) {
    fs::create_dir_all(path).unwrap();
    assert!(
        Command::new("git")
            .args(["init", "-q"])
            .current_dir(path)
            .status()
            .unwrap()
            .success()
    );
    assert!(
        Command::new("git")
            .args(["remote", "add", "origin", remote])
            .current_dir(path)
            .status()
            .unwrap()
            .success()
    );
}

fn run(sandbox: &PlanningSandbox, command: &str) -> Output {
    Command::new(&sandbox.bwrap)
        .args(sandbox.command_args(&["/bin/bash".into(), "-c".into(), command.into()]))
        .env_clear()
        .env("AWS_ACCESS_KEY_ID", "host-secret-sentinel")
        .env("OPENAI_API_KEY", "provider-secret-sentinel")
        .output()
        .unwrap()
}

#[test]
fn planning_boundary_reads_only_authorized_roots_and_hides_host_secrets() {
    if !cfg!(target_os = "linux") || !bwrap_available() {
        return;
    }
    let tree = TestTree::new();
    let workspace = tree.0.join("workspace");
    let root = workspace.join("authorized");
    let secondary = workspace.join("registered");
    let unregistered = workspace.join("unregistered");
    init_repository(&root, "https://example.test/root.git");
    init_repository(&secondary, "https://example.test/secondary.git");
    fs::create_dir_all(&unregistered).unwrap();
    let fake_home = tree.0.join("host-home");
    fs::create_dir_all(fake_home.join(".ssh")).unwrap();
    fs::create_dir_all(fake_home.join(".aws")).unwrap();
    fs::write(fake_home.join("secret.txt"), "host-home-secret").unwrap();
    fs::write(fake_home.join(".ssh/id_rsa"), "ssh-secret").unwrap();
    fs::write(fake_home.join(".aws/credentials"), "cloud-secret").unwrap();
    fs::write(root.join("root.txt"), "root-visible").unwrap();
    fs::write(secondary.join("secondary.txt"), "secondary-visible").unwrap();
    fs::write(unregistered.join("secret.txt"), "unregistered-secret").unwrap();
    let config = root.join(crate::core::project_repos::PROJECT_FILE);
    fs::create_dir_all(config.parent().unwrap()).unwrap();
    let manifest = ProjectManifest {
        repositories: vec![
            Repository {
                id: "root".into(),
                role: "Planning root".into(),
                remote: "https://example.test/root.git".into(),
                display_name: None,
            },
            Repository {
                id: "secondary".into(),
                role: "Registered secondary".into(),
                remote: "https://example.test/secondary.git".into(),
                display_name: None,
            },
        ],
    };
    fs::write(&config, serde_json::to_vec_pretty(&manifest).unwrap()).unwrap();
    map_local_checkout(&root, "secondary", &secondary).unwrap();
    let _private_state = PrivateState(crate::persistence::project_dir(
        &crate::persistence::project_slug(&root.canonicalize().unwrap()),
    ));
    let outside = tree.0.join("outside-created.txt");
    let home = std::env::var("HOME").unwrap_or_else(|_| "/home".into());
    let sandbox = sandbox(&root);
    let root_text = root.display().to_string();
    let secondary_text = secondary.display().to_string();
    let unregistered_text = unregistered.display().to_string();
    let outside_text = outside.display().to_string();
    let fake_home_text = fake_home.display().to_string();
    let command = format!(
        "test \"$(cat {root_text}/root.txt)\" = root-visible && \
         grep -R -q root-visible {root_text} && \
         test \"$(cat {secondary_text}/secondary.txt)\" = secondary-visible && \
         test ! -e {unregistered_text}/secret.txt && test ! -e '{home}' && \
         test ! -e '{home}/.ssh' && test ! -e '{home}/.aws' && \
         test ! -e '{fake_home_text}/secret.txt' && \
         test ! -e '{fake_home_text}/.ssh/id_rsa' && \
         test ! -e '{fake_home_text}/.aws/credentials' && \
         test ! -e /usr/share/doc && test ! -e /usr/local/src && \
         test -z \"${{AWS_ACCESS_KEY_ID:-}}\" && \
         test -z \"${{OPENAI_API_KEY:-}}\" && \
         test ! -w '{}' && touch '{outside_text}'",
        root_text
    );
    let output = run(&sandbox, &command);
    assert!(
        output.status.success(),
        "planning boundary failed ({:?}): {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!outside.exists());
    assert!(
        Command::new(&sandbox.bwrap)
            .args(sandbox.command_args(&[
                "/bin/bash".into(),
                "-c".into(),
                format!(
                    "if touch '{0}/.git/index' 2>/dev/null; then exit 51; fi; \
                 if echo blocked >> '{0}/.git/config' 2>/dev/null; then exit 52; fi",
                    root.display()
                ),
            ]))
            .env_clear()
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap()
            .success()
    );
}

#[test]
fn planning_boundary_has_no_host_network_route() {
    if !cfg!(target_os = "linux") || !bwrap_available() {
        return;
    }
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let tree = TestTree::new();
    let root = tree.0.join("authorized");
    fs::create_dir_all(&root).unwrap();
    let sandbox = sandbox(&root);
    let command = format!(
        "if timeout 2 /bin/bash -c 'echo blocked > /dev/tcp/127.0.0.1/{port}' 2>/dev/null; then exit 42; fi"
    );
    let output = run(&sandbox, &command);
    drop(listener);
    assert!(
        output.status.success(),
        "planning boundary allowed host networking: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
