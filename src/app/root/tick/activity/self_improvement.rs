use crate::{
    app::session::Project,
    core::{
        implementation::TaskRepositoryKind,
        lifecycle::{
            FeatureEvent, FeaturePhase, GateActor, GateDecision, LifecycleEventKind, LifecycleStore,
        },
        opportunity::{OpportunityDecisionKindV1, OpportunityStore, OpportunityTargetKindV1},
        self_improvement::{
            SelfImprovementEventV1 as Event, SelfImprovementPhaseV1 as Phase, SelfImprovementStore,
        },
    },
};
mod pr_evidence;
mod task_evidence;
use task_evidence::implementation_for;
#[cfg(test)]
#[path = "self_improvement/tests.rs"]
mod tests;

pub(super) fn synchronize(project: &mut Project) {
    if project.active_turn.is_some()
        || project
            .activity
            .last_self_improvement_sync
            .is_some_and(|last| last.elapsed() < std::time::Duration::from_secs(1))
    {
        return;
    }
    project.activity.last_self_improvement_sync = Some(std::time::Instant::now());

    let planning = project.state.planning_store.clone();
    let opportunities = OpportunityStore::new(&planning);
    let runs = SelfImprovementStore::new(&planning);
    let proposals = match opportunities.list() {
        Ok(proposals) => proposals,
        Err(error) => {
            warn_once(
                project,
                format!("Could not synchronize self-improvement runs: {error}"),
            );
            return;
        }
    };
    let lifecycle = LifecycleStore::new(&planning);
    for proposal in proposals {
        if proposal.target.kind != OpportunityTargetKindV1::KooladeProduct {
            continue;
        }
        let decision = match opportunities.decision_for_opportunity(&proposal.opportunity_uid) {
            Ok(Some(decision)) if decision.decision == OpportunityDecisionKindV1::Pursue => {
                decision
            }
            Ok(_) => continue,
            Err(error) => {
                warn_once(
                    project,
                    format!("Could not read self-improvement decision: {error}"),
                );
                continue;
            }
        };
        let run_uid = match SelfImprovementStore::run_uid_for_decision(&decision.uid) {
            Ok(uid) => uid,
            Err(error) => {
                warn_once(project, error.to_string());
                continue;
            }
        };
        let Ok(run) = runs.load(&run_uid) else {
            continue;
        };
        if !proposal.related_feature_uids.contains(&run.feature_uid) {
            continue;
        }
        if let Err(error) = synchronize_run(project, &lifecycle, &runs, &run_uid) {
            warn_once(
                project,
                format!("Self-improvement progress is waiting for lifecycle evidence: {error}"),
            );
        }
    }
    if let Ok(revision) = project.state.planning_store.revision() {
        project.state.baseline_planning_revision = revision;
    }
}

fn synchronize_run(
    project: &mut Project,
    lifecycle: &LifecycleStore<'_>,
    runs: &SelfImprovementStore<'_>,
    run_uid: &str,
) -> Result<(), String> {
    let mut run = runs.load(run_uid).map_err(|error| error.to_string())?;
    if run.phase == Phase::Planning {
        let feature_path = format!("state/lifecycle_features/{}.json", run.feature_uid);
        let feature_exists = project
            .state
            .planning_store
            .list_record_paths("state/lifecycle_features")
            .map_err(|error| error.to_string())?
            .iter()
            .any(|path| path == &feature_path);
        if !feature_exists {
            return Ok(());
        }
    }
    let feature = lifecycle
        .load_feature(&run.feature_uid)
        .map_err(|error| error.to_string())?;
    if feature.feature_uid != run.feature_uid {
        return Err("lifecycle feature identity differs from the pursued feature".into());
    }
    if feature.phase == FeaturePhase::Cancelled
        && !matches!(
            run.phase,
            Phase::PrOpened | Phase::MergedNotActivated | Phase::Activated | Phase::Cancelled
        )
    {
        apply(runs, &mut run, "cancel", Event::Cancel)?;
    }

    if run.phase == Phase::Planning {
        let Some(plan_hash) = feature.approved_plan_sha256.as_ref() else {
            return Ok(());
        };
        let Some(targets) = project
            .state
            .workflow
            .feature_branch_targets
            .get(&run.feature_uid)
        else {
            return Ok(());
        };
        let records = lifecycle
            .events_for_feature(&run.feature_uid)
            .map_err(|error| error.to_string())?;
        let Some((_user, review)) = reviewed_plan_receipts(&feature, &records, plan_hash) else {
            return Ok(());
        };
        if feature.plan_snapshot_sha256.as_deref() != Some(plan_hash)
            || feature.task_uids.is_empty()
        {
            return Ok(());
        }
        let tasks = feature
            .task_uids
            .iter()
            .map(|uid| lifecycle.load_task(uid).map_err(|error| error.to_string()))
            .collect::<Result<Vec<_>, _>>()?;
        if tasks.iter().any(|task| {
            task.feature_uid != run.feature_uid || task.approved_plan_sha256 != *plan_hash
        }) {
            return Err("task identities do not belong to the exact reviewed plan".into());
        }
        let roots = tasks
            .iter()
            .filter(|task| task.parent_task_uid.is_none())
            .collect::<Vec<_>>();
        let Some(owner) = roots.first() else {
            return Ok(());
        };
        apply(
            runs,
            &mut run,
            &format!("approved:{plan_hash}"),
            Event::PlanApproved {
                approval_sha256: plan_hash.to_owned(),
                review_uid: review.receipt_uid.clone(),
                source_ref: targets.source.to_owned(),
                destination_ref: targets.destination.to_owned(),
                task_uids: tasks.iter().map(|task| task.task_uid.clone()).collect(),
                owner_task_uid: owner.task_uid.clone(),
                parent_task_uids: roots.iter().map(|task| task.task_uid.clone()).collect(),
            },
        )?;
    }

    if run.phase == Phase::Ready {
        let Some(owner) = run.owner_task_uid.as_deref() else {
            return Ok(());
        };
        let task = lifecycle
            .load_task(owner)
            .map_err(|error| error.to_string())?;
        if let Some(implementation) = implementation_for(project, &task)
            && implementation.task_repository_kind == TaskRepositoryKind::Clone
            && implementation.task_repository_ready
            && implementation.source_commit.as_deref() == Some(run.source_sha.as_str())
            && implementation.repository_id.as_deref() == Some(run.target_repository_uid.as_str())
            && implementation.source_branch.as_deref() == run.source_ref.as_deref()
            && implementation.destination_branch.as_deref() == run.destination_ref.as_deref()
            && implementation.repository_identity.is_some()
        {
            let clone_uid = uuid::Uuid::new_v5(
                &uuid::Uuid::parse_str(&run.run_uid).map_err(|error| error.to_string())?,
                format!(
                    "{}:{}",
                    task.task_uid,
                    implementation
                        .repository_identity
                        .as_deref()
                        .unwrap_or_default()
                )
                .as_bytes(),
            )
            .hyphenated()
            .to_string();
            apply(
                runs,
                &mut run,
                &format!("clone:{clone_uid}"),
                Event::CloneAllocated { clone_uid },
            )?;
        }
    }

    task_evidence::synchronize(project, lifecycle, runs, &mut run)?;
    if run.phase == Phase::MergedNotActivated {
        runs.record_active_build_probe(&run.run_uid, run.revision)
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn apply(
    store: &SelfImprovementStore<'_>,
    run: &mut crate::core::self_improvement::SelfImprovementRunV1,
    evidence_key: &str,
    event: Event,
) -> Result<(), String> {
    let uid = transition_uid(&run.run_uid, evidence_key)?;
    *run = store
        .apply_event(&run.run_uid, run.revision, &uid, event)
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn transition_uid(run_uid: &str, evidence_key: &str) -> Result<String, String> {
    let namespace = uuid::Uuid::parse_str(run_uid).map_err(|error| error.to_string())?;
    Ok(uuid::Uuid::new_v5(&namespace, evidence_key.as_bytes())
        .hyphenated()
        .to_string())
}

fn reviewed_plan_receipts<'a>(
    feature: &crate::core::lifecycle::FeatureGoal,
    records: &'a [crate::core::lifecycle::LifecycleEventRecord],
    plan_hash: &str,
) -> Option<(
    &'a crate::core::lifecycle::GateReceipt,
    &'a crate::core::lifecycle::GateReceipt,
)> {
    let user = records.iter().find_map(|record| match &record.event {
        LifecycleEventKind::Feature(FeatureEvent::UserPlanDecision(receipt))
            if record.actor == crate::core::lifecycle::LifecycleActor::User
                && receipt.actor == GateActor::User
                && receipt.decision == GateDecision::Approved
                && receipt.subject_uid
                    == feature.plan_snapshot_uid.as_deref().unwrap_or_default()
                && receipt.subject_sha256 == plan_hash
                && feature.decision_refs.len() == 1
                && feature.decision_refs.first() == Some(&receipt.receipt_uid) =>
        {
            Some(receipt)
        }
        _ => None,
    })?;
    let review = records.iter().find_map(|record| match &record.event {
        LifecycleEventKind::Feature(FeatureEvent::PlanReview(receipt))
            if record.actor == crate::core::lifecycle::LifecycleActor::IndependentReviewer
                && receipt.actor == GateActor::IndependentReviewer
                && receipt.decision == GateDecision::Approved
                && receipt.subject_uid
                    == feature.plan_snapshot_uid.as_deref().unwrap_or_default()
                && receipt.subject_sha256 == plan_hash
                && receipt.prompt_role()
                    == Some(crate::harness::prompt_catalog::PromptRole::PlanReviewer)
                && feature.review_refs.len() == 1
                && feature.review_refs.first() == Some(&receipt.receipt_uid) =>
        {
            Some(receipt)
        }
        _ => None,
    })?;
    Some((user, review))
}

fn warn_once(project: &mut Project, message: String) {
    if !project
        .activity
        .pending
        .iter()
        .any(|existing| existing == &message)
    {
        project.activity.pending.push(message);
    }
}
