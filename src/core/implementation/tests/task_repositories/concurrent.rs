use super::*;

#[test]
fn concurrent_tasks_get_independent_clones_and_branches() {
    let sandbox = Sandbox::new();
    let second_ticket = "planning/tasks/feature/002-independent-task.md";
    let second_ticket_text = "# Independent task\n\nFeature: F1\n\n## Acceptance criteria\n\n- File contains implemented.\n";
    let second_ticket_document = crate::domain::ArtifactIdentity::preserve_markdown(
        second_ticket_text,
        None,
        "TASK-002",
        "Independent task",
    )
    .unwrap();
    fs::write(sandbox.repo.join(second_ticket), second_ticket_document).unwrap();
    sandbox.git(&sandbox.repo, &["add", second_ticket]);
    sandbox.git(&sandbox.repo, &["commit", "-qm", "add independent task"]);

    let (first, second) = std::thread::scope(|scope| {
        let first = scope.spawn(|| {
            let _executor = super::super::sandbox::install_verification_executor();
            run_on_worker(&sandbox, &sandbox.ticket)
        });
        let second = scope.spawn(|| {
            let _executor = super::super::sandbox::install_verification_executor();
            run_on_worker(&sandbox, second_ticket)
        });
        (first.join().unwrap(), second.join().unwrap())
    });
    let first = first.unwrap();
    let second = second.unwrap();

    assert_ne!(first.task_repository, second.task_repository);
    assert_ne!(first.branch, second.branch);
    assert_ne!(
        common(&first.task_repository).unwrap(),
        common(&second.task_repository).unwrap()
    );
    assert!(first.task_repository.join(".git").is_dir());
    assert!(second.task_repository.join(".git").is_dir());
    assert_eq!(
        first
            .task_repository_commits
            .get(&first.task_repository.to_string_lossy().into_owned()),
        first.verified_head.as_ref()
    );

    fs::write(first.task_repository.join("isolated.txt"), "first clone\n").unwrap();
    sandbox.git(&first.task_repository, &["add", "isolated.txt"]);
    sandbox.git(
        &first.task_repository,
        &["commit", "-qm", "change first task clone"],
    );
    assert!(!second.task_repository.join("isolated.txt").exists());
    assert!(!sandbox.repo.join("isolated.txt").exists());
}
