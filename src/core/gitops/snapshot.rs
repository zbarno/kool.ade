use super::*;

pub fn snapshot(cwd: &Path) -> GitSnapshot {
    let mut snap = GitSnapshot::default();
    if let Ok((_, out, _)) = run(cwd, &["symbolic-ref", "--short", "HEAD"]) {
        snap.branch = out.trim().to_string();
    }
    if let Ok((code, out, _)) = run(
        cwd,
        &[
            "for-each-ref",
            "--format=%(refname:short)",
            "refs/heads",
            "refs/remotes/origin",
        ],
    ) && code == 0
    {
        snap.branches = out
            .lines()
            .map(|name| name.strip_prefix("origin/").unwrap_or(name))
            .filter(|name| !name.is_empty() && *name != "HEAD")
            .map(str::to_owned)
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
    }
    snap.has_origin =
        run(cwd, &["remote", "get-url", "origin"]).is_ok_and(|(code, _, _)| code == 0);
    if let Ok((code, out, _)) = run(
        cwd,
        &[
            "for-each-ref",
            "--format=%(refname:short)",
            "refs/remotes/origin",
        ],
    ) && code == 0
    {
        snap.remote_branches = out
            .lines()
            .map(|name| name.strip_prefix("origin/").unwrap_or(name))
            .filter(|name| !name.is_empty() && *name != "HEAD")
            .map(str::to_owned)
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
    }
    if let Ok((code, out, _)) = run(
        cwd,
        &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"],
    ) && code == 0
    {
        snap.default_branch = out
            .trim()
            .strip_prefix("origin/")
            .unwrap_or(out.trim())
            .into();
    }
    if snap.branch.is_empty() {
        // Detached HEAD: label by short sha.
        if let Ok((code, out, _)) = run(cwd, &["rev-parse", "--short", "HEAD"])
            && code == 0
        {
            snap.branch = format!("detached @{head}", head = out.trim());
        }
    }
    if snap.default_branch.is_empty() {
        snap.default_branch = snap.branch.clone();
    }
    if let Ok((code, out, _)) = run(cwd, &["rev-parse", "--short", "HEAD"])
        && code == 0
    {
        snap.head_short = out.trim().to_string();
    }
    if let Ok((code, out, _)) = run(cwd, &["status", "--porcelain=v1"])
        && code == 0
    {
        snap.dirty = out.lines().count();
    }
    if let Ok((code, out, _)) = run(cwd, &["log", "-1", "--pretty=%s"])
        && code == 0
    {
        snap.last_subject = out.trim().to_string();
    }
    snap
}
