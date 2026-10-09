mod fingerprint;
mod ledger;
mod quarantine;
use fingerprint::fingerprint;
use ledger::{Ledger, eligible, ignored_inventory, inventory, load, visible_inventory};
pub(in crate::core::implementation) use quarantine::quarantine_untrusted_ignored;

use super::super::*;
use std::collections::BTreeSet;

const FILE: &str = "base-reconciliation-generated.json";

pub(super) struct Before {
    paths: BTreeSet<String>,
    trusted: BTreeSet<String>,
    ledger: Ledger,
}

pub(in crate::core::implementation) fn trusted(
    runner: &Runner,
    state: &Implementation,
    dir: &Path,
) -> anyhow::Result<BTreeSet<String>> {
    let ledger = load(dir, state)?;
    let mut trusted = crate::harness::pi_sandbox::runtime_config::paths_with_source(
        &state.task_repository,
        runner.runtime_config_source.as_deref(),
    )?;
    for path in inventory(runner, &state.task_repository)? {
        if !eligible(&path) {
            continue;
        }
        if let Some(expected) = ledger.files.get(&path)
            && fingerprint(&state.task_repository, &path)?.as_ref() == Some(expected)
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
        paths: inventory(runner, &state.task_repository)?,
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
    let paths = inventory(runner, &state.task_repository)?;
    // Refresh only previously unchanged output, or files absent before verification.
    // Preexisting unknown files and operator edits never acquire verification provenance.
    for path in paths.difference(&before.paths).chain(before.trusted.iter()) {
        if !eligible(path) {
            continue;
        }
        if let Some(hash) = fingerprint(&state.task_repository, path)? {
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

pub(in crate::core::implementation) fn verify(
    runner: &Runner,
    state: &Implementation,
    dir: &Path,
    command: &str,
) -> anyhow::Result<String> {
    quarantine_untrusted_ignored(runner, state, dir)?;
    let before = before(runner, state, dir)?;
    let result = runner.verify(&state.task_repository, command);
    after(runner, state, dir, before)?;
    result
}

pub(in crate::core::implementation) fn clean(
    runner: &Runner,
    state: &Implementation,
    dir: &Path,
) -> anyhow::Result<bool> {
    let generated =
        if dir.join(FILE).exists() || super::super::integrated_candidate_matches(dir, state)? {
            trusted(runner, state, dir)?
        } else {
            crate::harness::pi_sandbox::runtime_config::paths_with_source(
                &state.task_repository,
                runner.runtime_config_source.as_deref(),
            )?
        };
    for args in [
        vec!["diff", "--name-only", "-z"],
        vec!["diff", "--cached", "--name-only", "-z"],
    ] {
        if runner
            .git_nul_records(&state.task_repository, &args)?
            .iter()
            .any(|path| !generated.contains(path))
        {
            return Ok(false);
        }
    }
    if visible_inventory(runner, &state.task_repository)?
        .iter()
        .any(|path| !generated.contains(path))
    {
        return Ok(false);
    }
    if ignored_inventory(runner, &state.task_repository)?
        .iter()
        .any(|path| !generated.contains(path))
    {
        return Ok(false);
    }
    Ok(true)
}

pub(in crate::core::implementation) fn stage_task(
    runner: &Runner,
    state: &Implementation,
    dir: &Path,
) -> anyhow::Result<()> {
    let mut protected = crate::harness::pi_sandbox::runtime_config::paths_with_source(
        &state.task_repository,
        runner.runtime_config_source.as_deref(),
    )?;
    if dir.join(FILE).exists() || super::super::integrated_candidate_matches(dir, state)? {
        let ledger = load(dir, state)?;
        let trusted = trusted(runner, state, dir)?;
        let current = inventory(runner, &state.task_repository)?;
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
    let visible = runner.git_nul_records(
        &state.task_repository,
        &["ls-files", "--others", "--exclude-standard", "-z"],
    )?;
    let visible = visible.into_iter().collect::<BTreeSet<_>>();
    args.extend(
        protected
            .iter()
            .filter(|path| visible.contains(path.as_str()))
            .map(|path| format!(":(exclude,literal){path}")),
    );
    runner.git(
        &state.task_repository,
        &args.iter().map(String::as_str).collect::<Vec<_>>(),
    )?;
    Ok(())
}

#[cfg(test)]
mod tests;
