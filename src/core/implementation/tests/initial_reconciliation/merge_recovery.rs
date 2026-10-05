use super::*;

#[path = "merge_recovery/cases.rs"]
mod cases;
#[path = "merge_recovery/snapshots.rs"]
mod snapshots;

pub(super) fn make_divergent(s: &Sandbox) -> (String, String, String) {
    fs::write(s.repo.join(".gitignore"), ".cache/\n").unwrap();
    s.git(&s.repo, &["add", ".gitignore"]);
    s.git(&s.repo, &["commit", "-qm", "ignore generated cache"]);
    s.git(&s.repo, &["push", "-q", "origin", "main"]);
    let remote = s.advance_remote();
    s.git(&s.repo, &["fetch", "-q", "origin", "main"]);
    fs::write(s.repo.join("local.txt"), "local change\n").unwrap();
    s.git(&s.repo, &["add", "local.txt"]);
    s.git(&s.repo, &["commit", "-qm", "local change"]);
    let local = s.git(&s.repo, &["rev-parse", "HEAD"]);
    let common = s.git(&s.repo, &["merge-base", &local, &remote]);
    (remote, local, common)
}

pub(super) fn recovery_snapshot(s: &Sandbox) -> (std::path::PathBuf, serde_json::Value) {
    let dir = state_dir(&s.repo, &s.ticket).unwrap();
    let path = dir.join("base-reconciliation-recovery.json");
    let snapshot = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    (path, snapshot)
}

pub(super) fn task_worktree(s: &Sandbox) -> std::path::PathBuf {
    let key = crate::core::implementation::key_for_ticket(&s.ticket);
    s.repo
        .parent()
        .unwrap()
        .join(".koolade-worktrees")
        .join(crate::persistence::project_slug(
            &s.repo.canonicalize().unwrap(),
        ))
        .join(key)
}

pub(super) fn agent() -> (Arc<AtomicUsize>, ReconcilingAgent) {
    let calls = Arc::new(AtomicUsize::new(0));
    (
        calls.clone(),
        ReconcilingAgent {
            calls,
            conflict: false,
            block: false,
            expect_snapshot: false,
            wrap_report: false,
        },
    )
}
