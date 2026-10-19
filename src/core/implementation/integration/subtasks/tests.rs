use super::*;
use crate::core::implementation::runner::Runner;
use std::{
    fs,
    process::Command,
    sync::{Arc, atomic::AtomicBool, mpsc},
    time::{Duration, Instant},
};

/// Production boundary: real local Git repositories are fetched and squash-integrated,
/// and the parent marker scan must bind both commits to their reviewed heads. A unit
/// test of metadata alone cannot prove Git's resulting tree or commit ancestry. External
/// dependency: Git executable only; setup cost: moderate (three temporary repositories).
#[test]
fn reviewed_subtasks_squash_into_one_parent_diff_and_retry_markers_validate() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let parent = root.join("parent");
    let first = root.join("child-one");
    let second = root.join("child-two");
    fs::create_dir(&parent).unwrap();
    git(&parent, &["init", "--quiet", "--initial-branch=main"]);
    configure(&parent);
    fs::write(parent.join("base.txt"), "base\n").unwrap();
    git(&parent, &["add", "base.txt"]);
    git(&parent, &["commit", "--quiet", "-m", "base"]);
    let base = git(&parent, &["rev-parse", "HEAD"]);

    for (path, filename, contents) in [
        (&first, "first.txt", "first child\n"),
        (&second, "second.txt", "second child\n"),
    ] {
        git(
            root,
            &[
                "clone",
                "--quiet",
                parent.to_str().unwrap(),
                path.to_str().unwrap(),
            ],
        );
        configure(path);
        fs::write(path.join(filename), contents).unwrap();
        git(path, &["add", filename]);
        git(path, &["commit", "--quiet", "-m", filename]);
    }
    let first_head = git(&first, &["rev-parse", "HEAD"]);
    let second_head = git(&second, &["rev-parse", "HEAD"]);
    let runner = test_runner();
    let first_uid = "11111111-1111-4111-8111-111111111111";
    let second_uid = "22222222-2222-4222-8222-222222222222";

    git::squash_reviewed_child(
        &parent,
        first.to_str().unwrap(),
        first_uid,
        "TASK-1",
        &first_head,
        &runner,
    )
    .unwrap();
    git::squash_reviewed_child(
        &parent,
        second.to_str().unwrap(),
        second_uid,
        "TASK-2",
        &second_head,
        &runner,
    )
    .unwrap();

    assert_eq!(
        fs::read_to_string(parent.join("first.txt")).unwrap(),
        "first child\n"
    );
    assert_eq!(
        fs::read_to_string(parent.join("second.txt")).unwrap(),
        "second child\n"
    );
    let expected = vec![
        IntegratedSubtask {
            task_uid: first_uid.to_owned(),
            reviewed_head_sha: first_head,
        },
        IntegratedSubtask {
            task_uid: second_uid.to_owned(),
            reviewed_head_sha: second_head,
        },
    ];
    assert_eq!(
        git::integrated_children(&parent, &base, "HEAD", &expected, &runner)
            .unwrap()
            .len(),
        2
    );
    let stale = vec![IntegratedSubtask {
        task_uid: first_uid.to_owned(),
        reviewed_head_sha: "0".repeat(40),
    }];
    assert!(git::integrated_children(&parent, &base, "HEAD", &stale, &runner).is_err());
    let current_head = git(&parent, &["rev-parse", "HEAD"]);
    assert!(
        git::validate_receipt_binding(
            &parent,
            &base,
            &base,
            &current_head,
            &current_head,
            &expected,
            &runner,
        )
        .is_ok()
    );
    assert!(
        git::validate_receipt_binding(
            &parent,
            &base,
            &"0".repeat(40),
            &current_head,
            &current_head,
            &expected,
            &runner,
        )
        .is_err()
    );
    assert!(
        git::validate_receipt_binding(
            &parent,
            &base,
            &base,
            &current_head,
            &base,
            &expected,
            &runner,
        )
        .is_err()
    );
}

fn git(cwd: &std::path::Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn configure(repo: &std::path::Path) {
    git(repo, &["config", "user.name", "Test User"]);
    git(repo, &["config", "user.email", "test@example.invalid"]);
}

fn test_runner() -> Runner {
    let (progress, _receiver) = mpsc::channel();
    Runner {
        gh: "unused".into(),
        runtime_config_source: None,
        deadline: Instant::now() + Duration::from_secs(60),
        cancel: Arc::new(AtomicBool::new(false)),
        progress,
        authorization_verifier: None,
    }
}
