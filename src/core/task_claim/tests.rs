use super::*;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, Barrier},
};

#[path = "tests/remote_publication.rs"]
mod remote_publication;

struct Fixture {
    root: PathBuf,
    clones: [PathBuf; 2],
    remote: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("koolade-claims-{}", uuid::Uuid::new_v4()));
        let remote = root.join("remote.git");
        let seed = root.join("seed");
        let clones = [root.join("alice"), root.join("bob")];
        fs::create_dir_all(&root).unwrap();
        git_at(&root, &["init", "-q", "--bare", remote.to_str().unwrap()]);
        fs::create_dir(&seed).unwrap();
        git_at(&seed, &["init", "-q"]);
        git_at(&seed, &["config", "user.name", "Fixture User"]);
        git_at(&seed, &["config", "user.email", "fixture@noreply.example"]);
        fs::write(seed.join("README.md"), "fixture\n").unwrap();
        git_at(&seed, &["add", "README.md"]);
        git_at(&seed, &["commit", "-qm", "fixture"]);
        git_at(&seed, &["branch", "-M", "main"]);
        git_at(
            &seed,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        );
        git_at(&seed, &["push", "-q", "-u", "origin", "main"]);
        git_at(&remote, &["symbolic-ref", "HEAD", "refs/heads/main"]);
        for (index, clone) in clones.iter().enumerate() {
            git_at(
                &root,
                &[
                    "clone",
                    "-q",
                    remote.to_str().unwrap(),
                    clone.to_str().unwrap(),
                ],
            );
            git_at(
                clone,
                &[
                    "config",
                    "user.name",
                    if index == 0 { "Alice" } else { "Bob" },
                ],
            );
            git_at(clone, &["config", "user.email", "fixture@noreply.example"]);
        }
        Self {
            root,
            clones,
            remote,
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn git_at(cwd: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

#[test]
fn independent_clones_race_for_one_atomic_task_claim() {
    let fixture = Fixture::new();
    let barrier = Arc::new(Barrier::new(3));
    let threads = fixture
        .clones
        .clone()
        .into_iter()
        .map(|repo| {
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                let head = git_at(&repo, &["rev-parse", "HEAD"]);
                ClaimLease::acquire(&repo, "task-123", &head)
            })
        })
        .collect::<Vec<_>>();
    barrier.wait();
    let results = threads
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(result, Ok(Some(_))))
            .count(),
        1
    );
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(result, Err(ClaimError::AlreadyClaimed { .. })))
            .count(),
        1
    );
    let winner_index = results
        .iter()
        .position(|result| matches!(result, Ok(Some(_))))
        .unwrap();
    let Some(lease) = results.into_iter().nth(winner_index).unwrap().unwrap() else {
        unreachable!()
    };
    let record = lease.record().unwrap();
    assert_eq!(record.task_uid, "task-123");
    assert!(record.owner.contains("Kool.ad/e"));
    assert!(!record.session_id.is_empty());
    assert_eq!(
        record.base_commit,
        git_at(&fixture.clones[winner_index], &["rev-parse", "HEAD"])
    );
    drop(lease);
    assert!(
        git_at(
            &fixture.remote,
            &[
                "for-each-ref",
                "--format=%(refname)",
                "refs/heads/koolade/claims"
            ]
        )
        .is_empty()
    );
}

#[test]
fn stale_claim_takeover_requires_the_observed_session_and_uses_cas() {
    let fixture = Fixture::new();
    let repo = &fixture.clones[0];
    let reference = reference("task-456").unwrap();
    let stale = ClaimRecord {
        schema_version: 1,
        task_uid: "task-456".into(),
        owner: "Kool.ad/e session".into(),
        session_id: "stale-session".into(),
        base_commit: git_at(repo, &["rev-parse", "HEAD"]),
        claimed_at: 1,
        takeover_history: Box::default(),
    };
    let object = git::create_object(repo, &stale).unwrap();
    git_at(
        repo,
        &["push", "-q", "origin", &format!("{object}:{reference}")],
    );
    let recovered =
        ClaimLease::take_over_stale(repo, "task-456", "stale-session", "new-base").unwrap();
    let current = recovered.record().unwrap();
    assert_ne!(current.session_id, "stale-session");
    assert_eq!(current.base_commit, "new-base");
    assert_eq!(current.schema_version, 2);
    assert_eq!(current.takeover_history.len(), 1);
    assert_eq!(current.takeover_history[0].owner, stale.owner);
    assert_eq!(current.takeover_history[0].session_id, "stale-session");
    assert_eq!(current.takeover_history[0].base_commit, stale.base_commit);
    assert_eq!(current.takeover_history[0].claimed_at, 1);
    assert!(matches!(
        ClaimLease::take_over_stale(repo, "task-456", "stale-session", "other"),
        Err(ClaimError::AlreadyClaimed { .. })
    ));
}

#[test]
fn stale_claim_clock_and_ref_identity_are_checked() {
    let record = ClaimRecord {
        schema_version: 1,
        task_uid: String::new(),
        owner: String::new(),
        session_id: String::new(),
        base_commit: String::new(),
        claimed_at: 1_000,
        takeover_history: Box::default(),
    };
    assert!(!record.appears_stale(1_000 + stale_after_seconds() - 1));
    assert!(record.appears_stale(1_000 + stale_after_seconds()));
    assert!(reference("../other").is_err());
}

#[test]
fn repositories_without_an_origin_keep_the_local_only_path_available() {
    let fixture = Fixture::new();
    let repo = &fixture.clones[0];
    git_at(repo, &["remote", "remove", "origin"]);
    let base = git_at(repo, &["rev-parse", "HEAD"]);
    assert!(
        ClaimLease::acquire(repo, "task-offline", &base)
            .unwrap()
            .is_none()
    );
}

#[test]
fn unavailable_remote_claim_requires_explicit_per_run_override() {
    let fixture = Fixture::new();
    let repo = &fixture.clones[0];
    let missing = fixture.root.join("nonexistent-remote.git");
    git_at(
        repo,
        &["remote", "set-url", "origin", missing.to_str().unwrap()],
    );
    let base = git_at(repo, &["rev-parse", "HEAD"]);
    let claim = || {
        ClaimRequest::new(
            repo.clone(),
            "task-no-remote".into(),
            base.clone(),
            None,
            false,
        )
    };
    assert!(matches!(
        claim().acquire(),
        Err(ClaimError::RemoteUnavailable(_))
    ));
    let override_run = ClaimRequest::new(
        repo.clone(),
        "task-no-remote".into(),
        base.clone(),
        None,
        true,
    )
    .acquire()
    .unwrap();
    assert!(override_run.lease.is_none());
    assert!(
        override_run
            .warning
            .as_deref()
            .is_some_and(|text| text.contains("local clone lock only"))
    );
    // The override belonged to only that ClaimRequest. Normal new attempts
    // still require the shared remote claim.
    assert!(matches!(
        claim().acquire(),
        Err(ClaimError::RemoteUnavailable(_))
    ));
}
