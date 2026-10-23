use super::*;

#[cfg(unix)]
#[test]
fn scoped_stage_and_sync_cannot_modify_sibling_projects() {
    let fixture = Fixture::new();
    fs::create_dir_all(fixture.0.join("apps/web")).unwrap();
    fs::create_dir_all(fixture.0.join("apps/api")).unwrap();
    fs::write(fixture.0.join("apps/web/index.html"), b"web-original").unwrap();
    fs::write(fixture.0.join("apps/api/secret.txt"), b"api-original").unwrap();
    let workspace = ValidatedWorkspace::inspect(&fixture.0).unwrap();
    let snapshot = capture_scoped(&workspace, "apps/web", 1024 * 1024).unwrap();
    let entries = tar::Archive::new(Cursor::new(&snapshot.archive))
        .entries()
        .unwrap()
        .map(|entry| entry.unwrap().path().unwrap().into_owned())
        .collect::<Vec<_>>();
    assert!(entries.iter().any(|path| path == Path::new("index.html")));
    assert!(
        !entries
            .iter()
            .any(|path| path.starts_with("../") || path.starts_with("apps/api"))
    );

    let valid = returned(&[("index.html", b"web-updated")]);
    synchronize(&workspace, &snapshot, &valid, 1024 * 1024).unwrap();
    assert_eq!(
        fs::read(fixture.0.join("apps/web/index.html")).unwrap(),
        b"web-updated"
    );
    assert_eq!(
        fs::read(fixture.0.join("apps/api/secret.txt")).unwrap(),
        b"api-original"
    );

    let escape = traversal_returned("../api/secret.txt", b"stolen");
    assert!(synchronize(&workspace, &snapshot, &escape, 1024 * 1024).is_err());
    assert_eq!(
        fs::read(fixture.0.join("apps/api/secret.txt")).unwrap(),
        b"api-original"
    );
}

#[cfg(unix)]
#[test]
fn scoped_stage_and_sync_supports_an_explicit_project_root_target() {
    let fixture = Fixture::new();
    fs::create_dir_all(&fixture.0).unwrap();
    fs::write(fixture.0.join("app.txt"), b"original").unwrap();
    let workspace = ValidatedWorkspace::inspect(&fixture.0).unwrap();
    let snapshot = capture_scoped(&workspace, ".", 1024 * 1024).unwrap();
    assert_eq!(snapshot.scope_root.as_deref(), Some(Path::new(".")));
    assert!(snapshot.manifest.contains_key(Path::new("app.txt")));
    let recovered = super::super::WorkspaceSnapshot::from_record_entries(
        &snapshot.record_entries(),
        snapshot.scope_root.clone(),
    )
    .unwrap();

    synchronize(
        &workspace,
        &recovered,
        &returned(&[("app.txt", b"updated")]),
        1024 * 1024,
    )
    .unwrap();
    assert_eq!(fs::read(fixture.0.join("app.txt")).unwrap(), b"updated");
    assert!(
        synchronize(
            &workspace,
            &recovered,
            &traversal_returned("../outside.txt", b"escape"),
            1024 * 1024,
        )
        .is_err()
    );
}

#[cfg(unix)]
#[test]
fn scoped_staging_rejects_symlinked_target_roots() {
    use std::os::unix::fs::symlink;
    let fixture = Fixture::new();
    let outside = fixture.0.with_extension("outside");
    fs::create_dir_all(&outside).unwrap();
    symlink(&outside, fixture.0.join("linked-app")).unwrap();
    let workspace = ValidatedWorkspace::inspect(&fixture.0).unwrap();
    assert!(capture_scoped(&workspace, "linked-app", 1024 * 1024).is_err());
    let _ = fs::remove_dir_all(outside);
}
