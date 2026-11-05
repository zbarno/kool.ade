use super::*;
use crate::core::published_specs::PublishedDocument;
use std::sync::{Arc, Barrier};

/// Production boundary: immutable published-spec reads during real Git
/// checkout changes and concurrent task-branch updates. A mocked reader cannot
/// prove the exact commit object stays authoritative across branch activity.
/// Uses only temporary local repositories and a bare remote; setup is low cost.
#[test]
fn published_spec_pin_survives_parallel_task_branches_and_human_checkout_change() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("primary");
    let remote = temporary.path().join("primary.git");
    let first_task = temporary.path().join("task-a");
    let second_task = temporary.path().join("task-b");
    fs::create_dir_all(&root).unwrap();
    git(&root, &["init", "--quiet", "-b", "main"]);
    git(&root, &["config", "user.name", "Koolade Test"]);
    git(
        &root,
        &["config", "user.email", "koolade-test@example.invalid"],
    );
    let spec_sha = commit_spec(&root, "# Published\n\nAuthoritative revision.\n");
    git(
        temporary.path(),
        &["init", "--bare", "--quiet", remote.to_str().unwrap()],
    );
    git(
        &root,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    git(&root, &["push", "--quiet", "origin", "main"]);

    let reader = PublishedSpecReader::new(&root, "primary", "refs/heads/main").unwrap();
    let pin = reader.pin().unwrap();
    assert_eq!(pin.commit_sha, spec_sha);

    git(
        temporary.path(),
        &[
            "clone",
            "--quiet",
            remote.to_str().unwrap(),
            first_task.to_str().unwrap(),
        ],
    );
    git(
        temporary.path(),
        &[
            "clone",
            "--quiet",
            remote.to_str().unwrap(),
            second_task.to_str().unwrap(),
        ],
    );
    for task in [&first_task, &second_task] {
        git(task, &["config", "user.name", "Koolade Test"]);
        git(
            task,
            &["config", "user.email", "koolade-test@example.invalid"],
        );
    }

    git(&root, &["checkout", "--quiet", "-b", "human/working"]);
    commit_spec(
        &root,
        "# Published\n\nUnpublished human checkout content.\n",
    );
    let barrier = Arc::new(Barrier::new(3));
    let (first_result, second_result, observed) = std::thread::scope(|scope| {
        let first_reader = reader.clone();
        let first_pin = pin.clone();
        let first_barrier = Arc::clone(&barrier);
        let first_task_path = first_task.clone();
        let first = scope.spawn(move || {
            first_barrier.wait();
            branch_and_read(
                &first_task_path,
                "koolade/task-a",
                "task-a",
                &first_reader,
                &first_pin,
            )
        });
        let second_reader = reader.clone();
        let second_pin = pin.clone();
        let second_barrier = Arc::clone(&barrier);
        let second_task_path = second_task.clone();
        let second = scope.spawn(move || {
            second_barrier.wait();
            branch_and_read(
                &second_task_path,
                "koolade/task-b",
                "task-b",
                &second_reader,
                &second_pin,
            )
        });
        barrier.wait();
        let observed = reader.read(&pin, SPEC_PATH).unwrap();
        (first.join().unwrap(), second.join().unwrap(), observed)
    });

    assert_eq!(first_result.0.pin.commit_sha, spec_sha);
    assert_eq!(second_result.0.pin.commit_sha, spec_sha);
    assert_eq!(observed.pin.commit_sha, spec_sha);
    assert_eq!(
        first_result.0.content,
        "# Published\n\nAuthoritative revision.\n"
    );
    assert_eq!(second_result.0.content, first_result.0.content);
    assert_eq!(observed.content, first_result.0.content);
    assert_ne!(first_result.1, spec_sha);
    assert_ne!(second_result.1, spec_sha);
    assert_eq!(
        git(&first_task, &["branch", "--show-current"]),
        "koolade/task-a"
    );
    assert_eq!(
        git(&second_task, &["branch", "--show-current"]),
        "koolade/task-b"
    );
    assert_eq!(git(&root, &["branch", "--show-current"]), "human/working");
    assert_eq!(git(&root, &["rev-parse", "refs/heads/main"]), spec_sha);
    assert!(reader.ensure_local_ref_unchanged(&pin).is_ok());
}

fn branch_and_read(
    task: &Path,
    branch: &str,
    marker: &str,
    reader: &PublishedSpecReader,
    pin: &PublishedSpecPin,
) -> (PublishedDocument, String) {
    git(task, &["checkout", "--quiet", "-b", branch]);
    fs::write(task.join(format!("{marker}.txt")), "private task change\n").unwrap();
    git(task, &["add", &format!("{marker}.txt")]);
    git(task, &["commit", "--quiet", "-m", "Task-only change"]);
    let task_head = git(task, &["rev-parse", "HEAD"]);
    (reader.read(pin, SPEC_PATH).unwrap(), task_head)
}
