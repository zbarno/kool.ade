use crate::artifacts::planning_store::PlanningStore;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub(super) fn copy_store(source: &PlanningStore, target: &PlanningStore) {
    for (relative, bytes) in snapshot(&source.root) {
        target.atomic_write(&relative, &bytes).unwrap();
    }
}

pub(super) fn copy_directory(source: &PlanningStore, targets: &[&PlanningStore], directory: &str) {
    for (relative, bytes) in snapshot(&source.root) {
        if let Some(name) = relative.strip_prefix(&format!("{directory}/")) {
            for target in targets {
                target
                    .atomic_write(format!("{directory}/{name}"), &bytes)
                    .unwrap();
            }
        }
    }
}

pub(super) fn snapshot(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fn visit(root: &Path, current: &Path, files: &mut BTreeMap<String, Vec<u8>>) {
        for entry in std::fs::read_dir(current).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.file_name().is_some_and(|name| name == ".git") {
                continue;
            }
            if entry.file_type().unwrap().is_dir() {
                visit(root, &path, files);
            } else if entry.file_type().unwrap().is_file() {
                files.insert(
                    path.strip_prefix(root)
                        .unwrap()
                        .to_string_lossy()
                        .into_owned(),
                    std::fs::read(path).unwrap(),
                );
            }
        }
    }
    let mut files = BTreeMap::new();
    visit(root, root, &mut files);
    files
}

pub(super) fn git(root: &Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

pub(super) fn init_git(root: &Path) {
    std::fs::create_dir_all(root).unwrap();
    git(root, &["init", "-q", "-b", "main"]);
    git(root, &["config", "user.name", "Koolade Test"]);
    git(root, &["config", "user.email", "koolade@example.test"]);
}

pub(super) struct CleanupDir(PathBuf);

impl CleanupDir {
    pub(super) fn new(path: PathBuf) -> Self {
        Self(path)
    }
}

impl Drop for CleanupDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
