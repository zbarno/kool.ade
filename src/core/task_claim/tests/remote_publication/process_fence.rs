use super::*;
use std::{
    fs,
    process::{Child, Command},
    thread,
    time::{Duration, Instant},
};

const WORKER_MODE: &str = "KOOLADE_CLAIM_PROCESS_WORKER";

#[test]
fn stale_worker_process_cannot_publish_after_takeover() {
    if std::env::var_os(WORKER_MODE).is_some() {
        run_stale_worker_process();
        return;
    }

    let fixture = Fixture::new();
    let stale_repo = &fixture.clones[0];
    let takeover_repo = &fixture.clones[1];
    let base = git_at(stale_repo, &["rev-parse", "HEAD"]);
    fs::write(stale_repo.join("preserved.txt"), "stale worker evidence\n").unwrap();
    git_at(stale_repo, &["add", "preserved.txt"]);
    git_at(stale_repo, &["commit", "-qm", "prepare stale candidate"]);
    let candidate = git_at(stale_repo, &["rev-parse", "HEAD"]);
    let signal_dir = fixture.root.join("worker-signals");
    fs::create_dir(&signal_dir).unwrap();
    let task_uid = "task-two-process-fence";
    let test_name = std::thread::current().name().unwrap().to_owned();
    let mut worker = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", &test_name, "--nocapture"])
        .env(WORKER_MODE, "owner")
        .env("KOOLADE_CLAIM_REPO", stale_repo)
        .env("KOOLADE_CLAIM_BASE", &base)
        .env("KOOLADE_CLAIM_CANDIDATE", &candidate)
        .env("KOOLADE_CLAIM_TASK", task_uid)
        .env("KOOLADE_CLAIM_SIGNAL_DIR", &signal_dir)
        .spawn()
        .unwrap();

    let ready = signal_dir.join("ready");
    if wait_for_file(&ready, &mut worker, Duration::from_secs(10)).is_none() {
        let _ = worker.kill();
        panic!("stale worker did not acquire its task claim");
    }
    let session = fs::read_to_string(&ready).unwrap();
    let reference = reference(task_uid).unwrap();
    let old_object = git_at(&fixture.remote, &["rev-parse", &reference]);
    let mut stale_record: ClaimRecord = serde_json::from_str(&git_at(
        stale_repo,
        &["show", "-s", "--format=%B", &old_object],
    ))
    .unwrap();
    stale_record.claimed_at = 1;
    let stale_object =
        crate::core::task_claim::git::create_object(takeover_repo, &stale_record).unwrap();
    git_at(
        takeover_repo,
        &[
            "push",
            "-q",
            "--force",
            "origin",
            &format!("{stale_object}:{reference}"),
        ],
    );
    let takeover = ClaimLease::take_over_stale(takeover_repo, task_uid, &session, &base).unwrap();
    let takeover_record = takeover.record().unwrap();
    assert_eq!(takeover_record.takeover_history.len(), 1);
    assert_eq!(takeover_record.takeover_history[0].session_id, session);
    fs::write(signal_dir.join("continue"), "publish\n").unwrap();

    let output = worker.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(signal_dir.join("result")).unwrap(),
        "rejected"
    );
    assert!(fs::read_to_string(stale_repo.join("preserved.txt")).is_ok());
    assert_ne!(
        git_at(&fixture.remote, &["rev-parse", &reference]),
        old_object
    );
    assert!(
        !Command::new("git")
            .args([
                "show-ref",
                "--verify",
                "--quiet",
                "refs/heads/koolade/two-process-candidate"
            ])
            .current_dir(&fixture.remote)
            .status()
            .unwrap()
            .success()
    );
    assert_eq!(
        takeover.record().unwrap().session_id,
        takeover_record.session_id
    );
}

fn run_stale_worker_process() {
    let repo = std::path::PathBuf::from(std::env::var_os("KOOLADE_CLAIM_REPO").unwrap());
    let base = std::env::var("KOOLADE_CLAIM_BASE").unwrap();
    let candidate = std::env::var("KOOLADE_CLAIM_CANDIDATE").unwrap();
    let task_uid = std::env::var("KOOLADE_CLAIM_TASK").unwrap();
    let signal_dir =
        std::path::PathBuf::from(std::env::var_os("KOOLADE_CLAIM_SIGNAL_DIR").unwrap());
    let mut lease = ClaimLease::acquire(&repo, &task_uid, &base)
        .unwrap()
        .unwrap();
    fs::write(signal_dir.join("ready"), lease.record().unwrap().session_id).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !signal_dir.join("continue").exists() {
        assert!(
            Instant::now() < deadline,
            "parent did not release stale worker"
        );
        thread::sleep(Duration::from_millis(10));
    }
    let result = lease.fenced_push(
        &repo,
        &candidate,
        "refs/heads/koolade/two-process-candidate",
    );
    fs::write(
        signal_dir.join("result"),
        if result.is_err() {
            "rejected"
        } else {
            "published"
        },
    )
    .unwrap();
}

fn wait_for_file(path: &std::path::Path, child: &mut Child, timeout: Duration) -> Option<()> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if path.is_file() {
            return Some(());
        }
        if child.try_wait().ok().flatten().is_some() {
            return None;
        }
        thread::sleep(Duration::from_millis(10));
    }
    None
}
