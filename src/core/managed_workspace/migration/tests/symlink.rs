use std::{fs, os::unix::fs::symlink};

use super::support::Fixture;

#[test]
fn symlink_escape_blocks_migration_without_changing_source_or_target() {
    let fixture = Fixture::new("symlink-escape");
    let outside = fixture.root.join("outside.md");
    fs::write(&outside, "operator-owned file\n").unwrap();
    let link = fixture.code.join(".koolade-packet/planning/escaped.md");
    symlink(&outside, &link).unwrap();
    let before = fixture.source_snapshot();

    let error = fixture
        .manager
        .migrate_legacy_checkout(&fixture.code)
        .unwrap_err();

    assert!(
        error.to_string().contains("planning/escaped.md"),
        "unexpected preflight error: {error:#}"
    );
    assert!(
        fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(
        fs::read_to_string(&outside).unwrap(),
        "operator-owned file\n"
    );
    assert_eq!(fixture.source_snapshot(), before);
    assert!(
        fixture
            .manager
            .workspace_for_checkout(&fixture.code)
            .unwrap()
            .is_none()
    );
}
