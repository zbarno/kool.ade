use super::*;

#[cfg(target_os = "linux")]
#[test]
// Production boundary: task-clone stage-out must reject sibling edits beside
// an exact declared artifact. A path matcher unit test cannot prove the
// archive is rejected before host writes.
// Uses a temporary workspace and synthetic archive; no Docker or network.
fn custom_artifact_stage_out_rejects_sibling_changes() {
    let fixture = Fixture::new();
    fs::create_dir_all(fixture.0.join("dist")).unwrap();
    fs::write(fixture.0.join("dist/index.html"), b"approved baseline").unwrap();
    fs::write(fixture.0.join("dist/private.txt"), b"sibling baseline").unwrap();
    let workspace = ValidatedWorkspace::inspect(&fixture.0).unwrap();
    let snapshot = super::super::capture(&workspace, 1024 * 1024, false).unwrap();
    let archive = returned(&[
        ("dist/index.html", b"approved build output"),
        ("dist/private.txt", b"unapproved sibling edit"),
    ]);

    let result = super::super::super::synchronize_with_source_output_policy(
        &workspace,
        &snapshot,
        &archive,
        1024 * 1024,
        &[],
        Some(&["dist/index.html".into()]),
    );

    assert!(result.is_err());
    assert_eq!(
        fs::read(fixture.0.join("dist/index.html")).unwrap(),
        b"approved baseline"
    );
    assert_eq!(
        fs::read(fixture.0.join("dist/private.txt")).unwrap(),
        b"sibling baseline"
    );
}
