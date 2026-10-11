use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

pub struct Fixture {
    pub root: PathBuf,
    pub remote: PathBuf,
    pub first: PathBuf,
    pub second: PathBuf,
    pub workspace_id: uuid::Uuid,
    pub work_id: uuid::Uuid,
    pub workflow_id: uuid::Uuid,
}

impl Fixture {
    pub fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("koolade-shared-sync-{}", uuid::Uuid::new_v4()));
        let remote = root.join("remote.git");
        let seed = root.join("seed");
        fs::create_dir_all(&root).unwrap();
        git(
            None,
            &[
                "-c",
                "init.defaultBranch=main",
                "init",
                "--bare",
                remote.to_str().unwrap(),
            ],
        );
        git(
            None,
            &[
                "-c",
                "init.defaultBranch=main",
                "init",
                seed.to_str().unwrap(),
            ],
        );
        let workspace_id = uuid::Uuid::new_v4();
        let work_id = uuid::Uuid::new_v4();
        let workflow_id = uuid::Uuid::new_v4();
        fs::write(
            seed.join("manifest.json"),
            serde_json::to_vec_pretty(&crate::core::managed_workspace::WorkspaceManifest::new(
                workspace_id,
            ))
            .unwrap(),
        )
        .unwrap();
        fs::create_dir_all(seed.join("state/work")).unwrap();
        fs::write(
            seed.join(work_path(work_id)),
            record(work_id, "original", "stable", 1),
        )
        .unwrap();
        fs::create_dir_all(seed.join("state/workflow")).unwrap();
        fs::write(
            seed.join(format!("state/workflow/{workflow_id}.json")),
            serde_json::to_vec_pretty(&serde_json::json!({
                "schemaVersion": 1,
                "uid": workflow_id.hyphenated().to_string(),
                "revision": 1,
                "createdAtMs": 1,
                "updatedAtMs": 1,
                "featureId": "FEATURE-1",
                "data": { "approvedFeatures": { "FEATURE-1": { "approvedPlan": "original" } } }
            }))
            .unwrap(),
        )
        .unwrap();
        commit_all(&seed, "initial planning snapshot");
        git(
            Some(&seed),
            &["remote", "add", "origin", remote.to_str().unwrap()],
        );
        git(Some(&seed), &["push", "origin", "HEAD:refs/heads/main"]);
        let first = root.join("operator-a");
        let second = root.join("operator-b");
        git(
            None,
            &["clone", remote.to_str().unwrap(), first.to_str().unwrap()],
        );
        git(
            None,
            &["clone", remote.to_str().unwrap(), second.to_str().unwrap()],
        );
        Self {
            root,
            remote,
            first,
            second,
            workspace_id,
            work_id,
            workflow_id,
        }
    }

    pub fn state_root(&self, operator: &str) -> PathBuf {
        self.root.join(operator)
    }

    pub fn write_work(&self, repo: &Path, uid: uuid::Uuid, field: &str, value: &str) {
        let path = repo.join(work_path(uid));
        let mut record: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        record["data"][field] = value.into();
        record["revision"] = 2.into();
        record["updatedAtMs"] = 2.into();
        fs::write(path, serde_json::to_vec_pretty(&record).unwrap()).unwrap();
    }

    pub fn remote_record(&self, uid: uuid::Uuid) -> serde_json::Value {
        serde_json::from_slice(
            &git(
                None,
                &[
                    "--git-dir",
                    self.remote.to_str().unwrap(),
                    "show",
                    &format!("refs/heads/main:{}", work_path(uid)),
                ],
            )
            .stdout,
        )
        .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

pub fn work_path(uid: uuid::Uuid) -> String {
    format!("state/work/{uid}.json")
}

pub fn record(uid: uuid::Uuid, title: &str, detail: &str, revision: u64) -> Vec<u8> {
    serde_json::to_vec_pretty(&serde_json::json!({
        "schemaVersion": 1,
        "uid": uid.hyphenated().to_string(),
        "revision": revision,
        "createdAtMs": 1,
        "updatedAtMs": revision,
        "data": { "uid": uid.hyphenated().to_string(), "title": title, "detail": detail }
    }))
    .unwrap()
}

pub fn git(cwd: Option<&Path>, args: &[&str]) -> Output {
    let mut command = Command::new("git");
    command
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE");
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

pub fn commit_all(repo: &Path, message: &str) {
    git(Some(repo), &["add", "-A"]);
    git(
        Some(repo),
        &[
            "-c",
            "user.name=Test Operator",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "-m",
            message,
        ],
    );
}
