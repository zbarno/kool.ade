mod support;
use crate::core::planning_sync::{PlanningConflictChoice, PlanningSyncState};
use support::*;

#[test]
fn status_fixtures_render_all_sync_states_and_never_hide_auth_export() {
    for (state, expected) in [
        (PlanningSyncState::Local, "Local"),
        (PlanningSyncState::Synced, "Synced"),
        (PlanningSyncState::Pending, "Pending 1"),
        (PlanningSyncState::Offline, "Offline"),
        (PlanningSyncState::AuthRequired, "Auth required"),
        (PlanningSyncState::Conflicted, "Conflict 1"),
        (PlanningSyncState::ReadOnly, "Permission required"),
        (PlanningSyncState::IncompatibleSchema, "Incompatible schema"),
    ] {
        let mut probe = Probe::with_state(state.clone());
        probe.view.as_mut().unwrap().sync.detail = Some("fixture status detail".into());
        match state {
            PlanningSyncState::Pending => {
                probe.view.as_mut().unwrap().sync.pending_operations = 1;
            }
            PlanningSyncState::Conflicted => {
                probe.view.as_mut().unwrap().sync.conflicts.push(conflict());
            }
            _ => {}
        }
        let ctx = egui::Context::default();
        let text = render(&ctx, &mut probe, Vec::new()).0;
        assert!(text.contains(expected), "missing {expected}: {text}");
        assert!(text.contains("fixture status detail"));
        assert!(text.contains("Last confirmed sync: 2025-10-09 08:53:20 UTC"));
        assert!(text.contains("workspace-123"));
        assert!(text.contains("Not configured on this machine"));
        if matches!(
            state,
            PlanningSyncState::Pending
                | PlanningSyncState::Offline
                | PlanningSyncState::AuthRequired
                | PlanningSyncState::ReadOnly
        ) {
            assert!(text.contains("Export unsynced changes"));
        }
    }
}

#[test]
fn conflict_modal_shows_both_sides_and_dispatches_semantic_choice() {
    let mut probe = Probe::with_state(PlanningSyncState::Conflicted);
    probe.view.as_mut().unwrap().sync.conflicts.push(conflict());
    let ctx = egui::Context::default();
    let review = button_center(&ctx, &mut probe, "Review conflict…");
    click(&ctx, &mut probe, review);
    let texts = render(&ctx, &mut probe, Vec::new()).0;
    for expected in [
        "task-7",
        "title",
        "local title",
        "remote title",
        "abcdef1234",
        "fedcba9876",
        "123456abcd",
    ] {
        assert!(texts.contains(expected), "missing {expected}: {texts}");
    }
    let pos = button_center(&ctx, &mut probe, "Use local");
    click(&ctx, &mut probe, pos);
    assert_eq!(probe.resolution, Some(PlanningConflictChoice::Local));
}

#[test]
fn conflict_modal_cancel_keeps_the_conflict_unresolved() {
    let mut probe = Probe::with_state(PlanningSyncState::Conflicted);
    probe.view.as_mut().unwrap().sync.conflicts.push(conflict());
    let ctx = egui::Context::default();
    let review = button_center(&ctx, &mut probe, "Review conflict…");
    click(&ctx, &mut probe, review);
    let cancel = button_center(&ctx, &mut probe, "Cancel");
    click(&ctx, &mut probe, cancel);

    assert_eq!(probe.resolution, None);
    assert_eq!(probe.view.as_ref().unwrap().sync.conflicts.len(), 1);
    let texts = render(&ctx, &mut probe, Vec::new()).0;
    assert!(!texts.contains("Resolve planning conflict"));
}

#[test]
fn sync_button_dispatches() {
    let mut probe = Probe::with_state(PlanningSyncState::Synced);
    let ctx = egui::Context::default();
    let pos = button_center(&ctx, &mut probe, "Sync Now");
    click(&ctx, &mut probe, pos);
    assert!(probe.sync_requested);
}
