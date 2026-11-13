#[path = "auto_worker/task_fixture.rs"]
mod task_fixture;
#[path = "auto_worker/worker.rs"]
mod worker;
use std::sync::{Arc, Mutex, mpsc};
use worker::LocalImplementationWorker;

/// Production boundary: a typed Ready task flows through the durable Auto
/// queue, ImplementationController, isolated local task repository, persisted
/// completion event, and the next self-improvement projection. Projection
/// tests cannot prove an actual worker result advances the app stage. Uses a
/// synthetic local AiHarness, a verifier scoped to this temporary task clone,
/// and temporary Git/private stores; no provider, Docker, SSH, or network;
/// moderate app-and-clone setup.
pub(super) fn dispatch_and_verify(
    app: &mut crate::app::root::KooladeApp,
    lifecycle: &crate::core::lifecycle::LifecycleStore<'_>,
    runs: &crate::core::self_improvement::SelfImprovementStore<'_>,
    run_uid: &str,
    task_uid: &str,
    feature_id: &str,
) {
    use crate::core::self_improvement::SelfImprovementPhaseV1 as Phase;

    let run = runs.load(run_uid).unwrap();
    assert_eq!(run.phase, Phase::Ready);
    let task = lifecycle.load_task(task_uid).unwrap();
    assert_eq!(task.phase, crate::core::lifecycle::TaskPhase::Ready);
    let path = match &mut app.screen {
        crate::app::root::Screen::Connected(project) => {
            task_fixture::persist_task_document(project, &run, task_uid, feature_id)
        }
        crate::app::root::Screen::Welcome => panic!("fixture should remain connected"),
    };
    let (attempt_tx, attempt_rx) = mpsc::channel();
    let (review_started_tx, review_started_rx) = mpsc::channel();
    let (review_release_tx, review_release_rx) = mpsc::channel();
    if let crate::app::root::Screen::Connected(project) = &mut app.screen {
        let output = std::process::Command::new("git")
            .args([
                "-C",
                project.state.repo_root.to_str().unwrap(),
                "config",
                "remote.origin.pushurl",
                "https://github.com/zbarno/kool.ade",
            ])
            .output()
            .unwrap();
        assert!(output.status.success());
        let source_sha = std::process::Command::new("git")
            .args([
                "-C",
                project.state.repo_root.to_str().unwrap(),
                "rev-parse",
                "HEAD",
            ])
            .output()
            .unwrap();
        assert!(source_sha.status.success());
        let source_sha = String::from_utf8(source_sha.stdout)
            .unwrap()
            .trim()
            .to_owned();
        let output = std::process::Command::new("git")
            .args([
                "-C",
                project.state.repo_root.to_str().unwrap(),
                "update-ref",
                "refs/remotes/origin/main",
                &source_sha,
            ])
            .output()
            .unwrap();
        assert!(output.status.success());
        let output = std::process::Command::new("git")
            .args([
                "-C",
                project.state.repo_root.to_str().unwrap(),
                "symbolic-ref",
                "refs/remotes/origin/HEAD",
                "refs/remotes/origin/main",
            ])
            .output()
            .unwrap();
        assert!(output.status.success());
        project.implementation_states.clear();
        project.queue.auto_build = true;
        project.queue.max_parallel = 1;
        project
            .queue
            .bind_task_documents(&project.task_documents)
            .unwrap();
        project
            .queue
            .save(&project.state.planning_store.root)
            .unwrap();
        app.task_harness = Some(Box::new(LocalImplementationWorker {
            implementation: attempt_tx,
            review_started: review_started_tx,
            review_release: Arc::new(Mutex::new(review_release_rx)),
        }));
    } else {
        panic!("fixture should remain connected");
    }
    let _runtime = crate::harness::runtime_capabilities::RuntimeCapabilities::override_for_test(
        crate::harness::runtime_capabilities::RuntimeCapabilities {
            planning_access:
                crate::harness::runtime_capabilities::PlanningAccess::SandboxedRepositoryReads,
            implementation: true,
        },
    );
    let _verification = crate::core::implementation::install_scoped_test_verification_executor(
        std::env::temp_dir(),
        task_uid.to_owned(),
        |cwd, command, _workspace| {
            anyhow::ensure!(
                command == "test -f lifecycle-worker-output.txt",
                "unexpected verification command: {command}"
            );
            anyhow::ensure!(
                cwd.join("lifecycle-worker-output.txt").is_file(),
                "local worker output is missing from the isolated task repository"
            );
            Ok("local worker output is present".into())
        },
    );
    let context = egui::Context::default();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(180);
    let mut worker_observation = None;
    let mut reviewer_waiting = false;
    let mut review_stage_observed = false;
    let (reported_uid, task_repo) = loop {
        app.tick(0.0, &context);
        if worker_observation.is_none() {
            worker_observation = attempt_rx.try_recv().ok();
        }
        reviewer_waiting |= review_started_rx.try_recv().is_ok();
        let lifecycle_task = lifecycle.load_task(task_uid).unwrap();
        let projected = runs.load(run_uid).unwrap();
        if !review_stage_observed
            && reviewer_waiting
            && projected.phase == Phase::Reviewing
            && lifecycle_task.phase == crate::core::lifecycle::TaskPhase::AwaitingReview
            && lifecycle_task.implementation_sha256.is_some()
            && lifecycle_task.code_review.is_none()
            && projected.code_review_uids.is_empty()
            && projected.runtime_qa_uids.is_empty()
        {
            review_stage_observed = true;
            review_release_tx
                .send(())
                .expect("the independent reviewer should be waiting at the stage boundary");
        }
        let completed = match &app.screen {
            crate::app::root::Screen::Connected(project) => project
                .implementation_states
                .values()
                .find(|state| state.task_uid.as_deref() == Some(task_uid))
                .is_some_and(|state| {
                    review_stage_observed
                        && lifecycle_task.phase
                            == crate::core::lifecycle::TaskPhase::AwaitingRuntimeQa
                        && projected.phase == Phase::Qa
                        && state.status
                            == crate::core::implementation::ImplementationStatus::AwaitingApproval
                }),
            crate::app::root::Screen::Welcome => false,
        };
        if completed {
            break worker_observation.unwrap_or_else(|| {
                attempt_rx
                    .recv_timeout(std::time::Duration::from_secs(1))
                    .expect("worker invocation should be recorded")
            });
        }
        if std::time::Instant::now() >= deadline {
            let crate::app::root::Screen::Connected(project) = &app.screen else {
                panic!("the app disconnected before the worker completed");
            };
            panic!(
                "approved task did not complete: invoked={worker_observation:?}, reviewer_waiting={reviewer_waiting}, review_stage_observed={review_stage_observed}, lifecycle_phase={:?}, run_phase={:?}, run_target=({:?},{:?},{:?},{:?}), implementation={:?}, queue_running={}, queue_error={}, blocked={:?}, active={:?}, states={:?}, pending={:?}",
                lifecycle_task.phase,
                projected.phase,
                projected.target_repository_uid,
                projected.source_sha,
                projected.source_ref,
                projected.destination_ref,
                project
                    .implementation_states
                    .values()
                    .map(|state| (
                        state.task_uid.as_deref(),
                        state.status,
                        state.task_repository_kind,
                        state.task_repository_ready,
                        state.verified_head.as_deref(),
                        state.task_repositories.contains(&state.task_repository),
                        state.repository_id.as_deref(),
                        state.repository_identity.as_deref(),
                        state.source_commit.as_deref(),
                        state.source_branch.as_deref(),
                        state.destination_branch.as_deref()
                    ))
                    .collect::<Vec<_>>(),
                project.queue.running,
                project.queue.last_error,
                project.queue.blocked,
                project.active_implementations.keys().collect::<Vec<_>>(),
                project
                    .implementation_states
                    .iter()
                    .map(|(ticket, state)| (ticket, state.status, &state.detail))
                    .collect::<Vec<_>>(),
                project.activity.pending,
            );
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    };
    assert!(
        review_stage_observed,
        "Reviewing was not observable before the independent review receipt"
    );
    assert_eq!(reported_uid, task_uid);
    assert_eq!(
        std::fs::read_to_string(task_repo.join("lifecycle-worker-output.txt")).unwrap(),
        "worker completed the approved task\n"
    );
    let completed_task = lifecycle.load_task(task_uid).unwrap();
    assert!(completed_task.implementation_sha256.is_some());
    assert_eq!(
        completed_task.phase,
        crate::core::lifecycle::TaskPhase::AwaitingRuntimeQa
    );
    assert!(completed_task.code_review.is_some());
    let crate::app::root::Screen::Connected(project) = &app.screen else {
        panic!("fixture should remain connected");
    };
    let implementation = project
        .implementation_states
        .values()
        .find(|state| state.task_uid.as_deref() == Some(task_uid))
        .expect("the production controller should persist its implementation record");
    assert!(implementation.verified_head.is_some());
    assert!(!project.queue.blocked.contains_key(&path));
    let projected = runs.load(run_uid).unwrap();
    assert_eq!(projected.phase, Phase::Qa);
    assert_eq!(projected.code_review_uids.len(), 1);
    assert!(projected.runtime_qa_uids.is_empty());
}
