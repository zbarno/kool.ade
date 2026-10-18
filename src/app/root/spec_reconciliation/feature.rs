use super::evidence_item;
use crate::{
    app::session::Project,
    core::lifecycle::{FeatureGoal, SpecSyncQueueEntryV1},
    harness::{ExecutionMode, LiveProgress, PlanningRequest},
};
use std::sync::{Arc, atomic::AtomicBool, mpsc};

pub(super) fn prepare(
    project: &Project,
    feature: &FeatureGoal,
    origins: &[SpecSyncQueueEntryV1],
) -> Result<
    (
        PlanningRequest,
        crate::harness::prompt_catalog::OutputValidationContext<'static>,
        String,
    ),
    String,
> {
    use crate::harness::prompt_catalog::{PromptRole, RoleInputV1, RolePayloadV1};

    if origins.is_empty()
        || origins.iter().any(|entry| {
            entry.feature_uid != feature.feature_uid || entry.source_task_uid.is_some()
        })
    {
        return Err("feature refresh origins are empty or task-scoped".into());
    }
    let (feature_id, feature_text) = project
        .state
        .active_features
        .iter()
        .find(|(_, markdown)| {
            crate::domain::ArtifactIdentity::from_markdown(markdown)
                .ok()
                .flatten()
                .is_some_and(|identity| identity.uid == feature.feature_uid)
        })
        .ok_or_else(|| "approved feature document is unavailable".to_owned())?;
    let identity = crate::domain::ArtifactIdentity::from_markdown(feature_text)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "approved feature identity is invalid".to_owned())?;
    let snapshot = crate::core::lifecycle::plan_snapshot::from_feature_with_author(
        &project.state.planning_store,
        feature_id,
        &identity,
        feature_text,
        &crate::core::workflow::feature_contract(feature_text),
    )
    .map_err(|error| error.to_string())?;
    if feature.plan_snapshot_uid.as_deref() != Some(snapshot.uid.as_str())
        || feature.approved_plan_sha256.as_deref() != Some(snapshot.content_hash.as_str())
        || !crate::core::lifecycle::plan_snapshot::is_persisted(
            &project.state.planning_store,
            &snapshot,
        )
        .map_err(|error| error.to_string())?
        || !crate::core::lifecycle::plan_snapshot::has_user_confirmation(
            &project.state.planning_store,
            &snapshot,
        )
        .map_err(|error| error.to_string())?
        || !crate::core::lifecycle::plan_snapshot::has_approved_review(
            &project.state.planning_store,
            &snapshot,
        )
        .map_err(|error| error.to_string())?
    {
        return Err(
            "feature refresh requires the exact persisted, user-confirmed, reviewed plan".into(),
        );
    }
    let current_spec =
        crate::artifacts::product_docs::render_product(&project.state.planning_store)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "current product specification is unavailable".to_owned())?;
    let current_hash =
        crate::core::lifecycle::product_spec::current_product_hash(&project.state.planning_store)
            .map_err(|error| error.to_string())?;
    let workspace_uid = project
        .state
        .planning_store
        .project_id
        .hyphenated()
        .to_string();
    let plan_ref = format!("plan:{}", snapshot.uid);
    let current_ref = format!("current-spec:{current_hash}");
    let mut evidence_manifest = vec![
        evidence_item(
            &plan_ref,
            &workspace_uid,
            "approved-feature-plan",
            feature_text,
        )?,
        evidence_item(
            &current_ref,
            &workspace_uid,
            "current-rendered-product-specification",
            &current_spec,
        )?,
    ];
    for origin in origins {
        let id = format!("spec-trigger:{}", origin.uid);
        let source =
            if origin.trigger == crate::core::lifecycle::SpecSyncTriggerKindV1::ManualRefresh {
                "manual-documentation-refresh-request"
            } else {
                "feature-lifecycle-event"
            };
        let mut content = serde_json::to_string(origin).map_err(|error| error.to_string())?;
        if let Some(request_id) = origin.request_message_id.as_deref() {
            let request =
                manual_request_text(project.task_chats.messages.values().flatten(), request_id)?;
            content.push_str("\nUser request: ");
            content.push_str(request);
        }
        evidence_manifest.push(evidence_item(&id, &workspace_uid, source, &content)?);
    }
    let evidence_ids = evidence_manifest
        .iter()
        .map(|item| item.id.clone())
        .collect();
    let requirement_ids = snapshot
        .requirements
        .iter()
        .map(|requirement| requirement.stable_id.clone())
        .collect::<Vec<_>>();
    let rendered = crate::harness::prompt_catalog::render(&RoleInputV1 {
        role: PromptRole::SpecReconciler,
        workspace_uid,
        feature_uid: feature.feature_uid.clone(),
        task_uid: None,
        original_goal: snapshot.intent,
        approved_spec_ref: Some(snapshot.uid),
        approved_spec_hash: Some(snapshot.content_hash),
        decision_refs: feature.decision_refs.clone(),
        evidence_manifest,
        execution_policy_ref: "feature-refresh/no-implementation-claims/v1".into(),
        output_schema_version: PromptRole::SpecReconciler.output_schema_version().into(),
        payload: RolePayloadV1::SpecSync {
            approved_requirements: snapshot
                .requirements
                .iter()
                .map(|requirement| format!("{}: {}", requirement.stable_id, requirement.text))
                .collect(),
            verified_implementation_refs: Vec::new(),
        },
    })
    .map_err(|error| error.to_string())?;
    let context = crate::harness::prompt_catalog::OutputValidationContext {
        expected_subject_hash: None,
        evidence_ids,
        required_requirements: requirement_ids,
        ..Default::default()
    };
    let (progress_tx, _progress_rx) = mpsc::channel::<LiveProgress>();
    Ok((
        PlanningRequest {
            mode: ExecutionMode::Reconciliation,
            task_id: None,
            execution_workspace: None,
            destination_sha: None,
            authorization_verifier: None,
            prompt_manifest: Some(rendered.manifest),
            reasoning_level: "high".into(),
            telemetry_phase: Some("feature_spec_reconciliation".into()),
            repo_root: project.state.repo_root.clone(),
            runtime_config_source: None,
            prompt_body: rendered.user,
            system_instructions: rendered.system,
            timeout: crate::core::turn::configured_turn_timeout(),
            progress_tx,
            cancel: Arc::new(AtomicBool::new(false)),
            review_identity: None,
        },
        context,
        current_hash,
    ))
}

fn manual_request_text<'a>(
    messages: impl IntoIterator<Item = &'a crate::domain::chatlog::ChatMessage>,
    request_uid: &str,
) -> Result<&'a str, String> {
    let request = messages
        .into_iter()
        .find(|message| {
            message.id == request_uid
                && message.role == crate::domain::chatlog::ChatRole::User
        })
        .ok_or_else(|| {
            "manual documentation refresh request is no longer available in local conversation history".to_owned()
        })?;
    if request.text.trim().is_empty() || request.text.len() > 4096 {
        return Err(
            "manual documentation refresh request is empty or exceeds the prompt limit".into(),
        );
    }
    Ok(&request.text)
}

#[cfg(test)]
mod tests {
    use super::manual_request_text;
    use crate::domain::chatlog::{ChatMessage, ChatRole};

    #[test]
    fn manual_refresh_uses_only_the_exact_bounded_user_message() {
        let user = ChatMessage::new(ChatRole::User, "Refresh the feature documentation", None);
        let other = ChatMessage::new(ChatRole::User, "Unrelated request", None);
        let result = manual_request_text([&other, &user], &user.id).unwrap();
        assert_eq!(result, "Refresh the feature documentation");
        assert!(manual_request_text([&other], &user.id).is_err());
        let mut agent = ChatMessage::new(ChatRole::Agent, "not the user's request", None);
        agent.id.clone_from(&user.id);
        assert!(manual_request_text([&agent], &user.id).is_err());
    }

    #[test]
    fn manual_refresh_rejects_oversized_message_text() {
        let user = ChatMessage::new(ChatRole::User, "x".repeat(4097), None);
        assert!(manual_request_text([&user], &user.id).is_err());
    }
}
