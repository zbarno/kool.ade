use super::*;

pub(super) fn publish_conflicting_updates(first: &Path, second: &Path) {
    git(first, &["checkout", "--quiet", "-b", "docs/spec-one"]);
    edit_spec(first, "Published from installation one.");
    git(first, &["add", SPEC]);
    git(
        first,
        &[
            "commit",
            "--quiet",
            "-m",
            "Publish installation one spec update",
        ],
    );
    git(first, &["checkout", "--quiet", "main"]);
    git(first, &["merge", "--quiet", "--ff-only", "docs/spec-one"]);
    git(first, &["push", "--quiet", "origin", "main"]);

    git(second, &["checkout", "--quiet", "-b", "docs/spec-two"]);
    edit_spec(second, "Published from installation two.");
    git(second, &["add", SPEC]);
    git(
        second,
        &[
            "commit",
            "--quiet",
            "-m",
            "Publish installation two spec update",
        ],
    );
    git(second, &["fetch", "--quiet", "origin"]);
    let conflict = run_git(second, &["merge", "origin/main"]);
    assert!(!conflict.status.success(), "stale update must conflict");
    let content = std::fs::read_to_string(second.join(SPEC)).unwrap();
    assert!(content.contains("Published from installation one."));
    assert!(content.contains("Published from installation two."));
    std::fs::write(
        second.join(SPEC),
        content
            .replace("<<<<<<< HEAD\n", "")
            .replace("=======\n", "")
            .replace(">>>>>>> origin/main\n", ""),
    )
    .unwrap();
    git(second, &["add", SPEC]);
    git(
        second,
        &[
            "commit",
            "--quiet",
            "-m",
            "Resolve published spec update conflict",
        ],
    );
    git(second, &["checkout", "--quiet", "main"]);
    git(second, &["merge", "--quiet", "--ff-only", "origin/main"]);
    git(second, &["merge", "--quiet", "--no-edit", "docs/spec-two"]);
    git(second, &["push", "--quiet", "origin", "main"]);
}

fn edit_spec(repo: &Path, update: &str) {
    let path = repo.join(SPEC);
    let contents = std::fs::read_to_string(&path).unwrap();
    assert!(contents.contains("Test product context."));
    std::fs::write(path, contents.replace("Test product context.", update)).unwrap();
}
