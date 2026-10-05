mod fingerprint;
use fingerprint::fingerprint;

use super::super::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

const FILE: &str = "base-reconciliation-generated.json";

#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Ledger {
    schema_version: u8,
    worktree: PathBuf,
    branch: String,
    ticket: String,
    local: String,
    remote: String,
    files: BTreeMap<String, String>,
}

pub(super) struct Before {
    paths: BTreeSet<String>,
    trusted: BTreeSet<String>,
    ledger: Ledger,
}

fn load(dir: &Path, state: &Implementation) -> anyhow::Result<Ledger> {
    let plan = read_plan(&dir.join(PLAN_FILE))?;
    let path = dir.join(FILE);
    let ledger = if path.exists() {
        anyhow::ensure!(
            fs::metadata(&path)?.len() <= 16 * 1024 * 1024,
            "Verification artifact ledger is too large; preserved for review"
        );
        serde_json::from_slice::<Ledger>(&fs::read(path)?)?
    } else {
        Ledger {
            schema_version: 1,
            worktree: state.worktree.canonicalize()?,
            branch: state.branch.clone(),
            ticket: state.ticket.clone(),
            local: plan.local_commit.clone(),
            remote: plan.remote_commit.clone(),
            files: BTreeMap::new(),
        }
    };
    anyhow::ensure!(
        ledger.schema_version == 1
            && ledger.worktree == state.worktree.canonicalize()?
            && ledger.branch == state.branch
            && ledger.ticket == state.ticket
            && ledger.local == plan.local_commit
            && ledger.remote == plan.remote_commit,
        "Verification artifact identity changed; the worktree is preserved for review"
    );
    Ok(ledger)
}

fn inventory(runner: &Runner, worktree: &Path) -> anyhow::Result<BTreeSet<String>> {
    let mut paths = BTreeSet::new();
    for args in [
        vec!["ls-files", "--others", "--exclude-standard", "-z"],
        vec![
            "ls-files",
            "--others",
            "--ignored",
            "--exclude-standard",
            "-z",
        ],
    ] {
        paths.extend(
            runner
                .git(worktree, &args)?
                .split('\0')
                .filter(|path| !path.is_empty())
                .map(str::to_owned),
        );
    }
    anyhow::ensure!(
        paths.len() <= 100_000,
        "Verification artifact inventory is too large; preserved for review"
    );
    Ok(paths)
}

pub(in crate::core::implementation) fn trusted(
    runner: &Runner,
    state: &Implementation,
    dir: &Path,
) -> anyhow::Result<BTreeSet<String>> {
    let ledger = load(dir, state)?;
    let mut trusted = crate::harness::pi_sandbox::runtime_config::paths(&state.worktree)?;
    for path in inventory(runner, &state.worktree)? {
        if !eligible(&path) {
            continue;
        }
        if let Some(expected) = ledger.files.get(&path)
            && fingerprint(&state.worktree, &path)?.as_ref() == Some(expected)
        {
            trusted.insert(path);
        }
    }
    Ok(trusted)
}

pub(super) fn before(
    runner: &Runner,
    state: &Implementation,
    dir: &Path,
) -> anyhow::Result<Before> {
    Ok(Before {
        paths: inventory(runner, &state.worktree)?,
        trusted: trusted(runner, state, dir)?,
        ledger: load(dir, state)?,
    })
}

pub(super) fn after(
    runner: &Runner,
    state: &Implementation,
    dir: &Path,
    mut before: Before,
) -> anyhow::Result<()> {
    let paths = inventory(runner, &state.worktree)?;
    // Refresh only previously unchanged output, or files absent before verification.
    // Preexisting unknown files and operator edits never acquire verification provenance.
    for path in paths.difference(&before.paths).chain(before.trusted.iter()) {
        if !eligible(path) {
            continue;
        }
        if let Some(hash) = fingerprint(&state.worktree, path)? {
            before.ledger.files.insert(path.clone(), hash);
        } else {
            before.ledger.files.remove(path);
        }
    }
    before.ledger.files.retain(|path, _| paths.contains(path));
    crate::artifacts::atomic_write_bytes(
        &dir.join(FILE),
        &serde_json::to_vec_pretty(&before.ledger)?,
    )
}

fn eligible(path: &str) -> bool {
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

pub(in crate::core::implementation) fn clean(
    runner: &Runner,
    state: &Implementation,
    dir: &Path,
) -> anyhow::Result<bool> {
    let generated = if dir.join(FILE).exists() {
        trusted(runner, state, dir)?
    } else {
        crate::harness::pi_sandbox::runtime_config::paths(&state.worktree)?
    };
    for args in [
        vec!["diff", "--name-only", "-z"],
        vec!["diff", "--cached", "--name-only", "-z"],
        vec!["ls-files", "--others", "--exclude-standard", "-z"],
    ] {
        if runner
            .git(&state.worktree, &args)?
            .split('\0')
            .filter(|path| !path.is_empty())
            .any(|path| !generated.contains(path))
        {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(in crate::core::implementation) fn stage_task(
    runner: &Runner,
    state: &Implementation,
    dir: &Path,
) -> anyhow::Result<()> {
    let mut protected = crate::harness::pi_sandbox::runtime_config::paths(&state.worktree)?;
    if dir.join(FILE).exists() {
        let ledger = load(dir, state)?;
        let trusted = trusted(runner, state, dir)?;
        let current = inventory(runner, &state.worktree)?;
        anyhow::ensure!(
            ledger
                .files
                .keys()
                .all(|path| !current.contains(path) || trusted.contains(path)),
            "Verification output changed outside application verification; preserved for review"
        );
        protected.extend(trusted);
    }
    let mut args = vec!["add".to_owned(), "--all".into(), "--".into(), ".".into()];
    let visible = runner.git(
        &state.worktree,
        &["ls-files", "--others", "--exclude-standard", "-z"],
    )?;
    let visible = visible.split('\0').collect::<BTreeSet<_>>();
    args.extend(
        protected
            .iter()
            .filter(|path| visible.contains(path.as_str()))
            .map(|path| format!(":(exclude,literal){path}")),
    );
    runner.git(
        &state.worktree,
        &args.iter().map(String::as_str).collect::<Vec<_>>(),
    )?;
    Ok(())
}

#[cfg(test)]
mod tests;
