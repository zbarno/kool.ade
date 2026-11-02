use super::node_status_text;

#[test]
fn missing_and_unknown_statuses_never_look_completed() {
    for status in ["Unknown", "Unknown legacy state", "Status not recorded"] {
        let rendered = node_status_text(status);
        assert!(rendered.contains(status) || rendered == "? Status not recorded");
        assert!(
            !rendered.contains("✓"),
            "unexpected completion cue: {rendered}"
        );
        assert!(
            !rendered.contains("Completed"),
            "unexpected completion label: {rendered}"
        );
    }
}

#[test]
fn relationship_statuses_keep_exceptions_visible_without_color() {
    assert_eq!(node_status_text("Running"), "● Running");
    assert_eq!(node_status_text("Blocked"), "⚠ Blocked");
    assert_eq!(
        node_status_text("Waiting for your decision"),
        "✋ Needs user input"
    );
    assert_eq!(node_status_text("Cancelled"), "× Cancelled");
    assert_eq!(node_status_text("Failed"), "! Failed");
    assert_eq!(node_status_text("Resolved"), "✓ Completed");
    assert_eq!(node_status_text("Answer saved"), "◷ Answer saved");
    assert_eq!(
        node_status_text("Implementation complete · PR awaiting authorization"),
        "◷ Implementation complete · PR awaiting authorization"
    );
    assert_eq!(
        node_status_text("Implementation complete · PR merged"),
        "✓ Implementation complete · PR merged"
    );
    assert_eq!(node_status_text("Waiting for review"), "◷ Awaiting review");
    assert_eq!(
        node_status_text("Waiting for capacity"),
        "… Waiting for capacity"
    );
}
