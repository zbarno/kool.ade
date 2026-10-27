use super::*;

#[test]
// Production boundary: container stage-out must reject changed task-clone
// files outside the recipe's declared outputs. A path matcher test cannot
// prove the tar snapshot is checked before writes. Uses a local temporary
// workspace and synthetic archive only; no Docker or network setup.
fn recipe_stage_out_rejects_changes_outside_declared_outputs() {
    let fixture = Fixture::new();
    let workspace = ValidatedWorkspace::inspect(&fixture.0).unwrap();
    let snapshot = capture(&workspace, 1024 * 1024, false).unwrap();
    let archive = returned(&[("source.txt", b"container edit")]);

    let result = super::super::super::synchronize_with_source_output_policy(
        &workspace,
        &snapshot,
        &archive,
        1024 * 1024,
        &[],
        Some(&["dist/index.html".into()]),
    );

    assert!(result.is_err());
    assert_eq!(fs::read(fixture.0.join("source.txt")).unwrap(), b"original");
}

#[cfg(target_os = "linux")]
#[test]
// Production boundary: a Test recipe may stage in Build outputs, but its
// returned archive may update only its own declared outputs. A matcher unit
// test cannot prove rejection happens before host writes. Uses a temporary
// workspace and synthetic tar archive; no Docker or network setup is needed.
fn test_recipe_stage_out_cannot_modify_upstream_build_output() {
    let fixture = Fixture::new();
    fs::create_dir_all(fixture.0.join("dist")).unwrap();
    fs::write(fixture.0.join("dist/site.html"), b"build output").unwrap();
    let workspace = ValidatedWorkspace::inspect(&fixture.0).unwrap();
    let snapshot = capture(&workspace, 1024 * 1024, false).unwrap();
    let archive = returned(&[
        ("dist/site.html", b"tampered build output"),
        ("test-results/report.xml", b"test result"),
    ]);

    let result = super::super::super::synchronize_with_source_output_policy(
        &workspace,
        &snapshot,
        &archive,
        1024 * 1024,
        &[],
        Some(&["test-results/report.xml".into()]),
    );

    assert!(result.is_err());
    assert_eq!(
        fs::read(fixture.0.join("dist/site.html")).unwrap(),
        b"build output"
    );
    assert!(!fixture.0.join("test-results/report.xml").exists());
}

#[cfg(target_os = "linux")]
#[test]
// Production boundary: an approved recipe output still synchronizes while
// unrelated source files remain unchanged. The same temp workspace and
// synthetic archive setup as the rejection case; no Docker or network.
fn recipe_stage_out_applies_declared_outputs() {
    let fixture = Fixture::new();
    let workspace = ValidatedWorkspace::inspect(&fixture.0).unwrap();
    let snapshot = capture(&workspace, 1024 * 1024, false).unwrap();
    let archive = returned(&[
        ("source.txt", b"original"),
        ("dist/index.html", b"built output"),
    ]);

    super::super::super::synchronize_with_source_output_policy(
        &workspace,
        &snapshot,
        &archive,
        1024 * 1024,
        &[],
        Some(&["dist/index.html".into()]),
    )
    .unwrap();

    assert_eq!(fs::read(fixture.0.join("source.txt")).unwrap(), b"original");
    assert_eq!(
        fs::read(fixture.0.join("dist/index.html")).unwrap(),
        b"built output"
    );
}

#[cfg(target_os = "linux")]
#[test]
// Production boundary: a recipe mounted at a nested target root must stage
// its declared outputs without treating the target's host-side ancestors as
// deletions. Uses the scoped capture and synchronization paths with a local
// temporary checkout and synthetic archive; no Docker or network setup.
fn nested_target_stage_out_preserves_ancestor_directories() {
    let fixture = Fixture::new();
    fs::create_dir_all(fixture.0.join("apps/web")).unwrap();
    let workspace = ValidatedWorkspace::inspect(&fixture.0).unwrap();
    let snapshot = capture_scoped(&workspace, "apps/web", 1024 * 1024).unwrap();
    let archive = returned(&[("dist/index.html", b"built output")]);

    super::super::super::synchronize_with_source_output_policy(
        &workspace,
        &snapshot,
        &archive,
        1024 * 1024,
        &[],
        Some(&["apps/web/dist/index.html".into()]),
    )
    .unwrap();

    assert!(fixture.0.join("apps").is_dir());
    assert!(fixture.0.join("apps/web").is_dir());
    assert_eq!(
        fs::read(fixture.0.join("apps/web/dist/index.html")).unwrap(),
        b"built output"
    );
}

#[cfg(target_os = "linux")]
#[test]
// Production boundary: an output's parent directory can be created for a
// nested approved artifact, but a file at that ancestor path cannot replace
// the directory. Uses the real stage-out path over a synthetic archive.
fn recipe_stage_out_rejects_file_at_ancestor_of_declared_output() {
    let fixture = Fixture::new();
    let workspace = ValidatedWorkspace::inspect(&fixture.0).unwrap();
    let snapshot = capture(&workspace, 1024 * 1024, false).unwrap();
    let archive = returned(&[("dist", b"not a directory")]);

    let result = super::super::super::synchronize_with_source_output_policy(
        &workspace,
        &snapshot,
        &archive,
        1024 * 1024,
        &[],
        Some(&["dist/index.html".into()]),
    );

    assert!(result.is_err());
    assert!(!fixture.0.join("dist").exists());
}

#[cfg(target_os = "linux")]
#[test]
// Production boundary: a recipe cannot delete a tracked file that is an
// ancestor of a declared nested output. Uses the real stage-out path and
// proves the host workspace remains intact after denial.
fn recipe_stage_out_rejects_deleting_ancestor_of_declared_output() {
    let fixture = Fixture::new();
    fs::write(fixture.0.join("dist"), b"tracked file").unwrap();
    let workspace = ValidatedWorkspace::inspect(&fixture.0).unwrap();
    let snapshot = capture(&workspace, 1024 * 1024, false).unwrap();
    let archive = returned(&[]);

    let result = super::super::super::synchronize_with_source_output_policy(
        &workspace,
        &snapshot,
        &archive,
        1024 * 1024,
        &[],
        Some(&["dist/index.html".into()]),
    );

    assert!(result.is_err());
    assert_eq!(fs::read(fixture.0.join("dist")).unwrap(), b"tracked file");
}
