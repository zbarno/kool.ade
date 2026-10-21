use super::*;

#[test]
fn reviewed_capture_binds_staged_bytes_to_the_pinned_git_tree() {
    let fixture = Fixture::new();
    git(&fixture.0, &["init", "--quiet"]);
    git(&fixture.0, &["config", "user.name", "Fixture"]);
    git(
        &fixture.0,
        &["config", "user.email", "fixture@example.invalid"],
    );
    git(&fixture.0, &["add", "source.txt"]);
    git(&fixture.0, &["commit", "--quiet", "-m", "reviewed source"]);
    let source_sha = git(&fixture.0, &["rev-parse", "HEAD"]);
    let workspace = ValidatedWorkspace::inspect(&fixture.0).unwrap();
    let snapshot = capture_reviewed(&workspace, &source_sha, &[], None, 1024 * 1024, true)
        .expect("clean reviewed source should stage");

    let mut changed_content = snapshot.clone();
    changed_content.manifest.insert(
        "source.txt".into(),
        super::super::archive::Fingerprint::File {
            sha256: super::super::archive::hash(b"transient edit"),
            size: b"transient edit".len() as u64,
            mode: 0o644,
        },
    );
    assert!(
        super::super::verify_snapshot_matches_git_tree(
            &changed_content,
            &fixture.0,
            &source_sha,
            &[],
        )
        .is_err()
    );

    let mut missing_file = snapshot.clone();
    missing_file.manifest.remove(Path::new("source.txt"));
    assert!(
        super::super::verify_snapshot_matches_git_tree(&missing_file, &fixture.0, &source_sha, &[],)
            .is_err()
    );

    let mut changed_mode = snapshot;
    if let Some(super::super::archive::Fingerprint::File { mode, .. }) =
        changed_mode.manifest.get_mut(Path::new("source.txt"))
    {
        *mode |= 0o111;
    }
    assert!(
        super::super::verify_snapshot_matches_git_tree(&changed_mode, &fixture.0, &source_sha, &[],)
            .is_err()
    );
}

// Production boundary: reviewed task-clone staging must accept a generated
// file authorized only by the recipe's expected-artifact list. A path matcher
// unit test cannot prove checkout verification and snapshot comparison agree.
// This uses the local Git executable to initialize and commit one temporary
// repository; setup cost is low and Docker or network access is not required.

#[test]
fn reviewed_capture_accepts_expected_artifact_only_outputs() {
    let fixture = Fixture::new();
    git(&fixture.0, &["init", "--quiet"]);
    git(&fixture.0, &["config", "user.name", "Fixture"]);
    git(
        &fixture.0,
        &["config", "user.email", "fixture@example.invalid"],
    );
    git(&fixture.0, &["add", "source.txt"]);
    git(&fixture.0, &["commit", "--quiet", "-m", "reviewed source"]);
    let source_sha = git(&fixture.0, &["rev-parse", "HEAD"]);
    fs::create_dir_all(fixture.0.join("dist")).unwrap();
    fs::write(fixture.0.join("dist/index.html"), b"built output").unwrap();
    let workspace = ValidatedWorkspace::inspect(&fixture.0).unwrap();

    let snapshot = capture_reviewed(
        &workspace,
        &source_sha,
        &["dist/index.html".into()],
        None,
        1024 * 1024,
        true,
    )
    .expect("the expected artifact is an authorized source output");

    assert!(snapshot.manifest.contains_key(Path::new("dist/index.html")));
}
