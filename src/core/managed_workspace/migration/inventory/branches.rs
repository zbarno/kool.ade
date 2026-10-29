use std::path::Path;

use sha2::{Digest, Sha256};

use super::{
    Artifact, BranchArtifact, BranchConflict, Kind, collect::mapping, records::stable_record_ids,
};

pub(crate) fn collect_branch_artifacts(
    code_root: &Path,
    repository_id: &str,
    active_ref: &str,
) -> anyhow::Result<Vec<BranchArtifact>> {
    let refs = super::super::super::git::run(
        Some(code_root),
        &[
            "for-each-ref",
            "--format=%(refname)",
            "refs/heads",
            "refs/remotes",
        ],
    )?;
    let refs = String::from_utf8(refs.stdout)?;
    let mut artifacts = Vec::new();
    for source_ref in refs
        .lines()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let short = source_ref
            .strip_prefix("refs/heads/")
            .or_else(|| source_ref.strip_prefix("refs/remotes/"))
            .unwrap_or(source_ref);
        if short == active_ref || short.ends_with("/HEAD") {
            continue;
        }
        let commit = String::from_utf8(
            super::super::super::git::run(Some(code_root), &["rev-parse", source_ref])?.stdout,
        )?
        .trim()
        .to_owned();
        let tree = super::super::super::git::run(
            Some(code_root),
            &[
                "ls-tree",
                "-r",
                "-z",
                "--full-tree",
                source_ref,
                "--",
                crate::artifacts::layout::store::ROOT,
            ],
        )?;
        for entry in tree
            .stdout
            .split(|byte| *byte == 0)
            .filter(|entry| !entry.is_empty())
        {
            let tab = entry
                .iter()
                .position(|byte| *byte == b'\t')
                .ok_or_else(|| anyhow::anyhow!("malformed Git tree entry in {source_ref}"))?;
            let (header, path_with_tab) = entry.split_at(tab);
            let path = &path_with_tab[1..];
            let header = std::str::from_utf8(header)?;
            let mut fields = header.split_ascii_whitespace();
            let mode = fields.next().unwrap_or_default();
            let object_type = fields.next().unwrap_or_default();
            let object_id = fields.next().unwrap_or_default();
            anyhow::ensure!(
                mode != "120000",
                "branch {source_ref} contains a symbolic link under {}",
                crate::artifacts::layout::store::ROOT
            );
            if object_type != "blob" || path.is_empty() {
                continue;
            }
            let path = std::str::from_utf8(path)?;
            let prefix = format!("{}/", crate::artifacts::layout::store::ROOT);
            let relative = path
                .strip_prefix(&prefix)
                .ok_or_else(|| anyhow::anyhow!("unexpected planning path in Git tree"))?;
            let bytes =
                super::super::super::git::run(Some(code_root), &["cat-file", "blob", object_id])?
                    .stdout;
            let (kind, _, _) = mapping(relative);
            if relative == "manifest.json" {
                if let Some(workspace_id) =
                    super::records::legacy_workspace_id_from_manifest(&bytes)?
                {
                    artifacts.push(BranchArtifact {
                        repository_id: repository_id.to_owned(),
                        source_ref: source_ref.to_owned(),
                        source_commit: commit.clone(),
                        source_path: format!(".koolade-packet/{relative}"),
                        sha256: format!("{:x}", Sha256::digest(&bytes)),
                        disposition: format!("workspace-id:{workspace_id}"),
                        record_ids: vec![workspace_id.hyphenated().to_string()],
                        target_path: Some("manifest.json".into()),
                    });
                }
                continue;
            }
            if kind == Kind::Private {
                continue;
            }
            artifacts.push(BranchArtifact {
                repository_id: repository_id.to_owned(),
                source_ref: source_ref.to_owned(),
                source_commit: commit.clone(),
                source_path: format!(".koolade-packet/{relative}"),
                sha256: format!("{:x}", Sha256::digest(&bytes)),
                disposition: mapping(relative).1.to_owned(),
                record_ids: stable_record_ids(relative, &bytes),
                target_path: mapping(relative).2,
            });
        }
    }
    artifacts.sort_by(|left, right| {
        left.source_ref
            .cmp(&right.source_ref)
            .then_with(|| left.source_path.cmp(&right.source_path))
    });
    Ok(artifacts)
}

pub(crate) fn branch_conflicts(
    worktree: &[Artifact],
    branches: &[BranchArtifact],
    current_ref: &str,
    current_commit: &str,
) -> Vec<BranchConflict> {
    let active = worktree
        .iter()
        .map(|artifact| (&artifact.mapping.source_path, artifact))
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut conflicts = Vec::new();
    for branch in branches {
        let Some(active_artifact) = active.get(&branch.source_path) else {
            let ids = if branch.record_ids.is_empty() {
                vec![None]
            } else {
                branch.record_ids.iter().cloned().map(Some).collect()
            };
            for record_id in ids {
                conflicts.push(BranchConflict {
                    artifact_path: branch.source_path.clone(),
                    record_id,
                    kind: "branch-only-planning-artifact".into(),
                    sources: vec![branch.clone()],
                });
            }
            continue;
        };
        if active_artifact.relative == "manifest.json" {
            let active_id =
                super::records::legacy_workspace_id_from_manifest(&active_artifact.bytes)
                    .ok()
                    .flatten()
                    .map(|id| id.hyphenated().to_string());
            let branch_id = branch.disposition.strip_prefix("workspace-id:");
            if active_id.as_deref() != branch_id {
                conflicts.push(BranchConflict {
                    artifact_path: branch.source_path.clone(),
                    record_id: None,
                    kind: "workspace-identity-divergence".into(),
                    sources: vec![
                        BranchArtifact {
                            repository_id: branch.repository_id.clone(),
                            source_ref: current_ref.to_owned(),
                            source_commit: current_commit.to_owned(),
                            source_path: branch.source_path.clone(),
                            sha256: active_artifact.sha256.clone(),
                            disposition: active_id
                                .as_ref()
                                .map(|id| format!("workspace-id:{id}"))
                                .unwrap_or_else(|| "workspace-id:missing".into()),
                            record_ids: active_id.into_iter().collect(),
                            target_path: Some("manifest.json".into()),
                        },
                        branch.clone(),
                    ],
                });
            }
            continue;
        }
        if active_artifact.sha256 != branch.sha256 {
            let mut ids = stable_record_ids(&active_artifact.relative, &active_artifact.bytes);
            ids.extend(branch.record_ids.iter().cloned());
            ids.sort();
            ids.dedup();
            let ids = if ids.is_empty() {
                vec![None]
            } else {
                ids.into_iter().map(Some).collect()
            };
            for record_id in ids {
                conflicts.push(BranchConflict {
                    artifact_path: branch.source_path.clone(),
                    record_id,
                    kind: "same-path-divergence".into(),
                    sources: vec![
                        BranchArtifact {
                            repository_id: branch.repository_id.clone(),
                            source_ref: current_ref.to_owned(),
                            source_commit: current_commit.to_owned(),
                            source_path: branch.source_path.clone(),
                            sha256: active_artifact.sha256.clone(),
                            disposition: "present-in-active-worktree".into(),
                            record_ids: stable_record_ids(
                                &active_artifact.relative,
                                &active_artifact.bytes,
                            ),
                            target_path: None,
                        },
                        branch.clone(),
                    ],
                });
            }
        }
    }
    conflicts.sort_by(|left, right| {
        left.artifact_path
            .cmp(&right.artifact_path)
            .then_with(|| left.kind.cmp(&right.kind))
    });
    conflicts.dedup_by(|left, right| {
        left.artifact_path == right.artifact_path
            && left.record_id == right.record_id
            && left.sources == right.sources
    });
    conflicts
}
