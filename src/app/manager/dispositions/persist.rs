use super::*;

pub(crate) fn persist_dispositions(
    project: &mut Project,
    actions: &[ManagerActionV1],
    expected_evidence_hashes: &std::collections::HashMap<String, String>,
    expected_planning_revision: &str,
    code_review_targets: &std::collections::HashMap<
        String,
        crate::app::manager::catalog::code_reviews::FindingTarget,
    >,
) -> Result<Vec<String>, String> {
    let code_review_work = code_reviews::prepare(
        project,
        actions,
        expected_evidence_hashes,
        code_review_targets,
    )?;
    let has_plan_actions = actions.iter().any(is_plan_review_action);
    if !has_plan_actions && code_review_work.is_empty() {
        return Ok(Vec::new());
    }
    let current_revision = project
        .state
        .planning_store
        .revision()
        .map_err(|error| error.to_string())?;
    if current_revision != expected_planning_revision
        || project.state.baseline_planning_revision != expected_planning_revision
    {
        return Ok(vec![
            "Skipped stale Manager dispositions because planning state changed during the Manager run; refresh and review again.".into(),
        ]);
    }
    let reviews = crate::app::manager::catalog::reviews::current_review_evidence(project);
    let current_evidence_hashes = reviews
        .iter()
        .flat_map(|review| std::iter::once(&review.item).chain(review.supporting_items.iter()))
        .map(|item| {
            (
                item.id.clone(),
                format!("{:x}", sha2::Sha256::digest(item.content.as_bytes())),
            )
        })
        .collect::<std::collections::HashMap<_, _>>();
    let original = project.planning_work.clone();
    let mut messages = Vec::new();
    let mut changed = false;
    for action in actions {
        if !is_plan_review_action(action) {
            continue;
        }
        let Some(kind) = DispositionWork::from_action(&action.kind) else {
            continue;
        };
        if action
            .evidence
            .iter()
            .any(|id| expected_evidence_hashes.get(id) != current_evidence_hashes.get(id))
        {
            messages.push(format!(
                "Skipped stale Manager disposition for {}; its cited evidence changed.",
                action.target
            ));
            continue;
        }
        let Some((feature_id, finding_id)) = action.target.split_once('#') else {
            messages.push("Skipped a plan-review disposition with an invalid target.".into());
            continue;
        };
        let Some(review) = reviews.iter().find(|review| {
            action
                .evidence
                .iter()
                .any(|evidence| evidence == &review.id)
                && review.target.feature_id == feature_id
                && review.target.revision == action.expected_revision
                && review.target.finding_ids.iter().any(|id| id == finding_id)
        }) else {
            messages.push(format!(
                "Skipped stale Manager disposition for {feature_id}#{finding_id}; the plan review changed."
            ));
            continue;
        };
        let Some((_, markdown)) = project
            .state
            .active_features
            .iter()
            .find(|(id, _)| id == feature_id)
        else {
            continue;
        };
        let Some(identity) = crate::domain::ArtifactIdentity::from_markdown(markdown)
            .ok()
            .flatten()
        else {
            continue;
        };
        let key = format!(
            "task:manager-plan-{}:{}:{}",
            kind.key(),
            action.expected_revision,
            finding_id
        );
        if project.planning_work.iter().any(|work| work.key == key) {
            continue;
        }
        let verdict: crate::harness::prompt_catalog::ReviewVerdictV1 =
            serde_json::from_str(&review.item.content).map_err(|error| error.to_string())?;
        let finding = verdict
            .findings
            .iter()
            .find(|finding| finding.id == finding_id)
            .ok_or_else(|| format!("validated finding {finding_id} disappeared"))?;
        let reason = crate::app::root::plan_review::verdict::sanitize_text(&action.reason, 2048);
        let title = format!("{}: {}", kind.title(), finding_id);
        let request = format!(
            "{} for {feature_id} based on independent plan review finding {finding_id}. {reason}",
            kind.request()
        );
        let arbitration_detail = if let Some(claim) = action.contradiction.as_ref() {
            let source_quote = fields::encode(&claim.source_quote)?;
            let rationale = fields::encode(&claim.explanation)?;
            let source_hash = expected_evidence_hashes
                .get(&claim.source_evidence_id)
                .map(String::as_str)
                .unwrap_or("missing");
            format!(
                "\nArbitration finding claim: {}\nArbitration evidence: {}\nArbitration evidence SHA-256: {source_hash}\nArbitration source quote: {source_quote}\nArbitration rationale: {rationale}",
                crate::app::root::plan_review::verdict::sanitize_text(&claim.finding_claim, 1024),
                claim.source_evidence_id,
            )
        } else {
            String::new()
        };
        let detail = format!(
            "Manager disposition: {}\nSnapshot hash: {}\nReview evidence: {}\nRequirement: {}\nFinding: {}\nLocation: {}\nCounterexample: {}\nManager rationale: {reason}\nEvidence IDs: {}{}",
            kind.label(),
            action.expected_revision,
            review.id,
            finding.requirement,
            finding.proposed_correction,
            finding.location,
            finding.repro.as_deref().unwrap_or("not provided"),
            action.evidence.join(", "),
            arbitration_detail,
        );
        let mut work = Work::new(String::new(), title, request, detail);
        work.kind = WorkKind::PlanReviewDisposition;
        work.status = kind.status();
        work.feature_id = Some(feature_id.to_owned());
        work.feature_uid = Some(identity.uid);
        work.key = key;
        project.planning_work.push(work);
        changed = true;
    }
    for code_review in code_review_work {
        if let Some(existing) = project
            .planning_work
            .iter_mut()
            .find(|item| item.uid == code_review.uid)
        {
            *existing = code_review;
        } else {
            project.planning_work.push(code_review);
        }
        changed = true;
    }
    if changed && let Err(error) = project.save_planning_work() {
        project.planning_work = original;
        return Err(error);
    }
    Ok(messages)
}
