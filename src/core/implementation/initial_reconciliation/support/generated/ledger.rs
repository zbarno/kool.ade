use super::*;
use crate::core::implementation::Runner;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Ledger {
    schema_version: u8,
    #[serde(alias = "worktree")]
    task_repository: PathBuf,
    branch: String,
    ticket: String,
    #[serde(default)]
    task_uid: Option<String>,
    #[serde(default)]
    allocation_key: Option<String>,
    local: String,
    remote: String,
    pub(super) files: BTreeMap<String, String>,
}

pub(super) fn load(dir: &Path, state: &Implementation) -> anyhow::Result<Ledger> {
    let plan_path = dir.join(PLAN_FILE);
    let plan = plan_path
        .exists()
        .then(|| read_plan(&plan_path))
        .transpose()?;
    let repository = state.task_repository.canonicalize()?;
    let path = dir.join(FILE);
    let mut ledger = if path.exists() {
        anyhow::ensure!(
            fs::metadata(&path)?.len() <= 16 * 1024 * 1024,
            "Verification artifact ledger is too large; preserved for review"
        );
        serde_json::from_slice::<Ledger>(&fs::read(path)?)?
    } else {
        empty_ledger(state, &repository, plan.as_ref())
    };

    if super::super::super::integrated_candidate_matches(dir, state)? {
        let integration_dir = dir.join(format!("integration-{}", state.base_commit));
        if integration_dir.join(FILE).exists()
            || ledger.task_repository != repository
            || ledger.branch != state.branch
        {
            // The root ledger belongs to the original task clone or is absent.
            // Use provenance recorded during integration verification for this
            // exact candidate.
            ledger = load(&integration_dir, state)?;
            if let Some(plan) = plan.as_ref() {
                ledger.local = plan.local_commit.clone();
                ledger.remote = plan.remote_commit.clone();
            }
        }
    }

    let same_renamed_task = state.task_uid.is_some()
        && state.task_repository_allocation_key.is_some()
        && ledger
            .task_uid
            .as_ref()
            .is_none_or(|uid| state.task_uid.as_ref() == Some(uid))
        && ledger
            .allocation_key
            .as_ref()
            .is_none_or(|key| state.task_repository_allocation_key.as_ref() == Some(key));
    let (local, remote) = plan
        .as_ref()
        .map(|plan| (plan.local_commit.as_str(), plan.remote_commit.as_str()))
        .unwrap_or_default();
    anyhow::ensure!(
        ledger.schema_version == 1
            && ledger.task_repository == repository
            && ledger.branch == state.branch
            && (ledger.ticket == state.ticket || same_renamed_task)
            && ledger.local == local
            && ledger.remote == remote,
        "Verification artifact identity changed; the task repository is preserved for review"
    );
    ledger.ticket = state.ticket.clone();
    ledger.task_uid = state.task_uid.clone();
    ledger.allocation_key = state.task_repository_allocation_key.clone();
    Ok(ledger)
}

fn empty_ledger(state: &Implementation, repository: &Path, plan: Option<&Plan>) -> Ledger {
    Ledger {
        schema_version: 1,
        task_repository: repository.to_owned(),
        branch: state.branch.clone(),
        ticket: state.ticket.clone(),
        task_uid: state.task_uid.clone(),
        allocation_key: state.task_repository_allocation_key.clone(),
        local: plan.map_or_else(String::new, |plan| plan.local_commit.clone()),
        remote: plan.map_or_else(String::new, |plan| plan.remote_commit.clone()),
        files: BTreeMap::new(),
    }
}

pub(super) fn inventory(runner: &Runner, worktree: &Path) -> anyhow::Result<BTreeSet<String>> {
    let mut paths = visible_inventory(runner, worktree)?;
    paths.extend(ignored_inventory(runner, worktree)?);
    anyhow::ensure!(
        paths.len() <= 100_000,
        "Verification artifact inventory is too large; preserved for review"
    );
    Ok(paths)
}

pub(super) fn visible_inventory(
    runner: &Runner,
    worktree: &Path,
) -> anyhow::Result<BTreeSet<String>> {
    inventory_for(
        runner,
        worktree,
        &["ls-files", "--others", "--exclude-standard", "-z"],
    )
}

pub(super) fn ignored_inventory(
    runner: &Runner,
    worktree: &Path,
) -> anyhow::Result<BTreeSet<String>> {
    inventory_for(
        runner,
        worktree,
        &[
            "ls-files",
            "--others",
            "--ignored",
            "--exclude-standard",
            "-z",
        ],
    )
}

fn inventory_for(
    runner: &Runner,
    worktree: &Path,
    args: &[&str],
) -> anyhow::Result<BTreeSet<String>> {
    Ok(runner
        .git_nul_records(worktree, args)?
        .into_iter()
        .collect())
}

pub(super) fn eligible(path: &str) -> bool {
    let parts = path.split('/').collect::<Vec<_>>();
    parts
        .iter()
        .take(parts.len().saturating_sub(1))
        .any(|part| {
            matches!(
                *part,
                "bin"
                    | "obj"
                    | "target"
                    | "node_modules"
                    | "dist"
                    | "build"
                    | "coverage"
                    | ".cache"
            )
        })
        || (parts.len() >= 2
            && parts[parts.len() - 2] == "DataProtectionKeys"
            && path.ends_with(".xml"))
}
