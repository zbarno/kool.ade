use super::{Fixture, ValidatedWorkspace, capture_scoped, fs};
use std::path::Path;

// This test covers the application staging boundary that binds the captured subtree
// to a revalidated task-clone identity. Direct archive tests cannot prove that the
// scoped caller uses the held root handle and records the original write boundary.
// It uses native temporary directories only; no Docker or network service is needed.
// Setup cost is low.
#[test]
fn scoped_capture_stages_only_the_selected_task_clone_subtree() {
    let fixture = Fixture::new();
    fs::create_dir_all(fixture.0.join("apps/web")).unwrap();
    fs::create_dir_all(fixture.0.join("apps/api")).unwrap();
    fs::write(fixture.0.join("apps/web/index.html"), b"web source").unwrap();
    fs::write(fixture.0.join("apps/api/secret.txt"), b"sibling data").unwrap();

    let workspace = ValidatedWorkspace::inspect(&fixture.0).unwrap();
    let snapshot = capture_scoped(&workspace, "apps/web", 1024 * 1024).unwrap();

    assert_eq!(snapshot.scope_root.as_deref(), Some(Path::new("apps/web")));
    assert!(
        snapshot
            .manifest
            .contains_key(Path::new("apps/web/index.html"))
    );
    assert!(
        !snapshot
            .manifest
            .contains_key(Path::new("apps/api/secret.txt"))
    );
    let entries = tar::Archive::new(snapshot.archive.as_slice())
        .entries()
        .unwrap()
        .map(|entry| entry.unwrap().path().unwrap().into_owned())
        .collect::<Vec<_>>();
    assert_eq!(entries, vec![Path::new("index.html").to_path_buf()]);
}
