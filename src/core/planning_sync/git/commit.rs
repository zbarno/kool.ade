use std::{
    fs,
    path::Path,
    process::{Output, Stdio},
};

use super::{output, validate_path, validated_command};
use crate::core::planning_sync::journal::RecordMutation;

pub(in crate::core::planning_sync) fn create_commit(
    repo: &Path,
    base: &str,
    operation_id: uuid::Uuid,
    message: &str,
    mutations: &[RecordMutation],
) -> anyhow::Result<String> {
    let index = TemporaryIndex::new()?;
    index_git(repo, &index.path, &["read-tree", base])?;
    for mutation in mutations {
        validate_path(&mutation.path)?;
        match mutation.desired.as_deref() {
            Some(bytes) => {
                let blob = hash_blob(repo, bytes.as_bytes())?;
                index_git(
                    repo,
                    &index.path,
                    &[
                        "update-index",
                        "--add",
                        "--cacheinfo",
                        "100644",
                        &blob,
                        &mutation.path,
                    ],
                )?;
            }
            None => {
                let result = index_output(
                    repo,
                    &index.path,
                    &["update-index", "--force-remove", "--", &mutation.path],
                )?;
                anyhow::ensure!(
                    result.status.success(),
                    "could not remove planning record from temporary index"
                );
            }
        }
    }
    let tree = index_success(repo, &index.path, &["write-tree"])?;
    let trailer = format!("Koolade-Operation: {operation_id}");
    let result = output(
        repo,
        &[
            "-c",
            "user.name=Kool.ad/e Planner",
            "-c",
            "user.email=koolade-migration@users.noreply.github.com",
            "commit-tree",
            &tree,
            "-p",
            base,
            "-m",
            message,
            "-m",
            &trailer,
        ],
    )?;
    anyhow::ensure!(
        result.status.success(),
        "could not create planning operation commit"
    );
    Ok(String::from_utf8_lossy(&result.stdout).trim().to_owned())
}

fn hash_blob(repo: &Path, bytes: &[u8]) -> anyhow::Result<String> {
    let mut child = validated_command(repo)?
        .args(["hash-object", "-w", "--stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    use std::io::Write;
    child.stdin.take().expect("piped stdin").write_all(bytes)?;
    let result = child.wait_with_output()?;
    anyhow::ensure!(
        result.status.success(),
        "could not write planning record blob"
    );
    Ok(String::from_utf8_lossy(&result.stdout).trim().to_owned())
}

struct TemporaryIndex {
    path: std::path::PathBuf,
}

impl TemporaryIndex {
    fn new() -> anyhow::Result<Self> {
        let path =
            std::env::temp_dir().join(format!("koolade-sync-index-{}", uuid::Uuid::new_v4()));
        Ok(Self { path })
    }
}

impl Drop for TemporaryIndex {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn index_output(repo: &Path, index: &Path, args: &[&str]) -> anyhow::Result<Output> {
    Ok(validated_command(repo)?
        .args(args)
        .env("GIT_INDEX_FILE", index)
        .output()?)
}

fn index_success(repo: &Path, index: &Path, args: &[&str]) -> anyhow::Result<String> {
    let result = index_output(repo, index, args)?;
    anyhow::ensure!(
        result.status.success(),
        "temporary planning index operation failed"
    );
    Ok(String::from_utf8_lossy(&result.stdout).trim().to_owned())
}

fn index_git(repo: &Path, index: &Path, args: &[&str]) -> anyhow::Result<()> {
    let result = index_output(repo, index, args)?;
    anyhow::ensure!(
        result.status.success(),
        "temporary planning index operation failed"
    );
    Ok(())
}
