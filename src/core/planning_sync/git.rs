use std::path::Path;

mod commit;
mod config;
mod remote_identity;
mod snapshot;
pub(super) use commit::create_commit;
pub(super) use config::output;
#[cfg(test)]
use config::{audit_local_config, is_remote_helper, safe_command};
pub(crate) use config::{standalone_command, validate_repository_config, validated_command};
pub(super) use remote_identity::{
    is_canonical_branch, remote_identity, remote_identity_for, remote_is_configured, set_origin,
};
pub(super) use snapshot::{changed_paths, read_path, worktree_paths, write_worktree_path};

pub(super) fn success(repo: &Path, args: &[&str]) -> anyhow::Result<String> {
    let result = output(repo, args)?;
    anyhow::ensure!(
        result.status.success(),
        "planning Git operation failed ({})",
        args.first().copied().unwrap_or("unknown")
    );
    Ok(String::from_utf8_lossy(&result.stdout).trim().to_owned())
}

pub(super) fn remote_heads(repo: &Path) -> anyhow::Result<Vec<String>> {
    let result = output(repo, &["ls-remote", "--heads", "origin"])?;
    if !result.status.success() {
        let detail = format!(
            "{}{}",
            String::from_utf8_lossy(&result.stderr),
            String::from_utf8_lossy(&result.stdout)
        );
        anyhow::bail!(
            "planning remote inspection failed: {}",
            classify_transport(&detail.to_ascii_lowercase())
        );
    }
    Ok(String::from_utf8_lossy(&result.stdout)
        .lines()
        .filter_map(|line| {
            line.split_once('\t')
                .map(|(_, reference)| reference.to_owned())
        })
        .collect())
}

pub(super) fn push_initial(repo: &Path, canonical_ref: &str) -> anyhow::Result<()> {
    validate_ref(repo, canonical_ref)?;
    let refspec = format!("HEAD:{canonical_ref}");
    let result = output(repo, &["push", "--porcelain", "origin", &refspec])?;
    if !result.status.success() {
        let detail = format!(
            "{}{}",
            String::from_utf8_lossy(&result.stderr),
            String::from_utf8_lossy(&result.stdout)
        );
        anyhow::bail!(
            "initial planning publish failed: {}; remote history was preserved",
            classify_transport(&detail.to_ascii_lowercase())
        );
    }
    Ok(())
}

pub(super) fn head(repo: &Path) -> anyhow::Result<String> {
    success(repo, &["rev-parse", "--verify", "HEAD^{commit}"])
}

pub(super) fn remote_tracking_commit(
    repo: &Path,
    canonical_ref: &str,
) -> anyhow::Result<Option<String>> {
    validate_ref(repo, canonical_ref)?;
    let branch = canonical_ref
        .strip_prefix("refs/heads/")
        .expect("validated branch ref");
    let tracking = format!("refs/remotes/origin/{branch}^{{commit}}");
    let result = output(
        repo,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            "--end-of-options",
            &tracking,
        ],
    )?;
    Ok(result
        .status
        .success()
        .then(|| String::from_utf8_lossy(&result.stdout).trim().to_owned()))
}

pub(super) fn fetch(repo: &Path, canonical_ref: &str) -> anyhow::Result<String> {
    validate_ref(repo, canonical_ref)?;
    let result = output(repo, &["fetch", "--no-tags", "origin", canonical_ref])?;
    if !result.status.success() {
        let detail = String::from_utf8_lossy(&result.stderr).to_ascii_lowercase();
        anyhow::bail!("fetch failed: {}", classify_transport(&detail));
    }
    success(repo, &["rev-parse", "--verify", "FETCH_HEAD^{commit}"])
}

pub(super) fn push(repo: &Path, commit: &str, canonical_ref: &str) -> anyhow::Result<()> {
    validate_ref(repo, canonical_ref)?;
    let refspec = format!("{commit}:{canonical_ref}");
    let result = output(repo, &["push", "--porcelain", "origin", &refspec])?;
    if result.status.success() {
        return Ok(());
    }
    let detail = format!(
        "{}{}",
        String::from_utf8_lossy(&result.stderr),
        String::from_utf8_lossy(&result.stdout)
    )
    .to_ascii_lowercase();
    anyhow::bail!("push failed: {}", classify_transport(&detail));
}

pub(super) fn contains_operation(
    repo: &Path,
    commit: &str,
    id: uuid::Uuid,
) -> anyhow::Result<bool> {
    let trailer = format!("Koolade-Operation: {id}");
    let result = output(repo, &["log", "--format=%H", "--grep", &trailer, commit])?;
    anyhow::ensure!(
        result.status.success(),
        "could not inspect planning history"
    );
    Ok(!result.stdout.is_empty())
}

pub(super) fn is_ancestor(repo: &Path, older: &str, newer: &str) -> anyhow::Result<bool> {
    let result = output(repo, &["merge-base", "--is-ancestor", older, newer])?;
    Ok(result.status.success())
}

pub(super) fn merge_base(repo: &Path, left: &str, right: &str) -> anyhow::Result<Option<String>> {
    let result = output(repo, &["merge-base", left, right])?;
    if result.status.success() {
        return Ok(Some(
            String::from_utf8_lossy(&result.stdout).trim().to_owned(),
        ));
    }
    if result.status.code() == Some(1) {
        return Ok(None);
    }
    anyhow::bail!("could not find a common planning history base")
}

fn validate_ref(repo: &Path, reference: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        reference.starts_with("refs/heads/"),
        "canonical ref is not a branch"
    );
    let result = output(repo, &["check-ref-format", reference])?;
    anyhow::ensure!(result.status.success(), "canonical planning ref is invalid");
    Ok(())
}

pub(super) fn validate_path(path: &str) -> anyhow::Result<()> {
    let mut components = Path::new(path).components();
    anyhow::ensure!(
        !path.is_empty()
            && components.all(|component| matches!(component, std::path::Component::Normal(_)))
            && !path.starts_with('/')
            && !path.contains('\\'),
        "planning record path is invalid"
    );
    Ok(())
}

pub(super) fn classify_transport(detail: &str) -> &'static str {
    if [
        "authentication failed",
        "could not read username",
        "http 401",
        "permission denied (publickey)",
        "no supported authentication methods available",
    ]
    .iter()
    .any(|text| detail.contains(text))
    {
        "credentials are unavailable"
    } else if [
        "permission denied",
        "access denied",
        "http 403",
        "not authorized to push",
        "protected branch",
        "not permitted",
    ]
    .iter()
    .any(|text| detail.contains(text))
    {
        "remote write permission is unavailable"
    } else if [
        "could not resolve",
        "connection refused",
        "network is unreachable",
        "timed out",
        "failed to connect",
        "remote end hung up",
        "does not appear to be a git repository",
        "no such file or directory",
    ]
    .iter()
    .any(|text| detail.contains(text))
    {
        "remote is offline"
    } else if [
        "non-fast-forward",
        "fetch first",
        "stale info",
        "failed to update ref",
    ]
    .iter()
    .any(|text| detail.contains(text))
    {
        "canonical branch advanced during push"
    } else {
        "remote rejected the operation"
    }
}

#[cfg(test)]
#[path = "git/tests.rs"]
mod tests;
