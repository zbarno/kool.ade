use super::*;
use std::{
    fs,
    process::{Child, Command},
    thread,
    time::{Duration, Instant},
};

const WORKER_MODE: &str = "KOOLADE_CLAIM_PROCESS_WORKER";
const RACE_WORKER_MODE: &str = "KOOLADE_CLAIM_RACE_WORKER";

#[test]
fn independent_clone_processes_race_for_global_claim() {
    if std::env::var_os(RACE_WORKER_MODE).is_some() {
        run_race_worker_process();
        return;
    }

    let fixture = Fixture::new();
    let signals = fixture.root.join("race-signals");
    fs::create_dir(&signals).unwrap();
    let task_uid = "task-two-process-race";
    let base = git_at(&fixture.code_clones[0], &["rev-parse", "HEAD"]);
    let test_name = std::thread::current().name().unwrap().to_owned();
    let mut workers = fixture
        .clones
        .iter()
        .enumerate()
        .map(|(index, repo)| {
            Command::new(std::env::current_exe().unwrap())
                .args(["--exact", &test_name, "--nocapture"])
                .env(RACE_WORKER_MODE, "worker")
                .env("KOOLADE_CLAIM_REPO", repo)
                .env("KOOLADE_CLAIM_BASE", &base)
                .env("KOOLADE_CLAIM_TASK", task_uid)
                .env("KOOLADE_CLAIM_RACE_WORKER_INDEX", index.to_string())
                .env(
                    "KOOLADE_CLAIM_PROJECT",
                    fixture.project_uid.hyphenated().to_string(),
                )
                .env("KOOLADE_CLAIM_SIGNAL_DIR", &signals)
                .spawn()
                .unwrap()
        })
        .collect::<Vec<_>>();
    fs::write(signals.join("start"), "go\n").unwrap();
    for (index, worker) in workers.iter_mut().enumerate() {
        let result = signals.join(format!("result-{index}"));
        assert!(
            wait_for_file(&result, worker, Duration::from_secs(10)).is_some(),
            "claim race worker {index} exited before reporting"
        );
    }
    let results = (0..workers.len())
        .map(|index| fs::read_to_string(signals.join(format!("result-{index}"))).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        results
            .iter()
            .filter(|result| result.as_str() == "won")
            .count(),
        1
    );
    assert_eq!(
        results
            .iter()
            .filter(|result| result.as_str() == "already-claimed")
            .count(),
        1
    );
    fs::write(signals.join("finish"), "release\n").unwrap();
    for worker in workers {
        let output = worker.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn stale_worker_process_cannot_publish_after_takeover() {
    if std::env::var_os(WORKER_MODE).is_some() {
        run_stale_worker_process();
        return;
    }

    let fixture = Fixture::new();
    let stale_repo = &fixture.clones[0];
    let takeover_repo = &fixture.clones[1];
    let code_repo = &fixture.code_clones[0];
    let base = git_at(code_repo, &["rev-parse", "HEAD"]);
    fs::write(code_repo.join("preserved.txt"), "stale worker evidence\n").unwrap();
    git_at(code_repo, &["add", "preserved.txt"]);
    git_at(code_repo, &["commit", "-qm", "prepare stale candidate"]);
    let candidate = git_at(code_repo, &["rev-parse", "HEAD"]);
    let signal_dir = fixture.root.join("worker-signals");
    fs::create_dir(&signal_dir).unwrap();
    let task_uid = "task-two-process-fence";
    let test_name = std::thread::current().name().unwrap().to_owned();
    let mut worker = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", &test_name, "--nocapture"])
        .env(WORKER_MODE, "owner")
        .env("KOOLADE_CLAIM_REPO", stale_repo)
        .env("KOOLADE_CLAIM_CODE_REPO", code_repo)
        .env("KOOLADE_CLAIM_BASE", &base)
        .env("KOOLADE_CLAIM_CANDIDATE", &candidate)
        .env("KOOLADE_CLAIM_TASK", task_uid)
        .env(
            "KOOLADE_CLAIM_PROJECT",
            fixture.project_uid.hyphenated().to_string(),
        )
        .env("KOOLADE_CLAIM_SIGNAL_DIR", &signal_dir)
        .spawn()
        .unwrap();

    let ready = signal_dir.join("ready");
    if wait_for_file(&ready, &mut worker, Duration::from_secs(10)).is_none() {
        let _ = worker.kill();
        panic!("stale worker did not acquire its task claim");
    }
    let session = fs::read_to_string(&ready).unwrap();
    let reference = reference(fixture.project_uid, task_uid).unwrap();
    let old_object = git_at(&fixture.planning_remote, &["rev-parse", &reference]);
    let mut stale_record: ClaimRecord = serde_json::from_str(&git_at(
        stale_repo,
        &["show", "-s", "--format=%B", &old_object],
    ))
    .unwrap();
    stale_record.issued_at_utc = 1;
    stale_record.expires_at_utc = 1;
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
    let takeover = ClaimLease::take_over_stale(
        takeover_repo,
        fixture.project_uid,
        task_uid,
        "web",
        &session,
        &base,
    )
    .unwrap();
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
    assert!(fs::read_to_string(code_repo.join("preserved.txt")).is_ok());
    assert_ne!(
        git_at(&fixture.planning_remote, &["rev-parse", &reference]),
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
    let code_repo = std::path::PathBuf::from(std::env::var_os("KOOLADE_CLAIM_CODE_REPO").unwrap());
    let base = std::env::var("KOOLADE_CLAIM_BASE").unwrap();
    let candidate = std::env::var("KOOLADE_CLAIM_CANDIDATE").unwrap();
    let task_uid = std::env::var("KOOLADE_CLAIM_TASK").unwrap();
    let project_uid = std::env::var("KOOLADE_CLAIM_PROJECT")
        .unwrap()
        .parse::<uuid::Uuid>()
        .unwrap();
    let signal_dir =
        std::path::PathBuf::from(std::env::var_os("KOOLADE_CLAIM_SIGNAL_DIR").unwrap());
    let mut lease = ClaimLease::acquire(&repo, project_uid, &task_uid, "web", &base)
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
        &code_repo,
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

fn run_race_worker_process() {
    let repo = std::path::PathBuf::from(std::env::var_os("KOOLADE_CLAIM_REPO").unwrap());
    let base = std::env::var("KOOLADE_CLAIM_BASE").unwrap();
    let task_uid = std::env::var("KOOLADE_CLAIM_TASK").unwrap();
    let project_uid = std::env::var("KOOLADE_CLAIM_PROJECT")
        .unwrap()
        .parse::<uuid::Uuid>()
        .unwrap();
    let signal_dir =
        std::path::PathBuf::from(std::env::var_os("KOOLADE_CLAIM_SIGNAL_DIR").unwrap());
    let index = std::env::var("KOOLADE_CLAIM_RACE_WORKER_INDEX").unwrap();
    while !signal_dir.join("start").exists() {
        thread::sleep(Duration::from_millis(10));
    }
    let result = match ClaimLease::acquire(&repo, project_uid, &task_uid, "web", &base) {
        Ok(Some(lease)) => {
            fs::write(signal_dir.join(format!("result-{index}")), "won").unwrap();
            while !signal_dir.join("finish").exists() {
                thread::sleep(Duration::from_millis(10));
            }
            drop(lease);
            Ok(())
        }
        Err(ClaimError::AlreadyClaimed { .. }) => {
            fs::write(
                signal_dir.join(format!("result-{index}")),
                "already-claimed",
            )
            .unwrap();
            Ok(())
        }
        Err(error) => {
            fs::write(
                signal_dir.join(format!("result-{index}")),
                format!("error: {error}"),
            )
            .unwrap();
            Err(error)
        }
        Ok(None) => unreachable!("fixture planning remote must be configured"),
    };
    result.unwrap();
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
