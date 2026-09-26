//! Full preflight and restartable byte-preserving moves for legacy artifacts.
mod apply;
mod paths;

#[cfg(test)]
pub(super) use paths::destination;
pub(super) use paths::relocated_ticket_path;
use paths::{check_destination, collect, entry, migrate_workflow, validate_relative};

#[derive(Debug)]
pub(super) struct Generated {
    pub(super) target: String,
    pub(super) bytes: Vec<u8>,
}

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

#[derive(Debug)]
pub(super) struct Entry {
    pub(super) source: String,
    pub(super) target: String,
    pub(super) source_bytes: Vec<u8>,
    pub(super) target_bytes: Vec<u8>,
}

#[derive(Debug, Default)]
pub(super) struct Plan {
    entries: Vec<Entry>,
    generated: Vec<Generated>,
}

impl Plan {
    pub(super) fn build(repo: &Path) -> anyhow::Result<Self> {
        let mut entries = Vec::new();
        for root in [
            crate::artifacts::layout::legacy::PLANNING,
            crate::artifacts::layout::legacy::CONFIG,
            crate::artifacts::layout::legacy::ADR,
        ] {
            let path = repo.join(root);
            match fs::symlink_metadata(&path) {
                Ok(meta) => {
                    anyhow::ensure!(
                        meta.is_dir() && !meta.file_type().is_symlink(),
                        "Legacy artifact root {root} must be a real directory"
                    );
                    collect(repo, &path, &mut entries)?;
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        let root_spec = crate::artifacts::layout::legacy::ROOT_SPECIFICATION;
        let path = repo.join(root_spec);
        match fs::symlink_metadata(&path) {
            Ok(meta) => {
                anyhow::ensure!(
                    meta.is_file() && !meta.file_type().is_symlink(),
                    "Legacy artifact {root_spec} must be a regular file"
                );
                entries.push(entry(repo, &path)?);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }

        for item in &mut entries {
            if item.source == crate::artifacts::layout::legacy::WORKFLOW {
                item.target_bytes = migrate_workflow(&item.source_bytes)?;
            }
        }

        let generated = super::product::bootstrap_files(repo, None)?;
        let mut destinations = BTreeMap::<String, Vec<u8>>::new();
        for item in &entries {
            validate_relative(&item.source)?;
            validate_relative(&item.target)?;
            if let Some(previous) = destinations.get(&item.target) {
                anyhow::ensure!(
                    previous == &item.target_bytes,
                    "Migration conflict: multiple legacy files map to {}, with different contents; preserve both and resolve the conflict before connecting",
                    item.target
                );
            } else {
                destinations.insert(item.target.clone(), item.target_bytes.clone());
            }
            check_destination(repo, &item.target, &item.target_bytes)?;
        }
        for item in &generated {
            validate_relative(&item.target)?;
            if let Some(previous) = destinations.get(&item.target) {
                anyhow::ensure!(
                    previous == &item.bytes,
                    "Migration conflict: generated product artifact differs from existing target {}; both were preserved",
                    item.target
                );
            } else {
                destinations.insert(item.target.clone(), item.bytes.clone());
            }
            check_destination(repo, &item.target, &item.bytes)?;
        }
        entries.sort_by(|a, b| a.source.cmp(&b.source));
        Ok(Self { entries, generated })
    }

    pub(super) fn is_empty(&self) -> bool {
        self.entries.is_empty() && self.generated.is_empty()
    }

    pub(super) fn planned_content(&self, target: &str) -> Option<&[u8]> {
        self.entries
            .iter()
            .find(|entry| entry.target == target)
            .map(|entry| entry.target_bytes.as_slice())
            .or_else(|| {
                self.generated
                    .iter()
                    .find(|generated| generated.target == target)
                    .map(|generated| generated.bytes.as_slice())
            })
    }

    pub(super) fn target_paths(&self) -> impl Iterator<Item = &str> {
        self.entries
            .iter()
            .map(|entry| entry.target.as_str())
            .chain(self.generated.iter().map(|file| file.target.as_str()))
    }

    pub(super) fn git_paths(&self) -> Vec<String> {
        self.entries
            .iter()
            .flat_map(|entry| [entry.source.clone(), entry.target.clone()])
            .chain(self.generated.iter().map(|file| file.target.clone()))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    /// Copies every target before deleting any source. A crash at either
    /// boundary is retryable: equal source/target bytes are deduplicated.
    pub(super) fn apply(&self, repo: &Path) -> anyhow::Result<()> {
        apply::apply(self, repo)
    }
}
