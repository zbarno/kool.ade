use super::*;
use std::collections::BTreeSet;
use std::{fs, path::Path};

pub(super) fn read_receipt(path: &Path) -> anyhow::Result<Option<PreparedProposalV1>> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    anyhow::ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink(),
        "PR Preparer receipt is not a regular file"
    );
    Ok(Some(serde_json::from_slice(&fs::read(path)?)?))
}

pub(super) fn read_reusable_receipt(
    path: &Path,
    scope: &ProposalScope,
) -> anyhow::Result<Option<PreparedProposalV1>> {
    let Some(receipt) = read_receipt(path)? else {
        return Ok(None);
    };
    if validate_receipt(&receipt, scope)? {
        return Ok(Some(receipt));
    }
    fs::remove_file(path)?;
    Ok(None)
}

pub(super) fn validate_receipt(
    receipt: &PreparedProposalV1,
    scope: &ProposalScope,
) -> anyhow::Result<bool> {
    if receipt.schema_version != 1 || receipt.scope_sha256 != scope.scope_sha256 {
        return Ok(false);
    }
    let Ok(role) = crate::harness::prompt_catalog::validate_prompt_manifest(&receipt.prompt) else {
        return Ok(false);
    };
    if role != PromptRole::PrPreparer {
        return Ok(false);
    }
    validate_proposal(&receipt.proposal, &scope.expected).map(|()| true)
}

pub(super) fn validate_proposal(
    proposal: &crate::harness::prompt_catalog::PrProposalV1,
    expected: &ExpectedProposal,
) -> anyhow::Result<()> {
    let actual_evidence = proposal.evidence.iter().collect::<BTreeSet<_>>();
    let expected_evidence = expected.evidence_ids.iter().collect::<BTreeSet<_>>();
    anyhow::ensure!(
        proposal.task_uid == expected.task_uid
            && proposal.base_ref == expected.base_ref
            && proposal.head_ref == expected.head_ref
            && proposal.commits == expected.commits
            && proposal.files == expected.files
            && proposal.evidence.len() == expected_evidence.len()
            && actual_evidence == expected_evidence,
        "PR Preparer proposal does not match the exact application-validated scope"
    );
    anyhow::ensure!(
        !proposal.description.trim().is_empty()
            && proposal.description.len() <= 8 * 1024
            && !proposal.description.contains('\0'),
        "PR Preparer description is empty or exceeds the supported bounds"
    );
    Ok(())
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
