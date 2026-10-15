use super::{snapshot_matches_pinned_history, valid_private_ref};

fn recovered_snapshot() -> serde_json::Value {
    serde_json::json!({
        "phase": "recovered_once",
        "branch": "koolade/task/example",
        "base": "remote-commit",
        "target": "local-commit",
        "merge_head": "local-commit",
        "private_ref": "refs/koolade/reconciliation-recovery/task-key/nonce",
    })
}

#[test]
fn recovery_snapshot_must_match_its_pinned_histories() {
    let mut saved = recovered_snapshot();
    let matches = |snapshot: &serde_json::Value| {
        snapshot_matches_pinned_history(
            snapshot,
            "koolade/task/example",
            "remote-commit",
            "local-commit",
            "local-commit",
        )
    };

    assert!(matches(&saved));
    saved["phase"] = serde_json::json!("snapshot_saved");
    assert!(!matches(&saved));

    saved = recovered_snapshot();
    saved["target"] = serde_json::json!("different-commit");
    assert!(!matches(&saved));
}

#[test]
fn recovery_snapshot_private_ref_must_belong_to_the_task() {
    let mut saved = recovered_snapshot();
    assert_eq!(
        valid_private_ref(&saved, "task-key"),
        Some("refs/koolade/reconciliation-recovery/task-key/nonce")
    );

    saved["private_ref"] = serde_json::json!("refs/koolade/reconciliation-recovery/other/nonce");
    assert_eq!(valid_private_ref(&saved, "task-key"), None);
}
