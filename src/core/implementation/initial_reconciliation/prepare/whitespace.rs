use super::super::*;
use std::collections::BTreeMap;

const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

// Compare defects, not line numbers: conflict resolution can move an inherited
// line. Counts still prevent a new duplicate from borrowing an old exemption.
pub(super) fn check(runner: &Runner, worktree: &Path, plan: &Plan) -> anyhow::Result<()> {
    let paths = runner.git(
        worktree,
        &["diff", "--cached", "--name-only", "-z", &plan.remote_commit],
    )?;
    for path in paths.split('\0').filter(|path| !path.is_empty()) {
        let local_diff = runner.git(
            worktree,
            &[
                "--literal-pathspecs",
                "diff",
                "--cached",
                "--name-only",
                "-z",
                &plan.local_commit,
                "--",
                path,
            ],
        )?;
        if local_diff.is_empty() {
            continue;
        }
        let result = defects(runner, worktree, &["--cached", EMPTY_TREE], path)?;
        if result.is_empty() {
            continue;
        }
        let mut inherited = defects(runner, worktree, &[EMPTY_TREE, &plan.remote_commit], path)?;
        for (defect, count) in defects(runner, worktree, &[EMPTY_TREE, &plan.local_commit], path)? {
            inherited
                .entry(defect)
                .and_modify(|existing| *existing = (*existing).max(count))
                .or_insert(count);
        }
        anyhow::ensure!(
            result
                .iter()
                .all(|(defect, count)| *count <= inherited.get(defect).copied().unwrap_or(0)),
            "New whitespace defects in {path}; preserve inherited content and repair only reconciliation edits"
        );
    }
    Ok(())
}

fn defects(
    runner: &Runner,
    worktree: &Path,
    revisions: &[&str],
    path: &str,
) -> anyhow::Result<BTreeMap<String, usize>> {
    let mut args = vec!["--literal-pathspecs", "diff", "--check"];
    args.extend_from_slice(revisions);
    args.extend_from_slice(&["--", path]);
    let output = match runner.git(worktree, &args) {
        Ok(_) => return Ok(BTreeMap::new()),
        Err(error) => {
            let message = error.to_string();
            let Some((_, tail)) = message.split_once("git failed:\nstdout:\n") else {
                return Err(error);
            };
            let Some((stdout, stderr)) = tail.split_once("\nstderr:\n") else {
                return Err(error);
            };
            if !stderr.trim().is_empty() || stdout.trim().is_empty() {
                return Err(error);
            }
            stdout.to_owned()
        }
    };
    let mut lines = output.lines().peekable();
    let mut counts = BTreeMap::new();
    while let Some(header) = lines.next() {
        let Some((location, kind)) = header.rsplit_once(": ") else {
            anyhow::bail!("Unrecognized whitespace diagnostic; preserving worktree");
        };
        let Some((reported_path, line)) = location.rsplit_once(':') else {
            anyhow::bail!("Invalid whitespace diagnostic");
        };
        anyhow::ensure!(
            reported_path == path && line.parse::<usize>().is_ok(),
            "Unexpected whitespace diagnostic path"
        );
        let mut content = String::new();
        while lines.peek().is_some_and(|line| line.starts_with('+')) {
            content.push_str(lines.next().unwrap());
            content.push('\n');
        }
        *counts.entry(format!("{kind}\n{content}")).or_insert(0) += 1;
    }
    Ok(counts)
}
