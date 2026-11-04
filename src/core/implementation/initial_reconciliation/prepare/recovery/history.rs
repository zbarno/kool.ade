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
    let history_repository = (saved["schema_version"].as_u64() == Some(2)
        && saved["task_repository"].as_str() == state.task_repository.to_str())
    .then(|| state.task_repository.clone());
    let valid = history_repository.is_some()
        && snapshot_matches_pinned_history(
            &saved,
            &state.branch,
            &plan.remote_commit,
            &plan.local_commit,
            &plan.local_commit,
        );
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
    let current = crate::harness::runtime_config::paths_with_source(
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
    let allocation_key = task_repository::allocation_key(state)?;
    let private_ref = valid_private_ref(&saved, &allocation_key).ok_or_else(|| {
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
        let files = runner.git_nul_records(
            history_repository,
            &["ls-tree", "-r", "--name-only", "-z", tree],
        )?;
        anyhow::ensure!(
            !files.iter().any(|file| current
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

fn snapshot_matches_pinned_history(
    saved: &serde_json::Value,
    branch: &str,
    base: &str,
    target: &str,
    merge_head: &str,
) -> bool {
    saved["phase"] == "recovered_once"
        && saved["branch"].as_str() == Some(branch)
        && saved["base"].as_str() == Some(base)
        && saved["target"].as_str() == Some(target)
        && saved["merge_head"].as_str() == Some(merge_head)
}

fn valid_private_ref<'a>(saved: &'a serde_json::Value, allocation_key: &str) -> Option<&'a str> {
    saved["private_ref"].as_str().filter(|name| {
        name.starts_with(&format!(
            "refs/koolade/reconciliation-recovery/{allocation_key}/"
        ))
    })
}

#[cfg(test)]
#[path = "history/tests.rs"]
mod tests;
