use crate::app::session::Project;
use crate::core::self_improvement::{
    SelfImprovementEventV1 as Event, SelfImprovementPhaseV1 as Phase, SelfImprovementStore,
};

pub(super) fn retry_evolve_revocation_for_feature_uid(project: &mut Project, feature_uid: &str) {
    let prefix = format!(
        "Work is cancelled, but self-improvement authorization for feature {feature_uid} could not be revoked:"
    );
    record_revocation(project, &prefix, |project| {
        cancel_self_improvement_run_for_feature(
            project,
            feature_uid,
            &format!("user-cancelled-feature:{feature_uid}"),
        )
    });
}

pub(super) fn retry_evolve_revocation_for_feature(project: &mut Project, feature_id: &str) {
    let tickets = project
        .task_documents
        .iter()
        .filter(|document| {
            document
                .text
                .lines()
                .any(|line| line.strip_prefix("Feature ID: ") == Some(feature_id))
        })
        .map(|document| document.path.clone())
        .collect::<Vec<_>>();
    retry_evolve_revocation(project, &tickets);
}

pub(super) fn retry_evolve_revocation(project: &mut Project, tickets: &[String]) {
    for ticket in tickets {
        let prefix = format!(
            "Work is cancelled, but self-improvement authorization for {ticket} could not be revoked:"
        );
        record_revocation(project, &prefix, |project| {
            cancel_self_improvement_run(project, ticket)
        });
    }
}

fn record_revocation(
    project: &mut Project,
    prefix: &str,
    revoke: impl FnOnce(&mut Project) -> Result<(), String>,
) {
    match revoke(project) {
        Ok(()) => project
            .activity
            .pending
            .retain(|existing| !existing.starts_with(prefix)),
        Err(error) => {
            let warning = format!("{prefix} {error}");
            if !project
                .activity
                .pending
                .iter()
                .any(|existing| existing == &warning)
            {
                project.activity.pending.push(warning);
            }
        }
    }
}

fn cancel_self_improvement_run(project: &mut Project, ticket: &str) -> Result<(), String> {
    let Some(document) = project
        .task_documents
        .iter()
        .find(|document| document.path == ticket)
    else {
        return Ok(());
    };
    let Some(planning_item_uid) = document
        .metadata
        .as_ref()
        .map(|metadata| metadata.uid.as_str())
        .or_else(|| {
            document
                .identity
                .as_ref()
                .map(|identity| identity.uid.as_str())
        })
    else {
        return Ok(());
    };
    let planning = &project.state.planning_store;
    let lifecycle = crate::core::lifecycle::LifecycleStore::new(planning);
    let task = lifecycle
        .task_for_planning_item(planning_item_uid)
        .map_err(|error| error.to_string())?;
    let Some(task) = task else { return Ok(()) };
    cancel_self_improvement_run_for_feature(
        project,
        &task.feature_uid,
        &format!("user-cancelled-task:{}", task.task_uid),
    )
}

fn cancel_self_improvement_run_for_feature(
    project: &mut Project,
    feature_uid: &str,
    event_key: &str,
) -> Result<(), String> {
    let planning = &project.state.planning_store;
    let runs = SelfImprovementStore::new(planning);
    let Some(run) = runs
        .run_for_feature(feature_uid)
        .map_err(|error| error.to_string())?
    else {
        return Ok(());
    };
    if matches!(
        run.phase,
        Phase::PrOpened
            | Phase::PrClosedUnmerged
            | Phase::MergedNotActivated
            | Phase::InstalledNotActivated
            | Phase::Activated
            | Phase::Cancelled
    ) {
        return Ok(());
    }
    let namespace = uuid::Uuid::parse_str(&run.run_uid).map_err(|error| error.to_string())?;
    let event_uid = uuid::Uuid::new_v5(&namespace, event_key.as_bytes())
        .hyphenated()
        .to_string();
    runs.apply_event(&run.run_uid, run.revision, &event_uid, Event::Cancel)
        .map_err(|error| error.to_string())?;
    project.state.baseline_planning_revision =
        planning.revision().map_err(|error| error.to_string())?;
    Ok(())
}
