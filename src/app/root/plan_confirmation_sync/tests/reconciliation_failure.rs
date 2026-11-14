/// Production boundary: an app-level reconciliation error leaves the plan
/// card unchanged, surfaces a retry notice, and creates the canonical card
/// after the same activity seam recovers. Reducer tests cannot prove app error
/// handling or prompt suppression. Uses one private store and local Git repo;
/// no provider, network, or Docker; low setup cost.
#[test]
fn reconciliation_error_is_visible_and_retries_after_recovery() {
    let repo = super::repo("reconciliation-failure");
    let mut project = super::seed(&repo, &super::feature_uid(), false);
    let original = project.planning_work.clone();

    let failed = super::super::sync_with_reconciler(&mut project, true, |_, work, _, _| {
        work.push(crate::core::planning_work::Work::new(
            "task:partial-reconcile".into(),
            "Partial reconciliation".into(),
            "Must be discarded on failure".into(),
            "Injected partial mutation".into(),
        ));
        anyhow::bail!("injected reconciliation failure")
    });

    assert_eq!(project.planning_work, original);
    assert!(project.activity.pending_planning_work);
    assert!(failed.evaluations_wanted.is_empty());
    assert_eq!(failed.announcements.len(), 1);
    assert!(failed.announcements[0].contains("retry automatically"));

    let retried = super::sync(&mut project);
    assert!(!project.activity.pending_planning_work);
    assert_eq!(retried.evaluations_wanted.len(), 1);
    assert_eq!(
        project
            .planning_work
            .iter()
            .filter(|work| work.kind == super::WorkKind::PlanConfirmation)
            .count(),
        1,
        "recovery creates one canonical review card"
    );
    let mut reloaded = super::project_from(&repo.root);
    reloaded.planning_work =
        crate::core::planning_work::load(&reloaded.state.planning_store).unwrap();
    assert_eq!(
        reloaded
            .planning_work
            .iter()
            .filter(|work| work.kind == super::WorkKind::PlanConfirmation)
            .count(),
        1
    );
    assert_eq!(
        retried.evaluations_wanted[0].work_key,
        reloaded
            .planning_work
            .iter()
            .find(|work| work.kind == super::WorkKind::PlanConfirmation)
            .unwrap()
            .key
    );
    super::remove_planning_store_for(&repo.root);
}
