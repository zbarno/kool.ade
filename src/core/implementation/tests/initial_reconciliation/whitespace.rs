use super::*;

struct Resolution(&'static str);
impl AiHarness for Resolution {
    fn label(&self) -> String {
        "whitespace resolution fixture".into()
    }
    fn check_available(&self) -> Result<String, AppError> {
        Ok("fixture".into())
    }
    fn execute(
        &self,
        request: &PlanningRequest,
    ) -> Result<crate::harness::HarnessOutcome, AppError> {
        if request.prompt_body.contains("INITIAL BASE RECONCILIATION") {
            fs::write(request.repo_root.join("shared.txt"), self.0).unwrap();
            return Ok(outcome(complete_report("test -f shared.txt")));
        }
        let (_, agent) = super::merge_recovery::agent();
        agent.execute(request)
    }
}

#[test]
fn new_resolution_whitespace_is_rejected_before_baseline_commit() {
    let s = conflict_sandbox();
    let error = run_with_agent(&s, &Resolution("both edits preserved \n"), None)
        .unwrap_err()
        .to_string();
    assert!(error.contains("New whitespace defects"), "{error}");
    let state = load(&s.repo, &s.ticket).unwrap();
    assert_eq!(state.status, ImplementationStatus::Blocked);
    assert!(!state.worktree.join("implemented.txt").exists());
    assert!(
        !s.git(&state.worktree, &["diff", "--cached", "--name-only"])
            .is_empty()
    );
}

#[test]
fn moved_whitespace_inherited_from_both_sources_is_preserved() {
    let s = conflict_sandbox();
    let peer = s.root.join("peer");
    fs::write(peer.join("shared.txt"), "shared implementation \n").unwrap();
    s.git(&peer, &["add", "shared.txt"]);
    s.git(&peer, &["commit", "-qm", "retain shared whitespace"]);
    s.git(&peer, &["push", "-q", "origin", "main"]);
    fs::write(s.repo.join("shared.txt"), "local implementation \n").unwrap();
    s.git(&s.repo, &["add", "shared.txt"]);
    s.git(&s.repo, &["commit", "-qm", "retain local whitespace"]);
    let contents = "local implementation \nshared implementation \n";
    let result = run_with_agent(&s, &Resolution(contents), None).unwrap();
    assert_eq!(result.status, ImplementationStatus::AwaitingReview);
    assert_eq!(
        fs::read_to_string(result.worktree.join("shared.txt")).unwrap(),
        contents
    );
}

#[test]
fn inherited_defect_counts_cannot_exempt_extra_resolution_duplicates() {
    let s = conflict_sandbox();
    let peer = s.root.join("peer");
    fs::write(peer.join("shared.txt"), "line \nremote\n").unwrap();
    s.git(&peer, &["add", "shared.txt"]);
    s.git(&peer, &["commit", "-qm", "one inherited defect"]);
    s.git(&peer, &["push", "-q", "origin", "main"]);
    fs::write(s.repo.join("shared.txt"), "line \nline \nlocal\n").unwrap();
    s.git(&s.repo, &["add", "shared.txt"]);
    s.git(&s.repo, &["commit", "-qm", "two inherited defects"]);
    let error = run_with_agent(&s, &Resolution("line \nline \nline \nresolved\n"), None)
        .unwrap_err()
        .to_string();
    assert!(error.contains("New whitespace defects"), "{error}");
    assert!(
        !load(&s.repo, &s.ticket)
            .unwrap()
            .worktree
            .join("implemented.txt")
            .exists()
    );
}
