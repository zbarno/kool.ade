use super::*;

#[test]
fn independent_work_records_merge_while_same_record_updates_are_fenced() {
    let root = super::root();
    let mut first = Work::new(
        "planning:first".into(),
        "First".into(),
        "First request".into(),
        "First detail".into(),
    );
    let mut second = Work::new(
        "planning:second".into(),
        "Second".into(),
        "Second request".into(),
        "Second detail".into(),
    );
    save(&root, &[first.clone(), second.clone()]).unwrap();
    let base_revision = root.planning_store().revision().unwrap();
    let persisted = load(&root).unwrap();
    let stale_first = persisted
        .iter()
        .find(|item| item.uid == first.uid)
        .unwrap()
        .clone();
    let stale_second = persisted
        .iter()
        .find(|item| item.uid == second.uid)
        .unwrap()
        .clone();

    let mut first_snapshot = persisted.clone();
    first = stale_first.clone();
    first.detail = "First operator update".into();
    first_snapshot
        .iter_mut()
        .find(|item| item.uid == first.uid)
        .unwrap()
        .detail = first.detail.clone();
    save_expected(&root, &first_snapshot, &base_revision).unwrap();

    let mut second_snapshot = persisted.clone();
    second = stale_second;
    second.detail = "Second operator update".into();
    second_snapshot
        .iter_mut()
        .find(|item| item.uid == second.uid)
        .unwrap()
        .detail = second.detail.clone();
    save_expected(&root, &second_snapshot, &base_revision).unwrap();
    let merged = load(&root).unwrap();
    assert_eq!(merged.len(), 2);
    assert!(
        merged
            .iter()
            .any(|item| item.detail == "First operator update")
    );
    assert!(
        merged
            .iter()
            .any(|item| item.detail == "Second operator update")
    );

    let mut conflicting_snapshot = persisted;
    let mut conflicting = stale_first;
    conflicting.detail = "Stale same-record update".into();
    conflicting_snapshot
        .iter_mut()
        .find(|item| item.uid == conflicting.uid)
        .unwrap()
        .detail = conflicting.detail;
    assert!(save_expected(&root, &conflicting_snapshot, &base_revision).is_err());
    assert_eq!(
        load(&root)
            .unwrap()
            .iter()
            .find(|item| item.uid == first.uid)
            .unwrap()
            .detail,
        "First operator update"
    );
    std::fs::remove_dir_all(root).unwrap();
}
