use super::*;

/// `base` minus `ago` seconds.
fn ago(base: Instant, ago: u64) -> Instant {
    base - Duration::from_secs(ago)
}

#[test]
fn patrol_note_gate_requires_silence_respects_cooldown() {
    let now = Instant::now();
    // No live work -> never.
    assert!(!patrol_note_due(0, 0, None, None, now));
    // Silent-with-history and live work -> due.
    assert!(patrol_note_due(1, 0, None, None, now));
    // An unsurfaced pending event suppresses a duplicate note.
    assert!(!patrol_note_due(1, 1, None, None, now));
    // Fresh patrol start (last_update is the patrol-spacing clock) -> wait.
    assert!(!patrol_note_due(1, 0, Some(ago(now, 60)), None, now));
    assert!(patrol_note_due(1, 0, Some(ago(now, 121)), None, now));
    // Cooldown after a surfaced note (anti self-fed-loop cap).
    assert!(!patrol_note_due(
        1,
        0,
        Some(ago(now, 3_600)),
        Some(ago(now, 60)),
        now
    ));
    assert!(patrol_note_due(
        1,
        0,
        Some(ago(now, 3_600)),
        Some(ago(now, 1_801)),
        now
    ));
}

#[test]
fn patrol_manager_gate_watches_events_independently_of_auto_plan() {
    let now = Instant::now();
    // Busy slots block the auto start.
    assert!(!patrol_manager_due(true, false, 1, None, now));
    assert!(!patrol_manager_due(false, true, 1, None, now));
    // Nothing pending -> nothing to review.
    assert!(!patrol_manager_due(false, false, 0, None, now));
    // Pending with never-patrolled history -> immediately due (legacy).
    // Auto Plan may be off; manager updates are read-only and still run.
    assert!(patrol_manager_due(false, false, 1, None, now));
    // Within 30 s of the previous patrol start -> spacing holds.
    assert!(!patrol_manager_due(
        false,
        false,
        1,
        Some(ago(now, 20)),
        now
    ));
    assert!(patrol_manager_due(false, false, 1, Some(ago(now, 31)), now));
    // Many queued events do not change pacing.
    assert!(patrol_manager_due(false, false, 9, Some(ago(now, 31)), now));
}

#[test]
fn patrol_cascade_note_feeds_manager_same_tick_then_spaces_out() {
    let now = Instant::now();
    // Silently stuck queue: the SYNTHETIC note gate opens...
    assert!(patrol_note_due(2, 0, Some(ago(now, 600)), None, now));
    // ...and the event it surfaces satisfies the manager gate the same
    // tick (pending 0 -> 1), assuming a prior patrol spaced > 30 s back.
    assert!(patrol_manager_due(false, false, 1, Some(ago(now, 31)), now));
    // Patrol start stamps last_update (the only writer): the next
    // manager round must respect the 30 s spacing...
    assert!(!patrol_manager_due(false, false, 1, Some(now), now));
    assert!(patrol_manager_due(false, false, 1, Some(ago(now, 31)), now));
    // ...and the next SYNTHETIC note respects the 30 min cap even while
    // the queue stays stuck and pending keeps accumulating.
    assert!(!patrol_note_due(
        2,
        0,
        Some(ago(now, 10_000)),
        Some(ago(now, 1_000)),
        now
    ));
    assert!(patrol_note_due(
        2,
        0,
        Some(ago(now, 10_000)),
        Some(ago(now, 1_801)),
        now
    ));
}

#[test]
fn dirty_ticket_tracking_collects_once_per_flush() {
    let mut activity = WorkspaceActivity::default();
    assert_eq!(activity.take_dirty_tickets(), Vec::<String>::new());
    activity.mark_ticket_dirty("001-a");
    activity.mark_ticket_dirty("002-b");
    activity.mark_ticket_dirty("001-a");
    let flushed = activity.take_dirty_tickets();
    assert_eq!(flushed, vec!["001-a", "002-b"]);
    // Flushed set empties; later dirtiness survives independently.
    assert_eq!(activity.take_dirty_tickets(), Vec::<String>::new());
    activity.mark_ticket_dirty("003-c");
    assert_eq!(activity.take_dirty_tickets(), vec!["003-c"]);
}
