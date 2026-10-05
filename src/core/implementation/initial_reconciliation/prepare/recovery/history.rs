use super::*;

pub(super) fn archive(
    repo: &Path,
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
    let valid = saved["schema_version"] == 1
        && saved["phase"] == "recovered_once"
        && saved["worktree"].as_str() == state.worktree.to_str()
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
    let retained: Vec<String> = match saved.get("retained_configuration_paths") {
        Some(value) => serde_json::from_value(value.clone())?,
        None => Vec::new(),
    };
    let current = crate::harness::pi_sandbox::runtime_config::paths(&state.worktree)?;
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
                key_for_ticket(&state.ticket)
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
            repo,
            &["rev-parse", "--verify", &format!("{stash}^{{commit}}")]
        )? == stash
            && runner.git(repo, &["rev-parse", "--verify", private_ref])? == stash,
        "The prior recovery preservation ref changed; review is required"
    );
    let ancestry = runner.git(repo, &["rev-list", "--parents", "-n", "1", stash])?;
    let parents = ancestry.split_whitespace().collect::<Vec<_>>();
    anyhow::ensure!(
        matches!(parents.len(), 3 | 4) && parents[0] == stash,
        "Prior recovery has an invalid stash shape; preserved for review"
    );
    for tree in std::iter::once(stash).chain(parents.iter().skip(2).copied()) {
        let files = runner.git(repo, &["ls-tree", "-r", "--name-only", "-z", tree])?;
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
