use super::support::Fixture;
use crate::artifacts::{items_io, planning_store::paths};
#[test]
fn migration_normalizes_legacy_item_without_uid_and_reuses_the_identity() {
    let fixture = Fixture::new("legacy-item-without-uid");
    let store = crate::artifacts::planning_store::PlanningStore::legacy_embedded(
        uuid::Uuid::nil(),
        &fixture.code,
    );
    let mut item = fixture.open_item.clone();
    item.uid = None;
    store
        .atomic_write(paths::OPEN_ITEMS, items_io::serialize(&[item]).as_bytes())
        .unwrap();
    super::support::git(
        &fixture.code,
        &["add", ".koolade-packet/planning/open-items.md"],
    );
    super::support::git(
        &fixture.code,
        &["commit", "--quiet", "-m", "remove legacy item UID"],
    );
    let source_before = fixture.source_snapshot();

    let workspace = fixture
        .manager
        .migrate_legacy_checkout(&fixture.code)
        .unwrap();
    let (open, _, _) = items_io::load_store(&workspace.store).unwrap();
    assert_eq!(open.len(), 1);
    let generated_uid = open[0].uid.clone().expect("migration assigns a stable UID");
    assert!(uuid::Uuid::parse_str(&generated_uid).is_ok());
    assert_eq!(fixture.source_snapshot(), source_before);

    let resumed = fixture
        .manager
        .migrate_legacy_checkout(&fixture.code)
        .unwrap();
    let (resumed_open, _, _) = items_io::load_store(&resumed.store).unwrap();
    assert_eq!(resumed_open[0].uid.as_deref(), Some(generated_uid.as_str()));
    assert_eq!(fixture.source_snapshot(), source_before);
}
