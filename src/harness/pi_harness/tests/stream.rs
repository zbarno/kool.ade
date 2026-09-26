use super::super::*;

#[test]
fn completed_event_log_is_lossless_and_raw_log_is_removed_only_after_archive() {
    if std::process::Command::new("gzip")
        .arg("--version")
        .output()
        .is_err()
    {
        return;
    }
    let dir = std::env::temp_dir().join(format!(
        "packet-event-archive-{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let raw = dir.join("events.jsonl");
    let contents = "{\"type\":\"message_update\"}\n".repeat(100);
    std::fs::write(&raw, &contents).unwrap();
    compress_completed_events(&raw).unwrap();
    assert!(!raw.exists());
    let archived = raw.with_extension("jsonl.gz");
    let decoded = std::process::Command::new("gzip")
        .arg("-dc")
        .arg(&archived)
        .output()
        .unwrap();
    assert!(decoded.status.success());
    assert_eq!(decoded.stdout, contents.as_bytes());
    std::fs::remove_dir_all(dir).unwrap();
}
