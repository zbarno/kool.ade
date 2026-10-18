use std::fs;

use super::{
    MAX_EVENT_RECORD_BYTES, MAX_JOURNAL_BYTES, MAX_JOURNAL_EVENTS, inspect_journal_entry,
    within_journal_limits,
};

#[test]
fn journal_append_enforces_prospective_event_and_byte_quotas() {
    assert!(within_journal_limits(
        MAX_JOURNAL_EVENTS - 1,
        MAX_JOURNAL_BYTES - 1,
        1
    ));
    assert!(!within_journal_limits(MAX_JOURNAL_EVENTS + 1, 0, 1));
    assert!(!within_journal_limits(
        MAX_JOURNAL_EVENTS - 1,
        MAX_JOURNAL_BYTES,
        1
    ));
    assert!(!within_journal_limits(0, 0, MAX_EVENT_RECORD_BYTES + 1));
}

#[test]
fn removed_atomic_temp_entry_is_skipped_before_metadata_lookup() {
    let root = std::env::temp_dir().join(format!(
        "koolade-journal-temp-entry-{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir(&root).unwrap();
    let name = format!(".{:020}.json.{}-0.koolade.tmp", 1, std::process::id());
    let path = root.join(name);
    fs::write(&path, b"in-progress atomic write").unwrap();
    let entry = fs::read_dir(&root).unwrap().next().unwrap().unwrap();
    fs::remove_file(path).unwrap();

    assert!(inspect_journal_entry(&entry).unwrap().is_none());

    fs::remove_dir(root).unwrap();
}
