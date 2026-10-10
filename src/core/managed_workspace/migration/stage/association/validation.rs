use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Component, Path},
};

use sha2::Digest as _;

use super::super::super::{MigrationReport, journal::Journal};

pub fn validate_migrated_final(path: &Path, journal: &Journal) -> anyhow::Result<()> {
    anyhow::ensure!(
        crate::core::managed_workspace::git::repository_root(path)? == path,
        "migration destination is not a repository root"
    );
    let manifest = crate::core::managed_workspace::manifest::WorkspaceManifest::read(path)?;
    anyhow::ensure!(
        manifest.workspace_id == journal.workspace_id,
        "existing planning repository has a different workspace UUID"
    );
    let expected_commit = journal
        .initial_commit
        .as_deref()
        .filter(|commit| !commit.is_empty())
        .ok_or_else(|| anyhow::anyhow!("migration journal has no validated initial commit"))?;
    anyhow::ensure!(
        super::super::verification::git_text(path, &["rev-parse", "HEAD"])? == expected_commit,
        "existing planning repository HEAD differs from the validated migration commit"
    );
    anyhow::ensure!(
        crate::core::managed_workspace::git::status_paths(path)?.is_empty(),
        "existing planning repository has uncommitted changes"
    );

    let report: MigrationReport =
        serde_json::from_slice(&fs::read(path.join("migration-report.json"))?)?;
    anyhow::ensure!(
        report.workspace_id == journal.workspace_id
            && report.source_commit == journal.source_commit
            && report.source_repositories == journal.source_repositories
            && report.source_artifacts == journal.source_artifacts
            && report.source_branches == journal.source_branches,
        "existing planning repository does not match this migration journal"
    );
    validate_artifacts(path, &report)
}

fn validate_artifacts(path: &Path, report: &MigrationReport) -> anyhow::Result<()> {
    let mut expected = BTreeMap::<String, String>::new();
    for artifact in &report.generated_artifacts {
        insert_expected(&mut expected, &artifact.target_path, &artifact.sha256)?;
    }
    for artifact in &report.source_artifacts {
        if artifact.disposition == "copied-to-planning-repository" {
            let target = artifact
                .target_path
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("copied source artifact has no target path"))?;
            let hash = artifact
                .sha256
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("copied source artifact has no hash"))?;
            insert_expected(&mut expected, target, hash)?;
        }
    }
    anyhow::ensure!(
        expected
            .insert("migration-report.json".into(), String::new())
            .is_none(),
        "migration artifact manifest contains the report itself"
    );

    let mut actual = BTreeSet::new();
    collect_files(path, path, &mut actual)?;
    anyhow::ensure!(
        actual == expected.keys().cloned().collect(),
        "existing planning repository files differ from the migration artifact manifest"
    );
    for (relative, expected_hash) in expected {
        if relative == "migration-report.json" {
            continue;
        }
        let bytes = fs::read(path.join(&relative))?;
        let actual_hash = format!("{:x}", sha2::Sha256::digest(bytes));
        anyhow::ensure!(
            actual_hash == expected_hash,
            "migration artifact hash mismatch at {relative}"
        );
    }
    Ok(())
}

fn insert_expected(
    expected: &mut BTreeMap<String, String>,
    relative: &str,
    hash: &str,
) -> anyhow::Result<()> {
    let path = Path::new(relative);
    anyhow::ensure!(
        !relative.is_empty()
            && path
                .components()
                .all(|component| matches!(component, Component::Normal(_))),
        "migration report contains an unsafe artifact path"
    );
    if let Some(previous) = expected.insert(relative.to_owned(), hash.to_owned()) {
        anyhow::ensure!(
            previous == hash,
            "migration report contains conflicting hashes for {relative}"
        );
    }
    Ok(())
}

fn collect_files(
    root: &Path,
    directory: &Path,
    files: &mut BTreeSet<String>,
) -> anyhow::Result<()> {
    let mut entries = fs::read_dir(directory)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let entry_path = entry.path();
        let metadata = fs::symlink_metadata(&entry_path)?;
        anyhow::ensure!(
            !metadata.file_type().is_symlink(),
            "migration destination contains a symlink"
        );
        if entry.file_name() == ".git" {
            continue;
        }
        if metadata.is_dir() {
            collect_files(root, &entry_path, files)?;
        } else {
            anyhow::ensure!(
                metadata.is_file(),
                "unsupported migration destination entry"
            );
            let relative = entry_path
                .strip_prefix(root)?
                .to_str()
                .ok_or_else(|| anyhow::anyhow!("migration artifact path is not UTF-8"))?
                .replace('\\', "/");
            anyhow::ensure!(
                files.insert(relative),
                "duplicate migration destination artifact"
            );
        }
    }
    Ok(())
}
