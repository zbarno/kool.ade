use super::*;

/// Recover an external checkpoint after a process stopped before it could
/// update state.json. Only the newest report counts; older blockers cannot
/// override later successful work.
pub fn latest_external_blocker(repo: &Path, ticket: &str) -> Option<String> {
    let dir = state_dir(repo, ticket).ok()?;
    let path = fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| {
                    name.ends_with("-report.json") && name != "verified-report.json"
                })
        })
        .max()?;
    let report = parse_report(&fs::read_to_string(&path).ok()?).ok()?;
    external_blocker(&report).then(|| external_blocker_detail(&report, &path))
}
