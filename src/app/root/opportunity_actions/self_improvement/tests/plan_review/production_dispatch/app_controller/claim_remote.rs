use std::path::Path;

pub(super) fn configure_local_claim_remote(root: &Path, repository: &Path) {
    let remote = root.join("claim-remote.git");
    let initialized = std::process::Command::new("git")
        .args(["init", "--bare", "--initial-branch=main"])
        .arg(&remote)
        .output()
        .expect("Git can initialize the fixture claim remote");
    assert!(initialized.status.success());
    let filter_configured = std::process::Command::new("git")
        .args(["config", "uploadpack.allowFilter", "true"])
        .current_dir(&remote)
        .output()
        .expect("Git can enable bounded claim inspection on the fixture remote");
    assert!(filter_configured.status.success());

    let push_url = url::Url::from_file_path(&remote)
        .expect("fixture claim remote has a file URL")
        .to_string();
    let configured = std::process::Command::new("git")
        .args(["config", "--local", "remote.origin.pushurl", &push_url])
        .current_dir(repository)
        .output()
        .expect("Git can configure the fixture claim push URL");
    assert!(configured.status.success());

    let seeded = std::process::Command::new("git")
        .args(["push", "--no-verify", "origin", "main"])
        .current_dir(repository)
        .output()
        .expect("Git can seed the fixture claim remote");
    assert!(seeded.status.success());
}
