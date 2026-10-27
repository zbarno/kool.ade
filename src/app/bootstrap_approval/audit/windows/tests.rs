use super::*;
use crate::app::bootstrap_repair_policy::Operation;

#[test]
fn audit_append_replaces_atomically_and_keeps_private_windows_acl() {
    let root = tempfile::tempdir().unwrap();
    let path = root
        .path()
        .join("home")
        .join("machine")
        .join("repair-audit.json");
    for observed_at_unix_ms in 1..=3 {
        append(
            &path,
            "repair-audit.json",
            AuditEntry {
                observed_at_unix_ms,
                operation: Operation::PrepareLockedImage,
                prerequisite_id: "setup:images:general:amd64".into(),
                proposal_sha256: "a".repeat(64),
                binding_sha256: "b".repeat(64),
                outcome: "approved".into(),
            },
        )
        .unwrap();
    }

    let store: AuditStore = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(store.schema_version, VERSION);
    assert_eq!(
        store
            .entries
            .iter()
            .map(|entry| entry.observed_at_unix_ms)
            .collect::<Vec<_>>(),
        vec![1, 2, 3]
    );
    verify_regular_file(&path, &open_existing_file(&path).unwrap()).unwrap();
    verify_regular_file(
        &path.with_file_name("repair-audit.json.lock"),
        &open_existing_file(&path.with_file_name("repair-audit.json.lock")).unwrap(),
    )
    .unwrap();
}
