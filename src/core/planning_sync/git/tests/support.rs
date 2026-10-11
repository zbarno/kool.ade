use std::{
    path::{Path, PathBuf},
    process::Command,
};

pub(super) fn init_repo(path: &Path) {
    let output = Command::new("git")
        .args(["init", "--quiet", "--", path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success());
}

pub(super) fn test_dir() -> PathBuf {
    let path = std::env::temp_dir().join(format!("koolade-git-audit-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&path).unwrap();
    path
}
