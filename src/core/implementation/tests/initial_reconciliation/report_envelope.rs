use super::*;

#[test]
fn preface_and_json_fence_are_removed_before_report_validation() {
    let sandbox = conflict_sandbox();
    let agent = ReconcilingAgent {
        calls: Arc::new(AtomicUsize::new(0)),
        conflict: true,
        block: false,
        expect_snapshot: false,
        wrap_report: true,
    };

    let result = run_with_agent(&sandbox, &agent, None).unwrap();

    assert_eq!(agent.calls.load(Ordering::SeqCst), 2);
    assert!(
        sandbox
            .git(
                &result.worktree,
                &["merge-base", "--is-ancestor", &result.base_commit, "HEAD"]
            )
            .is_empty()
    );
    assert_eq!(
        fs::read_to_string(result.worktree.join("shared.txt")).unwrap(),
        "both edits preserved\n"
    );
    assert!(result.worktree.join("upstream.txt").exists());
}
