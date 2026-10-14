use super::*;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, Barrier},
};

#[path = "tests/claim_compatibility.rs"]
mod claim_compatibility;
#[path = "tests/claim_state.rs"]
mod claim_state;
mod concurrent_verification;
#[path = "tests/core_claims.rs"]
mod core_claims;
#[path = "tests/legacy_cutover.rs"]
mod legacy_cutover;
#[path = "tests/remote_configuration.rs"]
mod remote_configuration;
#[path = "tests/remote_publication.rs"]
mod remote_publication;

struct Fixture {
    root: PathBuf,
    clones: [PathBuf; 2],
    code_clones: [PathBuf; 2],
    remote: PathBuf,
    planning_remote: PathBuf,
    project_uid: uuid::Uuid,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("koolade-claims-{}", uuid::Uuid::new_v4()));
        let remote = root.join("code.git");
        let planning_remote = root.join("planning.git");
        let seed = root.join("seed");
        let clones = [root.join("planning-alice"), root.join("planning-bob")];
        let code_clones = [root.join("code-alice"), root.join("code-bob")];
        fs::create_dir_all(&root).unwrap();
        git_at(&root, &["init", "-q", "--bare", remote.to_str().unwrap()]);
        git_at(
            &root,
            &["init", "-q", "--bare", planning_remote.to_str().unwrap()],
        );
        for bare in [&remote, &planning_remote] {
            git_at(bare, &["config", "uploadpack.allowFilter", "true"]);
        }
        fs::create_dir(&seed).unwrap();
        git_at(&seed, &["init", "-q"]);
        git_at(&seed, &["config", "user.name", "Fixture User"]);
        git_at(&seed, &["config", "user.email", "fixture@noreply.example"]);
        fs::write(seed.join("README.md"), "fixture\n").unwrap();
        git_at(&seed, &["add", "README.md"]);
        git_at(&seed, &["commit", "-qm", "fixture"]);
        git_at(&seed, &["branch", "-M", "main"]);
        git_at(
            &seed,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        );
        git_at(
            &seed,
            &[
                "remote",
                "add",
                "planning",
                planning_remote.to_str().unwrap(),
            ],
        );
        git_at(&seed, &["push", "-q", "origin", "main"]);
        git_at(&seed, &["push", "-q", "planning", "main"]);
        for bare in [&remote, &planning_remote] {
            git_at(bare, &["symbolic-ref", "HEAD", "refs/heads/main"]);
        }
        for (index, clone) in clones.iter().enumerate() {
            git_at(
                &root,
                &[
                    "clone",
                    "-q",
                    planning_remote.to_str().unwrap(),
                    clone.to_str().unwrap(),
                ],
            );
            configure_clone(clone, index);
        }
        for (index, clone) in code_clones.iter().enumerate() {
            git_at(
                &root,
                &[
                    "clone",
                    "-q",
                    remote.to_str().unwrap(),
                    clone.to_str().unwrap(),
                ],
            );
            configure_clone(clone, index);
        }
        Self {
            root,
            clones,
            code_clones,
            remote,
            planning_remote,
            project_uid: uuid::Uuid::new_v4(),
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn configure_clone(clone: &Path, index: usize) {
    git_at(
        clone,
        &[
            "config",
            "user.name",
            if index == 0 { "Alice" } else { "Bob" },
        ],
    );
    git_at(clone, &["config", "user.email", "fixture@noreply.example"]);
}

fn git_at(cwd: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn claim(
    fixture: &Fixture,
    authority: &Path,
    task_uid: &str,
) -> Result<Option<ClaimLease>, ClaimError> {
    let base = git_at(&fixture.code_clones[0], &["rev-parse", "HEAD"]);
    ClaimLease::acquire(authority, fixture.project_uid, task_uid, "web", &base)
}

fn record(
    project_uid: uuid::Uuid,
    task_uid: &str,
    session_id: &str,
    base_commit: &str,
    expires_at_utc: i64,
) -> ClaimRecord {
    ClaimRecord {
        schema_version: 3,
        project_uid: project_uid.hyphenated().to_string(),
        task_uid: task_uid.to_owned(),
        owner: "Kool.ad/e session".into(),
        operator_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        session_id: session_id.to_owned(),
        code_repository_id: "web".into(),
        base_commit: base_commit.to_owned(),
        issued_at_utc: 1,
        expires_at_utc,
        takeover_history: Box::default(),
    }
}
