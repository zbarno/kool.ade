use super::super::*;

#[test]
fn schema_one_store_migrates_its_newest_task_observation() {
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("tasks.json");
    std::fs::write(
        &path,
        r#"{"schemaVersion":1,"tasks":{"setup:git:install":{"id":"setup:git:install","title":"Install Git","explanation":"Git required","nextStep":"Install","prerequisites":[],"state":"needs_attention","observedAtUnixMs":12,"verifiedAtUnixMs":null,"events":[]}}}"#,
    )
    .unwrap();

    let store = load_from(&path).unwrap();

    assert_eq!(store.schema_version, 3);
    assert_eq!(store.last_observed_at_unix_ms, 12);
    assert_eq!(store.last_scan_started_at_unix_ms, 12);
    assert_eq!(store.tasks["setup:git:install"].observed_at_unix_ms, 12);
}

#[test]
fn schema_two_store_migrates_the_completion_watermark_to_scan_ordering() {
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("tasks.json");
    std::fs::write(
        &path,
        r#"{"schemaVersion":2,"lastObservedAtUnixMs":12,"tasks":{}}"#,
    )
    .unwrap();

    let store = load_from(&path).unwrap();

    assert_eq!(store.schema_version, 3);
    assert_eq!(store.last_observed_at_unix_ms, 12);
    assert_eq!(store.last_scan_started_at_unix_ms, 12);
}
