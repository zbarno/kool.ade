use super::{PreparedProposalV1, ProposalScope, privacy, validation};
use std::path::Path;

/// Materialize the validated proposal at the exact path consumed by the
/// application-owned provider call. Rebuild it on receipt reuse as verification
/// may have replaced the older body, or a restart may follow the receipt write.
pub(super) fn write(
    dir: &Path,
    receipt: &PreparedProposalV1,
    scope: &ProposalScope,
) -> anyhow::Result<()> {
    let body = render(receipt, scope)?;
    crate::artifacts::atomic_write(&dir.join("pr-body.md"), &body)
}

pub(super) fn render(
    receipt: &PreparedProposalV1,
    scope: &ProposalScope,
) -> anyhow::Result<String> {
    anyhow::ensure!(
        validation::validate_receipt(receipt, scope)?,
        "PR description is stale for the current publication scope"
    );
    let mut body = crate::error::redact_secrets(&receipt.proposal.description);
    body.push_str(&format!(
        "\n\n## Reviewed publication scope\n\nParent task: `{}`\n\nSource: `{}`\n\nDestination: `{}`\n\nScope SHA-256: `{}`\n\n### Commits\n\n",
        scope.expected.task_uid,
        scope.expected.head_ref,
        scope.expected.base_ref,
        scope.scope_sha256,
    ));
    for commit in &scope.expected.commits {
        body.push_str(&format!("- `{commit}`\n"));
    }
    body.push_str("\n### Changed files\n\n");
    for path in &scope.expected.files {
        body.push_str(&format!("- `{}`\n", crate::error::redact_secrets(path)));
    }
    privacy::validate_public_description(&body, scope)?;
    // Do not upload the raw evidence bundle: it contains complete diffs,
    // internal artifact references and adjacent feature context. The validated
    // proposal describes only this task; immutable identifiers bind its scope.
    Ok(body)
}

pub(super) fn discard(dir: &Path) -> anyhow::Result<()> {
    match std::fs::remove_file(dir.join("pr-body.md")) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}
