use super::support::{Fixture, git};
use std::fs;

#[test]
fn migration_preview_reports_stable_ids_and_keeps_legacy_source_unchanged() {
    let fixture = Fixture::new("read-only-preview");
    let before = fixture.source_snapshot();
    let head = String::from_utf8(git(&fixture.code, &["rev-parse", "HEAD"]))
        .unwrap()
        .trim()
        .to_owned();

    let preview = fixture
        .manager
        .preview_legacy_migration(&fixture.code)
        .unwrap();

    assert_eq!(preview.source_root, fixture.code.canonicalize().unwrap());
    assert_eq!(preview.source_commit, head);
    assert_eq!(preview.workspace_id, fixture.legacy_workspace_id);
    assert!(preview.file_count > 0);
    assert!(preview.record_ids.contains(&fixture.work_uid));
    assert_eq!(preview.backup_path, fixture.code.join(".koolade-packet"));
    assert_eq!(fixture.source_snapshot(), before);
    assert_eq!(
        String::from_utf8(git(&fixture.code, &["rev-parse", "HEAD"]))
            .unwrap()
            .trim(),
        head
    );
}

#[test]
fn migration_confirmation_rejects_source_changes_after_preview() {
    let fixture = Fixture::new("changed-after-preview");
    let preview = fixture
        .manager
        .preview_legacy_migration(&fixture.code)
        .unwrap();
    let work_path = fixture
        .code
        .join(crate::artifacts::layout::store::ROOT)
        .join(crate::artifacts::planning_store::paths::WORK);
    let mut changed = fs::read(&work_path).unwrap();
    changed.push(b'\n');
    fs::write(&work_path, &changed).unwrap();

    let error = fixture
        .manager
        .migrate_legacy_checkout_from_preview(&preview)
        .unwrap_err();

    assert!(format!("{error:#}").contains("changed after the migration preview"));
    assert_eq!(fs::read(&work_path).unwrap(), changed);
    assert!(
        fixture
            .manager
            .workspace_for_checkout(&fixture.code)
            .unwrap()
            .is_none()
    );
}
