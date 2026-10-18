use crate::app::session::Project;

#[cfg(test)]
#[path = "lifecycle/tests.rs"]
mod tests;

pub(crate) fn mark_arbitration_failed(project: &mut Project, feature_id: &str, error: &str) {
    let original = project.planning_work.clone();
    if let Some(work) = project.planning_work.iter_mut().find(|work| {
        work.key.starts_with("task:manager-plan-contest:")
            && work.feature_id.as_deref() == Some(feature_id)
            && matches!(
                work.status,
                crate::core::planning_work::WorkStatus::InProgress
                    | crate::core::planning_work::WorkStatus::InReview
            )
    }) {
        work.status = crate::core::planning_work::WorkStatus::NeedsAttention;
        work.detail.push_str(&format!(
            "\n\nArbitration attempt failed: {}",
            crate::app::root::plan_review::verdict::sanitize_text(error, 1024)
        ));
        if arbitration_attempt_count(&work.detail)
            >= crate::app::manager::MAX_PLAN_ARBITRATION_ATTEMPTS
        {
            route_contest_to_repair(work, feature_id);
        }
        if project.save_planning_work().is_err() {
            project.planning_work = original;
        }
    }
}

pub(crate) fn recover_interrupted_arbitration(work: &mut crate::core::planning_work::Work) {
    if !work.key.starts_with("task:manager-plan-contest:")
        || work.status != crate::core::planning_work::WorkStatus::InProgress
    {
        return;
    }
    work.status = crate::core::planning_work::WorkStatus::NeedsAttention;
    work.detail.push_str(
        "\n\nArbitration attempt failed: prior arbitration was interrupted before a result was saved.",
    );
    let feature_id = work.feature_id.clone().unwrap_or_default();
    if arbitration_attempt_count(&work.detail) >= crate::app::manager::MAX_PLAN_ARBITRATION_ATTEMPTS
    {
        route_contest_to_repair(work, &feature_id);
    }
}

pub(super) fn record_plan_arbitration_work(
    project: &mut Project,
    feature_id: &str,
    arbitration: &crate::app::root::plan_review::ArbitrationContext,
    verdict: &crate::harness::prompt_catalog::ReviewVerdictV1,
    receipt: &crate::core::lifecycle::GateReceipt,
) -> Result<(), String> {
    if verdict.outcome == Some(crate::harness::prompt_catalog::ReviewOutcomeV1::ReviewUnable) {
        return Err("ReviewUnable cannot be persisted as an arbitration result".into());
    }
    let original = project.planning_work.clone();
    let snapshot_hash = &receipt.subject_sha256;
    let active_findings = verdict
        .findings
        .iter()
        .map(|finding| finding.id.as_str())
        .collect::<std::collections::HashSet<_>>();
    let mut changed = false;
    for work in project.planning_work.iter_mut().filter(|work| {
        work.key.starts_with("task:manager-plan-contest:")
            && work.feature_id.as_deref() == Some(feature_id)
            && work
                .detail
                .lines()
                .any(|line| line == format!("Snapshot hash: {snapshot_hash}"))
            && work.key.rsplit(':').next() == Some(arbitration.finding_id.as_str())
            && work.status != crate::core::planning_work::WorkStatus::Done
    }) {
        let finding_id = work.key.rsplit(':').next().unwrap_or_default();
        let finding_remains = active_findings.contains(finding_id);
        work.status = if finding_remains {
            crate::core::planning_work::WorkStatus::NeedsAttention
        } else {
            crate::core::planning_work::WorkStatus::Done
        };
        work.detail.push_str(&format!(
            "\n\nIndependent arbitration receipt: {}\nArbitration snapshot hash: {}\nCited evidence: {} ({})\nSource quote: {}\nArbitrator outcome: {}\nContested finding remains: {}",
            receipt.receipt_uid,
            snapshot_hash,
            arbitration.evidence_id,
            arbitration.evidence_sha256,
            arbitration.source_quote,
            if verdict.pass { "approved" } else { "changes requested" },
            finding_remains,
        ));
        if finding_remains
            && arbitration_attempt_count(&work.detail)
                >= crate::app::manager::MAX_PLAN_ARBITRATION_ATTEMPTS
        {
            route_contest_to_repair(work, feature_id);
        }
        changed = true;
    }
    if !changed {
        return Err("the Manager contest card is missing".into());
    }
    if let Err(error) = project.save_planning_work() {
        project.planning_work = original;
        return Err(error);
    }
    Ok(())
}

fn arbitration_attempt_count(detail: &str) -> usize {
    detail
        .lines()
        .filter(|line| {
            line.starts_with("Arbitration attempt failed: ")
                || line.starts_with("Independent arbitration receipt: ")
        })
        .count()
}

fn route_contest_to_repair(work: &mut crate::core::planning_work::Work, feature_id: &str) {
    let finding_id = work.key.rsplit(':').next().unwrap_or("finding").to_owned();
    let snapshot_hash = work
        .detail
        .lines()
        .find_map(|line| line.strip_prefix("Snapshot hash: "))
        .unwrap_or("unknown")
        .to_owned();
    work.key = format!("task:manager-plan-repair:{snapshot_hash}:{finding_id}:arbitration");
    work.title = format!("Manager repair: {finding_id}");
    work.request = format!(
        "Revise the plan for {feature_id} to resolve finding {finding_id}; independent arbitration did not settle it after the bounded attempts. The revised plan requires fresh user confirmation and review."
    );
    work.status = crate::core::planning_work::WorkStatus::Todo;
    work.detail.push_str(
        "\n\nEscalated to a Manager-directed plan repair after the bounded independent arbitration attempts.",
    );
}

pub(super) fn record_reviewed_plan(project: &Project, feature_id: &str) -> Result<(), String> {
    let markdown = project
        .state
        .active_features
        .iter()
        .find(|(id, _)| id == feature_id)
        .map(|(_, markdown)| markdown)
        .ok_or_else(|| "approved feature specification is not loaded".to_owned())?;
    let identity = crate::domain::ArtifactIdentity::from_markdown(markdown)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "approved feature has no stable identity".to_owned())?;
    let contract = crate::core::workflow::feature_contract(markdown);
    let store = &project.state.planning_store;
    let plan = crate::core::lifecycle::plan_snapshot::from_feature_with_author(
        store, feature_id, &identity, markdown, &contract,
    )?;
    if !crate::core::lifecycle::plan_snapshot::is_persisted(store, &plan)
        .map_err(|error| error.to_string())?
    {
        return Err("the reviewed plan snapshot is not persisted".into());
    }
    crate::app::root::feature_approval::readiness::current(store, &plan)?;
    let question_store = crate::core::planning::QuestionStore::new(store);
    let (readiness, _) = question_store
        .load_readiness(&plan.feature_uid)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "readiness evidence is missing".to_owned())?;
    if readiness.spec_revision.as_deref() != Some(plan.spec_revision.as_str()) {
        return Err("readiness evidence is bound to a stale specification".into());
    }
    let mut readiness_refs = readiness.evidence_refs;
    readiness_refs.extend(
        readiness
            .acceptance_tests
            .into_iter()
            .map(|test| test.test_ref),
    );
    readiness_refs.sort();
    readiness_refs.dedup();
    let user_receipt =
        crate::core::lifecycle::plan_snapshot::user_confirmation_receipt(store, &plan)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "the persisted user confirmation receipt is missing".to_owned())?;
    let review_receipt = crate::core::lifecycle::plan_snapshot::plan_review_receipt(store, &plan)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "the persisted independent PlanReviewer receipt is missing".to_owned())?;
    crate::core::lifecycle::LifecycleStore::new(store)
        .reconcile_reviewed_plan(
            &plan,
            markdown,
            readiness_refs,
            &user_receipt,
            &review_receipt,
        )
        .map(|_| ())
        .map_err(|error| error.to_string())
}

/// Close Manager-directed plan repairs only after a fresh, approved review
/// receipt exists for a different candidate snapshot.
pub(crate) fn verify_manager_plan_repairs(
    project: &mut Project,
    feature_id: &str,
    snapshot_hash: &str,
    reviewer_receipt_uid: &str,
) -> Result<bool, String> {
    let original = project.planning_work.clone();
    let mut changed = false;
    for work in &mut project.planning_work {
        if !work.key.starts_with("task:manager-plan-repair:")
            || work.feature_id.as_deref() != Some(feature_id)
            || !matches!(
                work.status,
                crate::core::planning_work::WorkStatus::Todo
                    | crate::core::planning_work::WorkStatus::InProgress
                    | crate::core::planning_work::WorkStatus::InReview
                    | crate::core::planning_work::WorkStatus::NeedsAttention
            )
        {
            continue;
        }
        let prior_hash = work
            .detail
            .lines()
            .find_map(|line| line.strip_prefix("Snapshot hash: "));
        if prior_hash == Some(snapshot_hash) {
            continue;
        }
        work.status = crate::core::planning_work::WorkStatus::Done;
        work.detail.push_str(&format!(
            "\n\nManager verification: the revised plan passed independent review. Snapshot hash: {snapshot_hash}. Reviewer receipt: {reviewer_receipt_uid}."
        ));
        changed = true;
    }
    if changed && let Err(error) = project.save_planning_work() {
        project.planning_work = original;
        return Err(error);
    }
    Ok(changed)
}
