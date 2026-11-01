use std::{path::Path, process::Command};

pub(super) fn ensure_workspace_manifest(repo: &Path) -> anyhow::Result<Option<uuid::Uuid>> {
    let output = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(repo)
        .output()?;
    if !output.status.success() {
        return Ok(None);
    }

    let branch = Command::new("git")
        .args(["symbolic-ref", "--quiet", "--short", "HEAD"])
        .current_dir(repo)
        .output()?;
    let branch = if branch.status.success() {
        String::from_utf8(branch.stdout)?.trim().to_owned()
    } else {
        "main".to_owned()
    };
    let canonical_ref = format!("refs/heads/{branch}");
    let exists = Command::new("git")
        .args(["show-ref", "--verify", "--quiet", &canonical_ref])
        .current_dir(repo)
        .status()?;
    if !exists.success() {
        let tree = Command::new("git")
            .args(["hash-object", "-t", "tree", "--stdin"])
            .current_dir(repo)
            .stdin(std::process::Stdio::null())
            .output()?;
        anyhow::ensure!(tree.status.success(), "could not create test Git tree");
        let tree = String::from_utf8(tree.stdout)?.trim().to_owned();
        let commit = Command::new("git")
            .args([
                "-c",
                "user.name=Kool.ad/e Test",
                "-c",
                "user.email=koolade@test.local",
                "commit-tree",
                &tree,
                "-m",
                "Test planning baseline",
            ])
            .current_dir(repo)
            .output()?;
        anyhow::ensure!(
            commit.status.success(),
            "could not create test Git baseline"
        );
        let commit = String::from_utf8(commit.stdout)?.trim().to_owned();
        let update = Command::new("git")
            .args(["update-ref", &canonical_ref, &commit])
            .current_dir(repo)
            .status()?;
        anyhow::ensure!(update.success(), "could not seed test planning branch");
    }

    let manifest_path = repo.join(crate::artifacts::layout::store::MANIFEST);
    let manifest = if manifest_path.is_file() {
        std::fs::read(&manifest_path)
            .ok()
            .and_then(|bytes| {
                serde_json::from_slice::<crate::core::managed_workspace::WorkspaceManifest>(&bytes)
                    .ok()
            })
            .filter(|manifest| {
                manifest.kind == "koolade-planning" && !manifest.workspace_id.is_nil()
            })
    } else {
        None
    };
    let mut manifest = manifest.unwrap_or_else(|| {
        crate::core::managed_workspace::WorkspaceManifest::new(uuid::Uuid::new_v4())
    });
    manifest.canonical_ref = canonical_ref;
    std::fs::write(&manifest_path, serde_json::to_vec_pretty(&manifest)?)?;
    Ok(Some(manifest.workspace_id))
}

pub(super) fn git_remote(repo: &Path) -> Option<String> {
    let output = Command::new("git")
        .args(["config", "--get", "remote.origin.url"])
        .current_dir(repo)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}
