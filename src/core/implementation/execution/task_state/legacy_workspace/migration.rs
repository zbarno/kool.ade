use super::*;
use crate::core::implementation::repository_cache::{GitCommitIdentity, RepositoryCache};
use serde::{Deserialize, Serialize};
use std::fs;

mod merge_state;
mod validation;
use merge_state::{ensure_merge_in_progress, read_merge_head};
use validation::validate_legacy_workspace;

const MIGRATION_FILE: &str = "legacy-migration.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(super) struct MigrationRecord {
    schema_version: u8,
    source: PathBuf,
    destination: PathBuf,
    branch: String,
    head: String,
    #[serde(default)]
    merge_head: Option<String>,
    #[serde(default)]
    staged_index_commit: Option<String>,
    snapshot_complete: bool,
    workspace_complete: bool,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn migrate_one(
    repo: &Path,
    dir: &Path,
    state: &mut Implementation,
    repository_id: &str,
    project_id: &str,
    cache: &RepositoryCache,
    identity: &GitCommitIdentity,
    destination: PathBuf,
    expected_merge_head: Option<&str>,
    runner: &Runner,
) -> anyhow::Result<()> {
    let migration_dir = dir.join("legacy-migration");
    ensure_real_directory(&migration_dir)?;
    let mut record = load_or_create_record(
        &migration_dir,
        repo,
        &state.task_repository,
        &destination,
        state,
        expected_merge_head,
        runner,
    )?;
    if !record.snapshot_complete {
        record.staged_index_commit = super::snapshot::capture(
            &record.source,
            &migration_dir,
            &record.head,
            repo,
            record.merge_head.is_some(),
            runner,
        )?;
        record.snapshot_complete = true;
        save_record(&migration_dir, &record)?;
    } else if record.source.exists() {
        super::snapshot::ensure_source_matches(
            &record.source,
            &migration_dir,
            &record.head,
            record.merge_head.is_some(),
            record.staged_index_commit.as_deref(),
            runner,
        )?;
        anyhow::ensure!(
            read_merge_head(&record.source, runner)? == record.merge_head,
            "Original legacy merge state changed during migration; both copies are preserved"
        );
    }

    if let Some(commit) = record.staged_index_commit.as_deref() {
        let reference = super::snapshot::index_reference(commit);
        cache.import_ref(repo, &reference, &reference, commit, runner)?;
    }

    cache.import_local_branch(repo, &record.branch, &record.head, runner)?;
    cache.pin_source(&record.head, runner)?;
    ensure_clone(cache, &record, state, identity, runner)?;
    if let Some(merge_head) = record.merge_head.as_deref() {
        ensure_merge_in_progress(&record.destination, &cache.path, merge_head, runner)?;
    }
    if record.workspace_complete {
        anyhow::ensure!(
            super::snapshot::matches_snapshot(
                &record.destination,
                &migration_dir,
                record.merge_head.is_some(),
                runner,
            )?,
            "Migrated clone changed after migration completed; both saved paths are preserved"
        );
    } else {
        super::snapshot::restore(
            &record.destination,
            &migration_dir,
            record.merge_head.is_some(),
            record.staged_index_commit.as_deref(),
            &cache.path,
            runner,
        )?;
        anyhow::ensure!(
            super::snapshot::matches_snapshot(
                &record.destination,
                &migration_dir,
                record.merge_head.is_some(),
                runner,
            )?,
            "Migrated clone does not match the captured staged, unstaged, and untracked changes"
        );
        record.workspace_complete = true;
        save_record(&migration_dir, &record)?;
    }

    state.task_repository = destination.clone();
    state.task_repository_kind = TaskRepositoryKind::Clone;
    state.task_repository_ready = true;
    state.repository_id = Some(repository_id.to_owned());
    state.project_id = Some(project_id.to_owned());
    state.repository_identity = Some(cache.identity.clone());
    state.push_repository = cache.push_identity_url.clone();
    state.repository_cache = Some(cache.path.clone());
    state.task_repository_allocation_key = Some(task_repository::allocation_key(state));
    state.task_repositories = vec![destination];
    task_repository::validate_clone_path(state)?;
    Ok(())
}

fn ensure_clone(
    cache: &RepositoryCache,
    record: &MigrationRecord,
    state: &Implementation,
    identity: &GitCommitIdentity,
    runner: &Runner,
) -> anyhow::Result<()> {
    match fs::symlink_metadata(&record.destination) {
        Ok(metadata) => {
            anyhow::ensure!(
                metadata.is_dir() && !metadata.file_type().is_symlink(),
                "Partially migrated clone path is not a real directory; both saved paths are preserved"
            );
            RepositoryCache::verify_task_repository(&record.destination, runner)?;
            anyhow::ensure!(
                runner.git(&record.destination, &["symbolic-ref", "--short", "HEAD"])?
                    == record.branch
                    && runner.git(&record.destination, &["rev-parse", "HEAD"])? == record.head,
                "Partially migrated clone has unexpected history; both saved paths are preserved"
            );
            crate::core::implementation::repository_cache::configure_clone(
                &record.destination,
                identity,
                true,
                runner,
            )?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let source_ref = cache.pin_source(&record.head, runner)?;
            ensure_parent_tree(&record.destination)?;
            cache.create_clone(
                &source_ref,
                &record.head,
                &record.branch,
                &record.destination,
                identity,
                runner,
            )?;
        }
        Err(error) => return Err(error.into()),
    }
    let base = &state.base_commit;
    anyhow::ensure!(
        runner
            .merge_base(&record.destination, base, &record.head)?
            .as_deref()
            == Some(base),
        "Legacy task base is not an ancestor of its saved workspace; both paths are preserved for review"
    );
    if let Some(verified) = state.verified_head.as_deref() {
        anyhow::ensure!(
            runner
                .merge_base(&record.destination, verified, &record.head)?
                .as_deref()
                == Some(verified),
            "Saved verified commit is not in the legacy workspace history; both paths are preserved for review"
        );
    }
    Ok(())
}

fn load_or_create_record(
    migration_dir: &Path,
    repo: &Path,
    source: &Path,
    destination: &Path,
    state: &Implementation,
    expected_merge_head: Option<&str>,
    runner: &Runner,
) -> anyhow::Result<MigrationRecord> {
    let path = migration_dir.join(MIGRATION_FILE);
    if path.exists() {
        let meta = fs::symlink_metadata(&path)?;
        anyhow::ensure!(meta.is_file() && !meta.file_type().is_symlink());
        let record: MigrationRecord = serde_json::from_slice(&fs::read(path)?)?;
        let merge_head = validate_legacy_workspace(source, repo, state, runner)?;
        anyhow::ensure!(
            record.schema_version == 1
                && record.source == source
                && record.destination == destination
                && record.branch == state.branch
                && record.merge_head == merge_head
                && merge_head
                    .as_deref()
                    .is_none_or(|head| expected_merge_head == Some(head)),
            "Legacy migration identity changed; both saved paths are preserved"
        );
        return Ok(record);
    }
    let merge_head = validate_legacy_workspace(source, repo, state, runner)?;
    anyhow::ensure!(
        merge_head
            .as_deref()
            .is_none_or(|head| expected_merge_head == Some(head)),
        "Legacy workspace has an unpinned merge in progress. Resume only after it is reconciled or reviewed; the original is preserved"
    );
    let head = runner.git(source, &["rev-parse", "HEAD"])?;
    let record = MigrationRecord {
        schema_version: 1,
        source: source.to_path_buf(),
        destination: destination.to_path_buf(),
        branch: state.branch.clone(),
        head,
        merge_head,
        snapshot_complete: false,
        staged_index_commit: None,
        workspace_complete: false,
    };
    save_record(migration_dir, &record)?;
    Ok(record)
}

fn ensure_real_directory(path: &Path) -> anyhow::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => anyhow::ensure!(
            metadata.is_dir() && !metadata.file_type().is_symlink(),
            "Migration state path is not a real directory"
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => fs::create_dir_all(path)?,
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

fn ensure_parent_tree(destination: &Path) -> anyhow::Result<()> {
    let mut missing = Vec::new();
    let mut current = destination
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Clone destination has no parent"))?;
    loop {
        match fs::symlink_metadata(current) {
            Ok(metadata) => {
                anyhow::ensure!(
                    metadata.is_dir() && !metadata.file_type().is_symlink(),
                    "Clone destination parent is not a real directory"
                );
                break;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                missing.push(current.to_path_buf());
                current = current
                    .parent()
                    .ok_or_else(|| anyhow::anyhow!("Clone destination has no existing root"))?;
            }
            Err(error) => return Err(error.into()),
        }
    }
    for directory in missing.iter().rev() {
        fs::create_dir(directory)?;
    }
    Ok(())
}

fn save_record(dir: &Path, record: &MigrationRecord) -> anyhow::Result<()> {
    crate::artifacts::atomic_write_bytes(
        &dir.join(MIGRATION_FILE),
        &serde_json::to_vec_pretty(record)?,
    )
}
