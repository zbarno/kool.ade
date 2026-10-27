use super::Snapshot;

fn record(mode: &str, stage: u8, path: &str, object: &str) -> Vec<u8> {
    format!("{mode} {object} {stage}\t{path}\0").into_bytes()
}

#[test]
fn parser_rejects_conflicts_and_unsupported_index_modes() {
    let object = "0123456789abcdef0123456789abcdef01234567";
    assert!(Snapshot::parse(record("100644", 2, "conflict.txt", object)).is_err());
    assert!(Snapshot::parse(record("120000", 0, "link", object)).is_err());
    assert!(Snapshot::parse(record("160000", 0, "submodule", object)).is_err());
}

#[test]
fn case_only_index_mismatches_are_rejected() {
    let object = "0123456789abcdef0123456789abcdef01234567";
    let index = Snapshot::parse(record("100755", 0, "Dir/Tool.sh", object)).unwrap();
    assert!(
        index
            .validate_captured_paths(&["dir/tool.sh".to_owned()], None)
            .is_err()
    );
    let scoped = Snapshot::parse(record("100755", 0, "Selected/runner.sh", object)).unwrap();
    assert!(
        scoped
            .validate_captured_paths(&[], Some(std::path::Path::new("selected")))
            .is_err()
    );
}

#[test]
fn index_snapshot_changes_are_rejected() {
    let object = "0123456789abcdef0123456789abcdef01234567";
    let before = Snapshot::parse(record("100644", 0, "run.sh", object)).unwrap();
    let after = Snapshot::parse(record("100755", 0, "run.sh", object)).unwrap();
    assert!(before.ensure_unchanged(&after).is_err());
}

#[test]
fn supported_modes_map_to_expected_executable_state() {
    let object = "0123456789abcdef0123456789abcdef01234567";
    let mut raw = record("100644", 0, "plain.txt", object);
    raw.extend(record("100755", 0, "run.sh", object));
    let index = Snapshot::parse(raw).unwrap();
    assert!(!index.executable_for(std::path::Path::new("plain.txt"), None));
    assert!(index.executable_for(std::path::Path::new("run.sh"), None));
    assert!(!index.executable_for(
        std::path::Path::new("run.sh"),
        Some(std::path::Path::new("tools"))
    ));
}
