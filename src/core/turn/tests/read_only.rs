//! Production turn pipeline boundary: clarification must never apply model edits.
//! A mocked harness is necessary to return an otherwise writable envelope through
//! the real worker; a validator-only test cannot establish the bypass. Requires
//! local Git and a temporary repository; no provider, network, Docker, or SSH.
//! Setup cost: one temporary Git repository and one local worker thread.
use super::*;

#[test]
fn clarification_keeps_repository_unchanged_even_when_the_model_returns_edits() {
    let (inputs, dir) = inputs_for("read_only_clarification", "What would this change do?");
    git_stdout(&dir, &["add", "-A"]);
    git_stdout(
        &dir,
        &[
            "commit",
            "--allow-empty",
            "-m",
            "Synthetic clarification baseline",
        ],
    );
    let before_head = git_stdout(&dir, &["rev-parse", "HEAD"]);
    let before_status = git_stdout(&dir, &["status", "--porcelain"]);
    let envelope: TurnEnvelope = serde_json::from_value(serde_json::json!({
        "schema_version": 2,
        "assistant_message": "It changes how the project is planned.",
        "change_summary": "This edit must never be applied",
        "document_updates": vision_update("An unauthorized replacement"),
        "open_items_added": [], "open_items_updated": [], "open_items_resolved": []
    }))
    .unwrap();
    let controller = TurnController::start_read_only(
        inputs,
        Box::new(ScriptedHarness {
            canned: Some(envelope),
            raw: None,
        }),
        None,
    );
    assert!(matches!(drain(&controller), TurnOutcome::Discussion { .. }));
    assert_eq!(git_stdout(&dir, &["rev-parse", "HEAD"]), before_head);
    assert_eq!(git_stdout(&dir, &["status", "--porcelain"]), before_status);
    let _ = std::fs::remove_dir_all(dir);
}
