use super::*;

pub(super) fn archive(
    path: &Path,
    state: &Implementation,
    runner: &Runner,
    plan: &Plan,
) -> anyhow::Result<()> {
    let bytes = fs::read(path)?;
    let saved: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| {
        review_required(
            state,
            plan,
            path,
            "The prior recovery snapshot is invalid; review is required.",
        )
    })?;
    let history_repository = match saved["schema_version"].as_u64() {
        Some(1) => legacy_repository_path(path, state, saved["worktree"].as_str())?,
        Some(2) if saved["task_repository"].as_str() == state.task_repository.to_str() => {
            Some(state.task_repository.clone())
        }
        _ => None,
    };
    let valid = history_repository.is_some()
        && saved["phase"] == "recovered_once"
        && saved["branch"].as_str() == Some(state.branch.as_str())
        && saved["base"].as_str() == Some(plan.remote_commit.as_str())
        && saved["target"].as_str() == Some(plan.local_commit.as_str())
        && saved["merge_head"].as_str() == Some(plan.local_commit.as_str());
    anyhow::ensure!(
        valid,
        "{}",
        review_required(
            state,
            plan,
            path,
            "The prior recovery is incomplete or belongs to different pinned histories."
        )
    );
    let history_repository = history_repository.as_deref().unwrap();
    let retained: Vec<String> = match saved.get("retained_configuration_paths") {
        Some(value) => serde_json::from_value(value.clone())?,
        None => Vec::new(),
    };
    let current = crate::harness::pi_sandbox::runtime_config::paths_with_source(
        &state.task_repository,
        runner.runtime_config_source.as_deref(),
    )?;
    anyhow::ensure!(
        retained.iter().all(|path| current.contains(path)),
        "Previously retained runtime configuration is no longer granted; recovery is preserved for review"
    );
    let stash = saved["stash_commit"]
        .as_str()
        .filter(|id| id.len() == 40 && id.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or_else(|| {
            review_required(
                state,
                plan,
                path,
                "The prior recovery has no valid stash identity.",
            )
        })?;
    let private_ref = saved["private_ref"]
        .as_str()
        .filter(|name| {
            name.starts_with(&format!(
                "refs/koolade/reconciliation-recovery/{}/",
                task_repository::allocation_key(state)
            ))
        })
        .ok_or_else(|| {
            review_required(
                state,
                plan,
                path,
                "The prior recovery has no valid preservation ref.",
            )
        })?;
    anyhow::ensure!(
        runner.git(
            history_repository,
            &["rev-parse", "--verify", &format!("{stash}^{{commit}}")]
        )? == stash
            && runner.git(history_repository, &["rev-parse", "--verify", private_ref])? == stash,
        "The prior recovery preservation ref changed; review is required"
    );
    let ancestry = runner.git(
        history_repository,
        &["rev-list", "--parents", "-n", "1", stash],
    )?;
    let parents = ancestry.split_whitespace().collect::<Vec<_>>();
    anyhow::ensure!(
        matches!(parents.len(), 3 | 4) && parents[0] == stash,
        "Prior recovery has an invalid stash shape; preserved for review"
    );
    for tree in std::iter::once(stash).chain(parents.iter().skip(2).copied()) {
        let files = runner.git(
            history_repository,
            &["ls-tree", "-r", "--name-only", "-z", tree],
        )?;
        anyhow::ensure!(
            !files.split('\0').any(|file| current
                .iter()
                .any(|path| file == path || file.starts_with(&format!("{path}/")))),
            "Prior recovery already contains newly granted configuration; its snapshot is preserved for review"
        );
    }
    let archive = path
        .parent()
        .unwrap()
        .join("reconciliation-recovery-history");
    fs::create_dir_all(&archive)?;
    let nonce = chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default();
    let destination = archive.join(format!("{nonce}.json"));
    // A hard link publishes the complete saved bytes without overwriting history.
    // Only then may the current snapshot path be reused by this invocation.
    fs::hard_link(path, &destination)?;
    fs::remove_file(path)?;
    Ok(())
}

fn legacy_repository_path(
    snapshot_path: &Path,
    state: &Implementation,
    saved_path: Option<&str>,
) -> anyhow::Result<Option<PathBuf>> {
    if state.task_repository_kind == crate::core::implementation::TaskRepositoryKind::LegacyWorktree
    {
        return Ok(
            (saved_path == state.task_repository.to_str()).then(|| state.task_repository.clone())
        );
    }
    if state.task_repository_kind != crate::core::implementation::TaskRepositoryKind::Clone {
        return Ok(None);
    }
    let Some(directory) = snapshot_path.parent() else {
        return Ok(None);
    };
    let migration_directory = directory.join("legacy-migration");
    let metadata = match fs::symlink_metadata(&migration_directory) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    anyhow::ensure!(
        metadata.is_dir() && !metadata.file_type().is_symlink(),
        "Legacy migration provenance is not a real directory"
    );
    let record_path = migration_directory.join("legacy-migration.json");
    let metadata = match fs::symlink_metadata(&record_path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    anyhow::ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink(),
        "Legacy migration provenance is not a regular file"
    );
    let record: serde_json::Value = serde_json::from_slice(&fs::read(record_path)?)?;
    if record["schema_version"] == 1
        && record["source"].as_str() == saved_path
        && record["destination"].as_str() == state.task_repository.to_str()
    {
        let source = PathBuf::from(saved_path.unwrap_or_default());
        let metadata = match fs::symlink_metadata(&source) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        anyhow::ensure!(
            metadata.is_dir() && !metadata.file_type().is_symlink(),
            "Legacy migration source is not a real directory"
        );
        Ok(Some(source))
    } else {
        Ok(None)
    }
}
