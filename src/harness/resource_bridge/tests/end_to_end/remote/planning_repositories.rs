use super::support;
use std::{fs, path::Path};

pub(super) fn create(root: &Path) -> (std::path::PathBuf, std::path::PathBuf, std::path::PathBuf) {
    let remote = root.join("planning.git");
    let seed = root.join("planning-seed");
    let first = root.join("planning-controller-a");
    let second = root.join("planning-controller-b");
    fs::create_dir_all(&seed).unwrap();
    support::run_git(
        root,
        &["init", "--quiet", "--bare", remote.to_str().unwrap()],
    );
    support::run_git(&remote, &["config", "uploadpack.allowFilter", "true"]);
    support::run_git(&seed, &["init", "--quiet"]);
    fs::write(seed.join("README.md"), "planning claim fixture\n").unwrap();
    support::run_git(
        &seed,
        &[
            "-c",
            "user.name=Synthetic Test",
            "-c",
            "user.email=test@example.invalid",
            "add",
            "README.md",
        ],
    );
    support::run_git(
        &seed,
        &[
            "-c",
            "user.name=Synthetic Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "--quiet",
            "-m",
            "planning fixture",
        ],
    );
    support::run_git(&seed, &["branch", "-M", "main"]);
    support::run_git(
        &seed,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    support::run_git(&seed, &["push", "--quiet", "origin", "main"]);
    support::run_git(&remote, &["symbolic-ref", "HEAD", "refs/heads/main"]);
    support::run_git(
        root,
        &[
            "clone",
            "--quiet",
            remote.to_str().unwrap(),
            first.to_str().unwrap(),
        ],
    );
    support::run_git(
        root,
        &[
            "clone",
            "--quiet",
            remote.to_str().unwrap(),
            second.to_str().unwrap(),
        ],
    );
    (remote, first, second)
}

pub(super) fn git_output(root: &Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
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
