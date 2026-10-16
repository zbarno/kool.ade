use crate::{
    app::{root::configured_harness_for, session::Project},
    core::lifecycle::{LifecycleStore, SpecSyncQueueEntryV1},
};

pub(super) fn advance_feature_spec_reconciliation(
    project: &mut Project,
    task_harness: &mut Option<Box<dyn crate::harness::AiHarness>>,
) {
    if let Some(controller) = project.activity.feature_spec_reconciliation.as_ref() {
        let Some(result) = controller.take_result() else {
            return;
        };
        let feature_uid = controller.feature_uid.clone();
        let expected_revision = controller.expected_revision;
        let expected_spec_sha256 = controller.spec_sha256.clone();
        let origins = controller.origins.clone();
        project.activity.feature_spec_reconciliation = None;
        finish_feature_spec_reconciliation(
            project,
            feature_uid,
            expected_revision,
            expected_spec_sha256,
            origins,
            result,
        );
        return;
    }

    let lifecycle = LifecycleStore::new(&project.state.planning_store);
    let candidates = match lifecycle.feature_spec_reconciliation_candidates() {
        Ok(candidates) => candidates,
        Err(error) => {
            project.activity.pending.push(format!(
                "Feature specification refresh queue could not be read: {error}"
            ));
            return;
        }
    };
    let Some((feature, origins)) = candidates.into_iter().next() else {
        return;
    };
    if origins
        .iter()
        .any(|origin| origin.request_message_id.is_some())
    {
        project.task_chats.ensure_loaded(&project.chat_slug);
    }
    let harness = configured_harness_for(
        task_harness,
        Some(crate::persistence::harness_settings::PLAN_REVIEW),
    );
    let route = harness.label();
    match crate::app::root::spec_reconciliation::FeatureSpecReconciliation::start(
        project, &feature, &origins, harness,
    ) {
        Ok(controller) => {
            project.activity.feature_spec_reconciliation = Some(controller);
            project.live_progress = crate::harness::LiveProgress {
                activity: Some(format!(
                    "Refreshing the approved feature documentation with {route}…"
                )),
                selected_route: Some(route),
                ..Default::default()
            };
        }
        Err(reason) => finish_feature_spec_reconciliation(
            project,
            feature.feature_uid,
            feature.revision,
            String::new(),
            origins,
            Err(reason),
        ),
    }
}

fn finish_feature_spec_reconciliation(
    project: &mut Project,
    feature_uid: String,
    expected_revision: u64,
    expected_spec_sha256: String,
    origins: Vec<SpecSyncQueueEntryV1>,
    result: Result<crate::app::root::spec_reconciliation::FeatureSpecReconciliationOutcome, String>,
) {
    project.live_progress = Default::default();
    let lifecycle = LifecycleStore::new(&project.state.planning_store);
    let feature = match lifecycle.load_feature(&feature_uid) {
        Ok(feature) => feature,
        Err(error) => {
            project.activity.pending.push(format!(
                "Feature refresh result for {feature_uid} could not be checked: {error}"
            ));
            return;
        }
    };
    let current_hash = match crate::core::lifecycle::product_spec::current_product_hash(
        &project.state.planning_store,
    ) {
        Ok(hash) => hash,
        Err(error) => {
            project.activity.pending.push(format!(
                "Feature refresh result for {feature_uid} could not verify the current specification: {error}"
            ));
            return;
        }
    };
    let stale_reason = (feature.revision != expected_revision)
        .then(|| "The approved feature changed while documentation refresh was running.".to_owned())
        .or_else(|| {
            (!expected_spec_sha256.is_empty() && current_hash != expected_spec_sha256)
                .then(|| "The current product specification changed while documentation refresh was running.".to_owned())
        });
    let (proposal, run_uid, reason) = if let Some(reason) = stale_reason {
        (None, None, Some(reason))
    } else {
        match result {
            Ok(outcome)
                if outcome.feature_uid == feature_uid
                    && outcome.expected_revision == expected_revision
                    && outcome.spec_sha256 == expected_spec_sha256 =>
            {
                (Some(outcome.proposal), Some(outcome.run_uid), None)
            }
            Ok(_) => (
                None,
                None,
                Some("Feature refresh returned a mismatched result.".to_owned()),
            ),
            Err(reason) => (None, None, Some(reason)),
        }
    };
    let proposal_ref = proposal.as_ref();
    if let Err(error) = super::finish::record_feature(
        project,
        &feature,
        &origins,
        proposal_ref,
        run_uid.as_deref(),
        reason.as_deref(),
    ) {
        project.activity.pending.push(format!(
            "Feature specification refresh for {feature_uid} could not be durably recorded: {error}"
        ));
        return;
    }
    let message = match (reason, proposal_ref) {
        (Some(reason), _) => format!(
            "Feature specification refresh for {feature_uid} needs human review: {reason} The approved plan and implementation state were not changed."
        ),
        (None, Some(proposal)) if proposal.deltas.is_empty() => format!(
            "Feature specification refresh for {feature_uid} completed with no proposed documentation or target changes."
        ),
        (None, Some(proposal))
            if proposal.deltas.iter().all(|delta| {
                matches!(
                    &delta.classification,
                    crate::harness::prompt_catalog::DeltaClassification::Documentation
                ) && matches!(
                    &delta.approval_effect,
                    crate::harness::prompt_catalog::ApprovalEffect::DoesNotChangeApproval
                )
            }) =>
        {
            format!(
                "Feature specification refresh for {feature_uid} recorded nonblocking documentation findings."
            )
        }
        (None, Some(_)) => format!(
            "Feature specification refresh for {feature_uid} proposed a target change that needs explicit user review."
        ),
        (None, None) => format!("Feature specification refresh for {feature_uid} was recorded."),
    };
    project.activity.pending.push(message.clone());
    project.remember_chat(vec![crate::domain::chatlog::ChatMessage::new(
        crate::domain::chatlog::ChatRole::System,
        message,
        None,
    )]);
}
