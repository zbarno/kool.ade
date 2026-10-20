use super::*;

// Production boundary: the application publisher pushes a real task clone to a
// local Git remote and invokes a provider executable whose create response is
// lost after accepting the request. Unit deduplication checks cannot prove that
// a second invocation reconciles before creating another PR. Synthetic Git/SSH
// and provider fixtures only; one verified fixture task, moderate setup cost.
#[test]
fn provider_timeout_after_creation_reconciles_without_another_create() {
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
    let dir = state_dir(&sandbox.repo, &sandbox.ticket).unwrap();
    fs::write(dir.join("pr-body.md"), "Scoped reviewed publication body").unwrap();
    fs::write(&sandbox.gh, r##"#!/bin/sh
root=$(dirname "$0")
if [ "$2" = list ]; then
  if [ -f "$root/pr-created" ]; then
    head=$(git rev-parse HEAD)
    printf '[{"url":"https://github.com/fixture/repo/pull/1","number":1,"state":"OPEN","headRefOid":"%s"}]\n' "$head"
  else
    echo '[]'
  fi
elif [ "$2" = view ]; then
  head=$(git --git-dir "$root/remote.git" for-each-ref --format='%(objectname)' refs/heads/koolade | head -n 1)
  base=$(git --git-dir "$root/remote.git" rev-parse refs/heads/main)
  printf '{"url":"https://github.com/fixture/repo/pull/1","number":1,"state":"OPEN","headRefOid":"%s","baseRefOid":"%s","baseRefName":"main"}\n' "$head" "$base"
elif [ "$2" = create ]; then
  while [ "$#" -gt 0 ]; do
    if [ "$1" = --body-file ]; then shift; cat "$1" > "$root/uploaded-body"; fi
    shift
  done
  echo created >> "$root/pr-created"
  echo 'provider response timed out after accepting creation' >&2
  exit 1
else
  exit 2
fi
"##).unwrap();
    let (progress, _updates) = mpsc::channel();
    let runner = Runner {
        gh: sandbox.gh.to_string_lossy().into_owned(),
        runtime_config_source: None,
        deadline: Instant::now() + Duration::from_secs(60),
        cancel: Arc::new(AtomicBool::new(false)),
        progress,
        authorization_verifier: None,
    };
    assert!(publication::create_pull_request(&dir, &mut state, &runner, None).is_err());
    assert_eq!(
        fs::read_to_string(sandbox.root.join("uploaded-body")).unwrap(),
        "Scoped reviewed publication body"
    );
    assert!(state.pr_url.is_none());
    assert!(state.task_repository.join("implemented.txt").exists());
    publication::create_pull_request(&dir, &mut state, &runner, None).unwrap();
    assert_eq!(
        state.pr_url.as_deref(),
        Some("https://github.com/fixture/repo/pull/1")
    );
    assert_eq!(
        state.pr_details.as_ref().unwrap().head_sha,
        state.verified_head.as_deref().unwrap()
    );
    assert_eq!(
        fs::read_to_string(sandbox.root.join("pr-created"))
            .unwrap()
            .lines()
            .count(),
        1
    );
}
