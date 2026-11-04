use std::path::{Component, Path};
use std::process::Command;

pub(super) fn validate_reference(reference: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        reference.starts_with("refs/heads/") || reference.starts_with("refs/remotes/"),
        "published-spec reference must be a full branch ref"
    );
    let output = Command::new("git")
        .args(["check-ref-format", reference])
        .output()?;
    anyhow::ensure!(
        output.status.success(),
        "published-spec reference is invalid"
    );
    Ok(())
}

pub(super) fn validate_document_path(path: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        !path.is_empty() && !path.contains('\\') && !path.contains('\0'),
        "published document path is invalid"
    );
    let parsed = Path::new(path);
    anyhow::ensure!(
        parsed
            .components()
            .all(|part| matches!(part, Component::Normal(_))),
        "published document path must be repository-relative"
    );
    anyhow::ensure!(
        parsed
            .extension()
            .is_some_and(|extension| extension == "md"),
        "published document must be Markdown"
    );
    Ok(())
}

pub(super) fn regular_blob_object(listing: &[u8], requested_path: &str) -> anyhow::Result<String> {
    let entries = listing
        .split(|byte| *byte == 0)
        .filter(|entry| !entry.is_empty());
    for entry in entries {
        let Some(separator) = entry.iter().position(|byte| *byte == b'\t') else {
            continue;
        };
        let (metadata, tail) = entry.split_at(separator);
        let path = &tail[1..];
        if path != requested_path.as_bytes() {
            continue;
        }
        let metadata = std::str::from_utf8(metadata)?;
        let mut fields = metadata.split_ascii_whitespace();
        let mode = fields.next().unwrap_or_default();
        let kind = fields.next().unwrap_or_default();
        let object_id = fields.next().unwrap_or_default();
        anyhow::ensure!(
            matches!(mode, "100644" | "100755") && kind == "blob",
            "published document is not a regular file"
        );
        anyhow::ensure!(
            valid_object_id(object_id),
            "Git tree has an invalid object ID"
        );
        return Ok(object_id.to_owned());
    }
    anyhow::bail!("published document does not exist at the pinned commit")
}

pub(super) fn valid_object_id(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub(super) fn git(root: &Path, arguments: &[&str]) -> anyhow::Result<String> {
    String::from_utf8(git_bytes(root, arguments)?)
        .map_err(|_| anyhow::anyhow!("Git returned non-UTF-8 output"))
}

pub(super) fn git_bytes(root: &Path, arguments: &[&str]) -> anyhow::Result<Vec<u8>> {
    let output = Command::new("git")
        .arg("--no-replace-objects")
        .arg("-C")
        .arg(root)
        .args(arguments)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_OBJECT_DIRECTORY")
        .env_remove("GIT_ALTERNATE_OBJECT_DIRECTORIES")
        .env_remove("GIT_NAMESPACE")
        .env_remove("GIT_CONFIG")
        .env_remove("GIT_CONFIG_PARAMETERS")
        .env_remove("GIT_CONFIG_COUNT")
        .output()?;
    anyhow::ensure!(
        output.status.success(),
        "Git could not read the published-spec object"
    );
    Ok(output.stdout)
}
