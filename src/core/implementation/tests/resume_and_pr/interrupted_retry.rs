//! Exercise explicit retry at the persisted implementation-state boundary, including a real task
//! clone. Unit tests cannot prove the retry cap preserves workspace contents or prevents worker
//! replay; the fixture uses local Git only, with no Docker/network dependency and moderate setup.
use super::*;

#[test]
fn interrupted_task_resume_is_bounded_and_preserves_workspace_without_replay() {
    let s = Sandbox::new();
    let calls = Arc::new(AtomicUsize::new(0));
    assert!(s.run("cancel", calls.clone()).is_err());
    let interrupted = load(&s.repo, &s.ticket).unwrap();
    let preserved_workspace = interrupted.task_repository.clone();
    let evidence = preserved_workspace.join("implemented.txt");
    assert_eq!(fs::read_to_string(&evidence).unwrap(), "partial\n");
    let auto_retry =
        crate::core::implementation::mark_resume_started_for_action(&s.repo, &s.ticket, false)
            .unwrap_err();
    assert!(
        auto_retry
            .to_string()
            .contains("Automatic queue retry is disabled")
    );
    assert_eq!(
        load(&s.repo, &s.ticket).unwrap().status,
        ImplementationStatus::Interrupted
    );

    for _ in 0..3 {
        let resumed = crate::core::implementation::mark_resume_started(&s.repo, &s.ticket)
            .unwrap()
            .unwrap();
        assert_eq!(resumed.status, ImplementationStatus::Preparing);
        assert_eq!(resumed.task_repository, preserved_workspace);
        let mut interrupted_again = resumed;
        interrupted_again.status = ImplementationStatus::Blocked;
        save(&state_dir(&s.repo, &s.ticket).unwrap(), &interrupted_again).unwrap();
    }

    let mut final_interruption = load(&s.repo, &s.ticket).unwrap();
    final_interruption.status = ImplementationStatus::Interrupted;
    save(&state_dir(&s.repo, &s.ticket).unwrap(), &final_interruption).unwrap();

    let error = crate::core::implementation::mark_resume_started(&s.repo, &s.ticket).unwrap_err();
    assert!(error.to_string().contains("retry limit (3) reached"));
    assert_eq!(
        load(&s.repo, &s.ticket).unwrap().status,
        ImplementationStatus::Interrupted
    );
    assert_eq!(fs::read_to_string(evidence).unwrap(), "partial\n");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn concurrent_explicit_resumes_consume_one_budget_entry_without_replay() {
    let s = Sandbox::new();
    let calls = Arc::new(AtomicUsize::new(0));
    assert!(s.run("cancel", calls.clone()).is_err());
    let barrier = Arc::new(std::sync::Barrier::new(3));
    let workers = (0..2)
        .map(|_| {
            let barrier = barrier.clone();
            let repo = s.repo.clone();
            let ticket = s.ticket.clone();
            std::thread::spawn(move || {
                barrier.wait();
                crate::core::implementation::mark_resume_started(&repo, &ticket).unwrap()
            })
        })
        .collect::<Vec<_>>();
    barrier.wait();

    for worker in workers {
        assert_eq!(
            worker.join().unwrap().unwrap().status,
            ImplementationStatus::Preparing
        );
    }
    let counter = state_dir(&s.repo, &s.ticket)
        .unwrap()
        .join("interrupted-resumes.json");
    assert_eq!(
        serde_json::from_slice::<u32>(&fs::read(counter).unwrap()).unwrap(),
        1
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[cfg(unix)]
#[test]
fn interrupted_resume_rejects_symlink_and_fifo_retry_state_without_blocking() {
    use std::os::unix::ffi::OsStrExt;

    let s = Sandbox::new();
    assert!(s.run("cancel", Arc::new(AtomicUsize::new(0))).is_err());
    let dir = state_dir(&s.repo, &s.ticket).unwrap();
    let counter = dir.join("interrupted-resumes.json");
    let target = dir.join("counter-target");
    fs::write(&target, b"0").unwrap();
    std::os::unix::fs::symlink(&target, &counter).unwrap();
    assert!(crate::core::implementation::mark_resume_started(&s.repo, &s.ticket).is_err());
    fs::remove_file(&counter).unwrap();

    let fifo = std::ffi::CString::new(counter.as_os_str().as_bytes()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
    assert!(crate::core::implementation::mark_resume_started(&s.repo, &s.ticket).is_err());
    fs::remove_file(&counter).unwrap();

    let lock = dir.join("interrupted-resume.lock");
    if lock.exists() {
        fs::remove_file(&lock).unwrap();
    }
    let fifo = std::ffi::CString::new(lock.as_os_str().as_bytes()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
    assert!(crate::core::implementation::mark_resume_started(&s.repo, &s.ticket).is_err());
    assert_eq!(
        load(&s.repo, &s.ticket).unwrap().status,
        ImplementationStatus::Interrupted
    );
}
