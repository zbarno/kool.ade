use std::path::Path;

pub(super) fn advance_branch(repository: &Path, branch: &str) -> String {
    let reference = format!("refs/heads/{branch}");
    let parent = git(
        repository,
        &["rev-parse", "--verify", &format!("{reference}^{{commit}}")],
    );
    let tree = git(repository, &["rev-parse", &format!("{parent}^{{tree}}")]);
    let output = std::process::Command::new("git")
        .args([
            "-c",
            "user.name=Koolade Evolve fixture",
            "-c",
            "user.email=evolve-fixture@koolade.invalid",
            "commit-tree",
            &tree,
            "-p",
            &parent,
            "-m",
            "advance source after Evolve approval",
        ])
        .current_dir(repository)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "fixture source branch advances: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let advanced = String::from_utf8(output.stdout).unwrap().trim().to_owned();
    git(repository, &["update-ref", &reference, &advanced, &parent]);
    advanced
}

pub(super) fn head(repository: &Path) -> String {
    git(repository, &["rev-parse", "HEAD"])
}

fn git(repository: &Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(repository)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git fixture command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}
