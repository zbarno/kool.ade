use super::*;

// Production boundary: after publishing to a real Git remote, the application
// reads provider-owned PR metadata before persisting lifecycle Opened. This
// fake provider returns a valid but stale PR head to prove it cannot borrow
// the task's review receipt. Synthetic local Git/provider only; one task clone
// and a few subprocesses, moderate setup, no external network or Docker.
#[test]
fn stale_provider_pull_request_head_is_not_recorded_as_opened() {
    let sandbox = Sandbox::new();
    let mut state = sandbox
        .run_with_publication_policy(
            "complete",
            Arc::new(AtomicUsize::new(0)),
            PublicationMode::HoldForReview,
            None,
            false,
        )
        .unwrap();
    let expected_head = state.verified_head.clone().unwrap();
    state.pr_url = None;
    state.pr_details = None;
    state.pr_state = None;
    fs::write(
        &sandbox.gh,
        r##"#!/bin/sh
root=$(dirname "$0")
if [ "$2" = list ]; then
  echo '[{"url":"https://github.com/fixture/repo/pull/1","state":"OPEN"}]'
elif [ "$2" = view ]; then
  base=$(git --git-dir "$root/remote.git" rev-parse refs/heads/main)
  state=OPEN
  head=dddddddddddddddddddddddddddddddddddddddd
  if [ -f "$root/closed" ]; then
    state=CLOSED
    branch=$(cat "$root/closed")
    head=$(git --git-dir "$root/remote.git" rev-parse "refs/heads/$branch")
  fi
  printf '{"url":"https://github.com/fixture/repo/pull/1","number":1,"state":"%s","headRefOid":"%s","baseRefOid":"%s","baseRefName":"main"}\n' "$state" "$head" "$base"
else
  exit 2
fi
"##,
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&sandbox.gh, fs::Permissions::from_mode(0o700)).unwrap();
    }
    let (progress, _updates) = mpsc::channel();
    let runner = Runner {
        gh: sandbox.gh.to_string_lossy().into_owned(),
        runtime_config_source: None,
        deadline: Instant::now() + Duration::from_secs(60),
        cancel: Arc::new(AtomicBool::new(false)),
        progress,
        authorization_verifier: None,
    };

    let error = publication::create_pull_request(
        &state_dir(&sandbox.repo, &sandbox.ticket).unwrap(),
        &mut state,
        &runner,
        None,
    )
    .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("differs from the reviewed implementation head")
    );
    assert_eq!(state.pr_url, None);
    assert_eq!(state.pr_details, None);
    assert_eq!(state.pr_state, None);
    assert_eq!(state.verified_head.as_deref(), Some(expected_head.as_str()));
    assert_eq!(
        sandbox.git(
            &sandbox.root.join("remote.git"),
            &["rev-parse", &format!("refs/heads/{}", state.branch)],
        ),
        expected_head
    );

    fs::write(sandbox.root.join("closed"), &state.branch).unwrap();
    publication::create_pull_request(
        &state_dir(&sandbox.repo, &sandbox.ticket).unwrap(),
        &mut state,
        &runner,
        None,
    )
    .unwrap();

    assert_eq!(
        state.pr_url.as_deref(),
        Some("https://github.com/fixture/repo/pull/1")
    );
    assert_eq!(
        state
            .pr_details
            .as_ref()
            .map(|details| details.head_sha.as_str()),
        Some(expected_head.as_str())
    );
    assert_eq!(state.pr_state, Some(PullRequestState::Closed));
    assert_eq!(state.status, ImplementationStatus::PullRequestClosed);
    assert_eq!(state.verified_head.as_deref(), Some(expected_head.as_str()));
}
