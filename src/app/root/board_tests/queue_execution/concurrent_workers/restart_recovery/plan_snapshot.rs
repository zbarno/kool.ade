use super::*;

pub(super) struct StalePlan {
    pub(super) feature_uid: String,
    pub(super) feature_id: String,
    pub(super) path: std::path::PathBuf,
    pub(super) updated_body: String,
    pub(super) old_hash: String,
    pub(super) old_spec_revision: String,
}

pub(super) fn seed_feature_file(repo: &std::path::Path) -> StalePlan {
    let body = crate::test_support::current_change_fixture(
        "# CHG-581-RESTART: Restart plan snapshot\n\n## Intent\n\nKeep a plan snapshot bound to its source specification.\n\n## Current Behavior\n\nThe approval action records the current plan.\n\n## Desired Behavior\n\nA stale plan is refreshed after reconnect.\n\n## Scope\n\n- Refresh a stale plan snapshot\n\n## Affected Product Areas\n\nPlanning.\n\n## Requirements\n\n- REQ-RESTART: Keep the plan hash current.\n\n## Decisions and Assumptions\n\nNo unresolved decisions.\n\n## Acceptance Criteria\n\nA stale plan is refreshed after reconnect.\n",
        crate::domain::ChangeStatus::Draft,
    )
    .unwrap();
    let identity = crate::domain::ArtifactIdentity::from_markdown(&body)
        .unwrap()
        .unwrap();
    let feature_id = identity.display_id.clone();
    let path = repo
        .join("docs/koolade/features")
        .join(format!("{}.md", identity.uid));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, &body).unwrap();
    super::git(repo, &["add", path.to_str().unwrap()]);
    super::git(
        repo,
        &[
            "-c",
            "user.name=Recovery Test",
            "-c",
            "user.email=recovery@example.test",
            "commit",
            "-m",
            "seed restart plan",
        ],
    );
    StalePlan {
        feature_uid: identity.uid,
        feature_id,
        path,
        updated_body: body.replace(
            "A stale plan is refreshed after reconnect.",
            "A stale plan is refreshed after reconnect without changing its approved intent.",
        ),
        old_hash: String::new(),
        old_spec_revision: String::new(),
    }
}

pub(super) fn persist_card(project: &mut crate::app::session::Project, plan: &mut StalePlan) {
    let mut root = Work::new(
        format!("planning:restart-plan-root:{}", plan.feature_uid),
        "Restart plan snapshot goal".into(),
        "Keep review bound to the current feature snapshot.".into(),
        "Planning is waiting for review.".into(),
    );
    root.kind = WorkKind::Feature;
    root.status = WorkStatus::InProgress;
    root.feature_id = Some(plan.feature_id.clone());
    root.feature_uid = Some(plan.feature_uid.clone());
    root.task_intent_uid = Some(root.uid.clone());
    project.planning_work.push(root);
    crate::app::root::plan_confirmation_sync::sync(project);
    let card = project
        .planning_work
        .iter()
        .find(|work| {
            work.kind == WorkKind::PlanConfirmation
                && work.feature_uid.as_deref() == Some(plan.feature_uid.as_str())
        })
        .expect("plan sync creates the canonical review action");
    let payload = card.plan_confirmation.as_ref().unwrap();
    plan.old_hash = payload.snapshot_content_hash.clone();
    plan.old_spec_revision = payload.snapshot_spec_revision.clone();
}

pub(super) fn refresh_after_restart(project: &mut crate::app::session::Project, plan: &StalePlan) {
    crate::app::root::plan_confirmation_sync::sync(project);
    let card = project
        .planning_work
        .iter()
        .find(|work| {
            work.kind == WorkKind::PlanConfirmation
                && work.feature_uid.as_deref() == Some(plan.feature_uid.as_str())
        })
        .expect("restarted plan action remains present");
    let payload = card.plan_confirmation.as_ref().unwrap();
    let identity = crate::domain::ArtifactIdentity::from_markdown(&plan.updated_body)
        .unwrap()
        .unwrap();
    let snapshot = crate::core::lifecycle::plan_snapshot::from_feature_with_author(
        &project.state.planning_store,
        &plan.feature_id,
        &identity,
        &plan.updated_body,
        &crate::core::workflow::feature_contract(&plan.updated_body),
    )
    .unwrap();
    assert_ne!(payload.snapshot_content_hash, plan.old_hash);
    assert_ne!(payload.snapshot_spec_revision, plan.old_spec_revision);
    assert_eq!(payload.snapshot_content_hash, snapshot.content_hash);
    assert_eq!(payload.snapshot_spec_revision, snapshot.spec_revision);
    assert_eq!(card.status, WorkStatus::NeedsAttention);
    assert_eq!(
        project
            .planning_work
            .iter()
            .filter(|work| {
                work.kind == WorkKind::PlanConfirmation
                    && work.feature_uid.as_deref() == Some(plan.feature_uid.as_str())
            })
            .count(),
        1,
        "reconnect updates the existing plan action instead of duplicating it"
    );
}
