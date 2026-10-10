use super::*;

fn git_ok(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn seed_repo(tag: &str) -> PathBuf {
    let repo = mkrepo(tag);
    fs::create_dir_all(repo.join("src")).unwrap();
    fs::create_dir_all(repo.join("planning")).unwrap();
    for (path, text) in [
        ("src/foo.rs", "fn value() { 1 }\n"),
        ("one.txt", "one baseline\n"),
        ("two.txt", "two baseline\n"),
        ("partial.txt", "first\nsecond\nthird\nfourth\n"),
        ("unstaged.txt", "unstaged baseline\n"),
        ("planning/open-items.md", "old open items\n"),
    ] {
        fs::write(repo.join(path), text).unwrap();
    }
    git_ok(&repo, &["add", "-A"]);
    git_ok(&repo, &["commit", "-qm", "seed Kool.ad/e test files"]);
    repo
}

fn index_entry(repo: &Path, path: &str) -> String {
    git_ok(repo, &["ls-files", "--stage", "--", path])
}

mod behavior;
mod concurrency;
