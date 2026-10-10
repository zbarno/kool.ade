#[path = "support/fixtures.rs"]
mod fixtures;
#[path = "support/multi_feature.rs"]
mod multi_feature;
#[path = "support/setup.rs"]
mod setup;

use fixtures::walk;

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use crate::{
    core::managed_workspace::WorkspaceManager,
    domain::{ArtifactIdentity, OpenItem},
};

pub(super) struct Fixture {
    pub root: PathBuf,
    pub code: PathBuf,
    pub manager: WorkspaceManager,
    pub feature: ArtifactIdentity,
    pub feature_two: ArtifactIdentity,
    pub task: ArtifactIdentity,
    pub work_uid: String,
    pub open_item: OpenItem,
    pub resolved_item: OpenItem,
    pub task_two: ArtifactIdentity,
    pub legacy_workspace_id: Option<uuid::Uuid>,
}

impl Fixture {
    pub fn new(label: &str) -> Self {
        setup::build(label, true)
    }

    pub fn new_without_legacy_workspace_id(label: &str) -> Self {
        setup::build(label, false)
    }

    pub fn source_snapshot(&self) -> Vec<(String, Vec<u8>)> {
        let root = self.code.join(".koolade-packet");
        let mut files = Vec::new();
        walk(&root, &root, &mut files);
        files.sort_by(|a, b| a.0.cmp(&b.0));
        files
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

pub(super) fn git(root: &Path, args: &[&str]) -> Vec<u8> {
    let output = Command::new("git")
        .args(["-C", root.to_str().unwrap()])
        .args(args)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}
