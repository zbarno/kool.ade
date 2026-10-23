use super::{
    MAX_ENTRIES, MAX_STAGED_BYTES, ValidatedWorkspace, WorkspaceSnapshot, archive, artifacts,
    failure, sync,
};
use crate::execution::errors::ExecutionError;
use std::path::PathBuf;

pub(crate) fn synchronize(
    workspace: &ValidatedWorkspace,
    snapshot: &WorkspaceSnapshot,
    returned_archive: &[u8],
    budget: u64,
) -> Result<(), ExecutionError> {
    synchronize_with_source_output_policy(workspace, snapshot, returned_archive, budget, &[], None)
        .map(|_| ())
}

#[cfg(test)]
pub(crate) fn synchronize_with_artifacts(
    workspace: &ValidatedWorkspace,
    snapshot: &WorkspaceSnapshot,
    returned_archive: &[u8],
    budget: u64,
    artifact_paths: &[String],
) -> Result<Vec<crate::execution::contract::ArtifactManifestEntryV1>, ExecutionError> {
    synchronize_with_source_output_policy(
        workspace,
        snapshot,
        returned_archive,
        budget,
        artifact_paths,
        None,
    )
}

pub(crate) fn synchronize_with_source_output_policy(
    workspace: &ValidatedWorkspace,
    snapshot: &WorkspaceSnapshot,
    returned_archive: &[u8],
    budget: u64,
    artifact_paths: &[String],
    source_output_paths: Option<&[String]>,
) -> Result<Vec<crate::execution::contract::ArtifactManifestEntryV1>, ExecutionError> {
    workspace.revalidate()?;
    let limit = budget.min(MAX_STAGED_BYTES);
    let parsed = archive::parse_returned(returned_archive, limit, MAX_ENTRIES)
        .map_err(|_| failure("Container returned an invalid or oversized workspace archive"))?;
    let artifacts = collect_parsed_artifacts(&parsed, artifact_paths, limit)?;
    let mut entries = if let Some(scope) = &snapshot.scope_root
        && scope != std::path::Path::new(".")
    {
        parsed
            .into_iter()
            .map(|(path, entry)| (scope.join(path), entry))
            .collect()
    } else {
        parsed
    };
    if let Some(scope) = &snapshot.scope_root
        && scope != std::path::Path::new(".")
    {
        // A scoped archive starts at the target root. Preserve its host-side
        // ancestor directories in the returned manifest so allowlist checks
        // do not mistake them for deletions outside the target.
        for ancestor in scope
            .ancestors()
            .filter(|path| !path.as_os_str().is_empty())
        {
            entries
                .entry(ancestor.to_path_buf())
                .or_insert_with(|| archive::ReturnedEntry {
                    fingerprint: archive::Fingerprint::Directory,
                    bytes: Vec::new(),
                    mode: 0o755,
                });
        }
    }
    sync::apply_entries(
        workspace.root(),
        &snapshot.manifest,
        entries,
        limit,
        source_output_paths,
    )
    .map_err(|error| failure(&error))?;
    Ok(artifacts)
}

pub(crate) fn collect_artifacts(
    returned_archive: &[u8],
    budget: u64,
    artifact_paths: &[String],
) -> Result<Vec<crate::execution::contract::ArtifactManifestEntryV1>, ExecutionError> {
    let limit = budget.min(MAX_STAGED_BYTES);
    let parsed = archive::parse_returned(returned_archive, limit, MAX_ENTRIES)
        .map_err(|_| failure("Container returned an invalid or oversized workspace archive"))?;
    collect_parsed_artifacts(&parsed, artifact_paths, limit)
}

fn collect_parsed_artifacts(
    parsed: &std::collections::BTreeMap<PathBuf, archive::ReturnedEntry>,
    artifact_paths: &[String],
    limit: u64,
) -> Result<Vec<crate::execution::contract::ArtifactManifestEntryV1>, ExecutionError> {
    artifacts::collect(parsed, artifact_paths, limit)
        .map_err(|_| failure("Container did not return the requested bounded artifacts"))
}
