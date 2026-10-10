use super::*;
use crate::artifacts::planning_store::PlanningStore;

pub fn load_all(repo: &Path) -> Vec<Implementation> {
    let Ok(roots) = state_paths::implementation_roots(repo) else {
        return Vec::new();
    };
    let mut seen = std::collections::BTreeSet::new();
    roots
        .into_iter()
        .flat_map(|root| {
            fs::read_dir(root)
                .into_iter()
                .flatten()
                .filter_map(Result::ok)
        })
        .filter_map(|entry| read_state_file(&entry.path().join("state.json")).ok())
        .filter(|state| {
            seen.insert(
                state
                    .task_uid
                    .as_deref()
                    .map(|uid| format!("uid:{uid}"))
                    .unwrap_or_else(|| format!("ticket:{}", state.ticket)),
            )
        })
        .collect()
}
pub fn load(repo: &Path, ticket: &str) -> Option<Implementation> {
    let store = PlanningStore::legacy_embedded(uuid::Uuid::nil(), repo);
    load_with_store(&store, repo, ticket)
}

pub fn load_with_store(
    planning_store: &PlanningStore,
    state_root: &Path,
    ticket: &str,
) -> Option<Implementation> {
    let uid = ticket_identity(planning_store, ticket).ok().flatten();
    let directory = state_dir_for_task(state_root, ticket, uid.as_deref()).ok()?;
    read_state_file(&directory.join("state.json")).ok()
}

pub(super) fn resume_failure_context(detail: &str) -> String {
    // Also unwrap legacy errors whose complete correction histories were nested
    // on every resume. Keep only the newest diagnostic in the active prompt.
    let start = detail
        .rfind("\nAttempt ")
        .into_iter()
        .chain(detail.rfind("\nHarness failure "))
        .max();
    let latest = start.map(|index| &detail[index + 1..]).unwrap_or(detail);
    let latest = latest
        .rsplit_once("Latest failure: ")
        .map(|(_, tail)| tail)
        .unwrap_or(latest);
    let latest = latest
        .split("\nAUTOMATIC BLOCKER RECOVERY REQUIRED")
        .next()
        .unwrap_or(latest);
    let latest = latest
        .split("\nSELF-REPAIR REQUIRED")
        .next()
        .unwrap_or(latest);
    crate::core::context_build::clip(latest, 4000)
}

pub(super) fn history_preflight_context(
    runner: &Runner,
    worktree: &Path,
    base_commit: &str,
    ticket_text: &str,
) -> anyhow::Result<String> {
    let anchors = ticket_text
        .split(|c: char| !c.is_ascii_hexdigit())
        .filter(|token| (7..=40).contains(&token.len()))
        .map(str::to_ascii_lowercase)
        .collect::<BTreeSet<_>>();
    let mut evidence = format!(
        "Task base: {base_commit}\nTicket commit references: {}\n",
        anchors.len()
    );
    for anchor in anchors.iter().take(20) {
        let Ok(resolved) = runner.git(
            worktree,
            &["rev-parse", "--verify", &format!("{anchor}^{{commit}}")],
        ) else {
            evidence.push_str(&format!(
                "\n{anchor}: not a resolvable commit in this checkout\n"
            ));
            continue;
        };
        let common = runner.git(worktree, &["merge-base", &resolved, base_commit]);
        match common {
            Ok(common) if common == resolved => {
                let paths = runner.git(worktree, &["diff", "--name-status", &resolved, base_commit])?;
                evidence.push_str(&format!(
                    "\n{anchor} resolves to {resolved} and is an ancestor of the task base.\nChanged paths from that checkpoint to the task base (git diff --name-status):\n{}\n",
                    if paths.is_empty() { "(none)" } else { paths.as_str() }
                ));
            }
            Ok(common) => evidence.push_str(&format!(
                "\n{anchor} resolves to {resolved}, but is not an ancestor of the task base (merge base {common}).\n"
            )),
            Err(_) => evidence.push_str(&format!(
                "\n{anchor} resolves to {resolved}, but shares no reachable history with the task base.\n"
            )),
        }
    }
    if anchors.len() > 20 {
        evidence.push_str(&format!(
            "\nOnly the first 20 of {} ticket references are shown.\n",
            anchors.len()
        ));
    }
    Ok(evidence)
}
