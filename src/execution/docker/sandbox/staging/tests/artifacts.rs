use super::{Fixture, returned};
use crate::execution::docker::sandbox::ValidatedWorkspace;

#[test]
fn artifact_manifest_hashes_only_exact_requested_regular_files() {
    let archive = returned(&[
        ("./dist/index.html", b"<main>ready</main>"),
        ("./dist/extra.js", b"ignored"),
    ]);
    let artifacts =
        super::super::collect_artifacts(&archive, 16 * 1024, &["dist/index.html".into()]).unwrap();

    assert_eq!(artifacts.len(), 1);
    assert_eq!(artifacts[0].relative_path, "dist/index.html");
    assert_eq!(artifacts[0].size_bytes, b"<main>ready</main>".len() as u64);
    assert_eq!(artifacts[0].media_type, "text/html");
    assert_eq!(
        artifacts[0].sha256,
        super::super::archive::hash(b"<main>ready</main>")
    );
}

#[test]
fn artifact_manifest_rejects_missing_duplicates_and_oversized_selections() {
    let archive = returned(&[("dist/app.bin", b"payload")]);
    let entries = super::super::archive::parse_returned(&archive, 16 * 1024, 10).unwrap();
    assert!(super::super::artifacts::collect(&entries, &["missing".into()], 16 * 1024).is_err());
    assert!(
        super::super::artifacts::collect(
            &entries,
            &["dist/app.bin".into(), "dist/app.bin".into()],
            16 * 1024,
        )
        .is_err()
    );
    assert!(super::super::artifacts::collect(&entries, &["dist/app.bin".into()], 6).is_err());
}

#[cfg(unix)]
#[test]
fn synchronization_returns_hashes_for_files_applied_to_the_task_clone() {
    let fixture = Fixture::new();
    let workspace = ValidatedWorkspace::inspect(&fixture.0).unwrap();
    let snapshot = super::capture(&workspace, 1024 * 1024, false).unwrap();
    let archive = returned(&[
        ("source.txt", b"original"),
        ("dist/index.html", b"<h1>built</h1>"),
    ]);
    let artifacts = super::super::synchronize_with_artifacts(
        &workspace,
        &snapshot,
        &archive,
        1024 * 1024,
        &["dist/index.html".into()],
    )
    .unwrap();

    assert_eq!(
        std::fs::read(fixture.0.join("dist/index.html")).unwrap(),
        b"<h1>built</h1>"
    );
    assert_eq!(artifacts.len(), 1);
    assert_eq!(artifacts[0].relative_path, "dist/index.html");
    assert_eq!(
        artifacts[0].sha256,
        super::super::archive::hash(b"<h1>built</h1>")
    );
}
