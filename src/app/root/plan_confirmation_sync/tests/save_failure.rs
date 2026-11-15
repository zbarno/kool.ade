use super::*;

/// Production boundary: failed writes for ready and unready plan cards remain
/// visible and retry until one canonical card is durable; an unready card also
/// emits its evaluation request only after the prompt claim is saved. Reducer
/// tests cannot prove the app retries persistence or preserves prompt delivery.
/// Uses local private-store fixtures and an unwritable synthetic path; no
/// provider, network, or Docker; low cost.
#[test]
fn failed_card_save_is_visible_and_retries_after_store_recovers() {
    for (tag, ready) in [("ready", true), ("unready", false)] {
        let repo = repo(&format!("save-failure-{tag}"));
        let mut project = seed(&repo, &feature_uid(), ready);
        let durable_root = project.state.planning_store.root.clone();
        project.state.planning_store.root = std::path::PathBuf::from("/proc/self");

        let failed = sync(&mut project);
        assert_eq!(failed.evaluations_wanted.len(), 0);
        assert_eq!(failed.announcements.len(), 1);
        assert!(failed.announcements[0].contains("Couldn't save the plan review card"));
        assert!(project.activity.pending_planning_work);

        project.state.planning_store.root = durable_root;
        let retried = sync(&mut project);
        assert!(!project.activity.pending_planning_work);
        assert!(retried.announcements.len() <= 1);
        assert_eq!(
            retried.evaluations_wanted.len(),
            usize::from(!ready),
            "only an unready plan requests an evaluation after saving"
        );
        assert_eq!(
            project
                .planning_work
                .iter()
                .filter(|work| work.kind == WorkKind::PlanConfirmation)
                .count(),
            1,
            "retry persists one canonical review action"
        );

        let mut reloaded = project_from(&repo.root);
        reloaded.planning_work = crate::core::planning_work::load(&reloaded.state.planning_store)
            .expect("retried plan action survives private-store reload");
        assert_eq!(
            reloaded
                .planning_work
                .iter()
                .filter(|work| work.kind == WorkKind::PlanConfirmation)
                .count(),
            1
        );
        let payload = the_card(&reloaded)
            .plan_confirmation
            .as_ref()
            .expect("prompt receipt reloads with the action");
        assert_eq!(
            payload.evaluation_prompted_for.as_deref(),
            (!ready).then_some(payload.snapshot_spec_revision.as_str())
        );
        remove_planning_store_for(&repo.root);
    }
}
